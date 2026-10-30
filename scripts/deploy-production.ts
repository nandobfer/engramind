/**
 * Orquestra o deploy de produção do Engramind.
 *
 * O GitHub Actions constrói a imagem Docker no runner, exporta (`docker save`) e
 * transfere o arquivo por SCP para o servidor de produção. Este script faz a
 * parte remota: confere pré-requisitos, valida o checksum, importa a imagem
 * (`docker load`), recria o serviço com Compose (sem build e sem pull) e valida
 * a disponibilidade.
 *
 * Roda com `npx tsx scripts/deploy-production.ts`. Toda a configuração vem de
 * variáveis de ambiente (ver `aicontext/use-quando-configurar-deploy.md`). As dependências
 * externas (`exec`, `sleep`, `exists`) são injetáveis para permitir testes sem
 * rede nem Docker real.
 *
 * Segurança: nenhuma credencial entra no código. A chave SSH é lida de um
 * arquivo temporário criado pelo workflow; o `known_hosts` é fixo e conferido.
 */
import { execFileSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { basename } from 'node:path'
import { fileURLToPath } from 'node:url'

/** Erro de configuração ou de execução do deploy (mensagem em pt-BR). */
export class DeployError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'DeployError'
  }
}

/** Opções repassadas a um comando externo. */
export interface ExecOptions {
  input?: string
}

/** Resultado normalizado de um comando externo (nunca lança por código de saída). */
export interface ExecResult {
  status: number
  stdout: string
  stderr: string
}

/** Executa um comando externo e devolve status+saída em vez de lançar. */
export type Exec = (file: string, args: string[], options?: ExecOptions) => ExecResult

/** Configuração completa do deploy, já validada. */
export interface DeployConfig {
  host: string
  user: string
  port: number
  sshKeyPath: string
  knownHostsPath: string
  remoteDir: string
  projectName: string
  service: string
  imageRepository: string
  imageTag: string
  imageArchive: string
  imageSha256: string
  platform: string
  expectedArchitecture: string
  healthcheckUrl: string
  healthcheckRetries: number
  healthcheckIntervalMs: number
  healthcheckTimeoutSeconds: number
  composeFiles: string[]
  requiredNetworks: string[]
  cleanupArchive: boolean
}

/** Dependências externas do deploy (injetáveis nos testes). */
export interface DeployDeps {
  exec: Exec
  log: (message: string) => void
  sleep?: (ms: number) => void
  exists?: (path: string) => boolean
}

/** Resultado de um deploy bem-sucedido. */
export interface DeployResult {
  image: string
  previousImage?: string
  steps: string[]
}

const PATTERNS = {
  host: /^[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?$/,
  user: /^[A-Za-z0-9._-]+$/,
  remoteDir: /^(?:\/[A-Za-z0-9._-]+)+$/,
  projectName: /^[a-z0-9][a-z0-9_-]*$/,
  service: /^[A-Za-z0-9._-]+$/,
  repository: /^[a-z0-9]+(?:[._/-][a-z0-9]+)*$/,
  tag: /^[A-Za-z0-9_][A-Za-z0-9._-]{0,127}$/,
  sha256: /^[0-9a-f]{64}$/,
  architecture: /^[A-Za-z0-9_]+$/,
  platform: /^linux\/(amd64|arm64)$/,
  healthcheckUrl: /^https?:\/\/[^\s'"<>]+$/,
  filename: /^[A-Za-z0-9._-]+$/,
  network: /^[A-Za-z0-9][A-Za-z0-9._-]*$/,
} as const

/** Converte `linux/amd64` na arquitetura reportada por `docker info`. */
export function architectureForPlatform(platform: string): string {
  switch (platform) {
    case 'linux/amd64':
      return 'x86_64'
    case 'linux/arm64':
      return 'aarch64'
    default:
      throw new DeployError(`Plataforma não suportada: ${platform}`)
  }
}

/** Referência completa da imagem (`repositório:tag`). */
export function imageRef(config: Pick<DeployConfig, 'imageRepository' | 'imageTag'>): string {
  return `${config.imageRepository}:${config.imageTag}`
}

/** Escapa um valor para uso seguro como argumento do shell remoto. */
export function shellQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`
}

function requireEnv(env: NodeJS.ProcessEnv, name: string): string {
  const value = env[name]?.trim()
  if (!value) {
    throw new DeployError(`Variável de ambiente obrigatória ausente: ${name}`)
  }
  return value
}

function requireMatch(value: string, pattern: RegExp, name: string): string {
  if (!pattern.test(value)) {
    throw new DeployError(`Valor inválido para ${name}: ${value}`)
  }
  return value
}

function readInteger(env: NodeJS.ProcessEnv, name: string, fallback: number): number {
  const raw = env[name]?.trim()
  if (!raw) return fallback
  const value = Number(raw)
  if (!Number.isInteger(value) || value <= 0) {
    throw new DeployError(`Valor inválido para ${name}: ${raw}`)
  }
  return value
}

function readBoolean(env: NodeJS.ProcessEnv, name: string, fallback: boolean): boolean {
  const raw = env[name]?.trim().toLowerCase()
  if (!raw) return fallback
  if (['1', 'true', 'sim', 'yes'].includes(raw)) return true
  if (['0', 'false', 'nao', 'não', 'no'].includes(raw)) return false
  throw new DeployError(`Valor inválido para ${name}: ${raw}`)
}

function readList(env: NodeJS.ProcessEnv, name: string, fallback: string[]): string[] {
  const raw = env[name]?.trim()
  if (!raw) return fallback
  return raw
    .split(',')
    .map((item) => item.trim())
    .filter((item) => item.length > 0)
}

/**
 * Lê e valida a configuração a partir do ambiente. Lança `DeployError` na
 * primeira inconsistência, antes de qualquer comando remoto.
 */
export function readConfig(env: NodeJS.ProcessEnv = process.env): DeployConfig {
  const platform = env.DEPLOY_PLATFORM?.trim() || 'linux/amd64'
  requireMatch(platform, PATTERNS.platform, 'DEPLOY_PLATFORM')
  const expectedArchitecture = requireMatch(
    env.DEPLOY_ARCHITECTURE?.trim() || architectureForPlatform(platform),
    PATTERNS.architecture,
    'DEPLOY_ARCHITECTURE',
  )

  const port = readInteger(env, 'DEPLOY_SSH_PORT', 22)
  if (port > 65535) {
    throw new DeployError(`Valor inválido para DEPLOY_SSH_PORT: ${port}`)
  }

  const imageArchive = requireEnv(env, 'DEPLOY_IMAGE_ARCHIVE')
  requireMatch(basename(imageArchive), PATTERNS.filename, 'DEPLOY_IMAGE_ARCHIVE (nome do arquivo)')

  const composeFiles = readList(env, 'DEPLOY_COMPOSE_FILES', ['docker-compose.yml', 'docker-compose.deploy.yml'])
  if (composeFiles.length === 0) {
    throw new DeployError('DEPLOY_COMPOSE_FILES não pode ser vazio')
  }

  const requiredNetworks = readList(env, 'DEPLOY_REQUIRED_NETWORKS', ['engramind-network', 'minio-network'])

  return {
    host: requireMatch(requireEnv(env, 'DEPLOY_HOST'), PATTERNS.host, 'DEPLOY_HOST'),
    user: requireMatch(requireEnv(env, 'DEPLOY_SSH_USER'), PATTERNS.user, 'DEPLOY_SSH_USER'),
    port,
    sshKeyPath: requireEnv(env, 'DEPLOY_SSH_KEY'),
    knownHostsPath: requireEnv(env, 'DEPLOY_KNOWN_HOSTS'),
    remoteDir: requireMatch(requireEnv(env, 'DEPLOY_REMOTE_DIR'), PATTERNS.remoteDir, 'DEPLOY_REMOTE_DIR'),
    projectName: requireMatch(requireEnv(env, 'DEPLOY_PROJECT_NAME'), PATTERNS.projectName, 'DEPLOY_PROJECT_NAME'),
    service: requireMatch(env.DEPLOY_SERVICE?.trim() || 'api', PATTERNS.service, 'DEPLOY_SERVICE'),
    imageRepository: requireMatch(
      env.DEPLOY_IMAGE_REPOSITORY?.trim() || 'engramind-api',
      PATTERNS.repository,
      'DEPLOY_IMAGE_REPOSITORY',
    ),
    imageTag: requireMatch(requireEnv(env, 'DEPLOY_IMAGE_TAG'), PATTERNS.tag, 'DEPLOY_IMAGE_TAG'),
    imageArchive,
    imageSha256: requireMatch(requireEnv(env, 'DEPLOY_IMAGE_SHA256'), PATTERNS.sha256, 'DEPLOY_IMAGE_SHA256'),
    platform,
    expectedArchitecture,
    healthcheckUrl: requireMatch(
      requireEnv(env, 'DEPLOY_HEALTHCHECK_URL'),
      PATTERNS.healthcheckUrl,
      'DEPLOY_HEALTHCHECK_URL',
    ),
    healthcheckRetries: readInteger(env, 'DEPLOY_HEALTHCHECK_RETRIES', 30),
    healthcheckIntervalMs: readInteger(env, 'DEPLOY_HEALTHCHECK_INTERVAL_MS', 5000),
    healthcheckTimeoutSeconds: readInteger(env, 'DEPLOY_HEALTHCHECK_TIMEOUT_SECONDS', 10),
    composeFiles,
    requiredNetworks: requiredNetworks.map((network) => requireMatch(network, PATTERNS.network, 'DEPLOY_REQUIRED_NETWORKS')),
    cleanupArchive: readBoolean(env, 'DEPLOY_CLEANUP_ARCHIVE', true),
  }
}

/** Comando externo padrão: converte falha de processo em status != 0. */
export const defaultExec: Exec = (file, args, options = {}) => {
  try {
    const stdout = execFileSync(file, args, {
      encoding: 'utf8',
      input: options.input,
      stdio: options.input ? ['pipe', 'pipe', 'pipe'] : ['ignore', 'pipe', 'pipe'],
    })
    return { status: 0, stdout: stdout ?? '', stderr: '' }
  } catch (error) {
    const failure = error as { status?: number | null; stdout?: string; stderr?: string }
    return {
      status: typeof failure.status === 'number' ? failure.status : 1,
      stdout: failure.stdout?.toString() ?? '',
      stderr: failure.stderr?.toString() ?? '',
    }
  }
}

/** Espera síncrona padrão, usada entre tentativas do healthcheck. */
export function defaultSleep(ms: number): void {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms)
}

function sshBaseArgs(config: DeployConfig): string[] {
  return [
    '-i',
    config.sshKeyPath,
    '-p',
    String(config.port),
    '-o',
    `UserKnownHostsFile=${config.knownHostsPath}`,
    '-o',
    'StrictHostKeyChecking=yes',
    '-o',
    'BatchMode=yes',
    '-o',
    'ConnectTimeout=15',
  ]
}

function scpBaseArgs(config: DeployConfig): string[] {
  return [
    '-i',
    config.sshKeyPath,
    '-P',
    String(config.port),
    '-o',
    `UserKnownHostsFile=${config.knownHostsPath}`,
    '-o',
    'StrictHostKeyChecking=yes',
    '-o',
    'BatchMode=yes',
    '-o',
    'ConnectTimeout=15',
  ]
}

/** Confere que os arquivos locais exigidos existem antes de tocar no servidor. */
export function assertLocalPrerequisites(config: DeployConfig, exists: (path: string) => boolean): void {
  const files = [config.sshKeyPath, config.knownHostsPath, config.imageArchive, ...config.composeFiles]
  const missing = files.filter((file) => !exists(file))
  if (missing.length > 0) {
    throw new DeployError(`Arquivos locais ausentes: ${missing.join(', ')}`)
  }
}

/**
 * Executa o deploy completo. Lança `DeployError` na primeira falha, sem deixar
 * o serviço parcialmente atualizado quando possível.
 */
export function runDeploy(config: DeployConfig, deps: DeployDeps): DeployResult {
  const { exec, log } = deps
  const sleep = deps.sleep ?? defaultSleep
  const exists = deps.exists ?? existsSync

  assertLocalPrerequisites(config, exists)

  const target = `${config.user}@${config.host}`
  const image = imageRef(config)
  const archiveName = basename(config.imageArchive)
  const remoteDir = config.remoteDir.replace(/\/+$/, '')
  const remoteFile = (name: string): string => `${remoteDir}/${name}`
  const steps: string[] = []
  let previousImage: string | undefined

  const ssh = (remoteCommand: string): ExecResult => exec('ssh', [...sshBaseArgs(config), target, remoteCommand])

  const must = (result: ExecResult, label: string): ExecResult => {
    if (result.status !== 0) {
      const detail = (result.stderr || result.stdout).trim()
      throw new DeployError(`${label} falhou (código ${result.status})${detail ? `: ${detail}` : ''}`)
    }
    return result
  }

  const step = (name: string, action: () => void): void => {
    log(`▶ ${name}`)
    action()
    steps.push(name)
    log(`✔ ${name}`)
  }

  const remoteComposePaths = config.composeFiles.map((file) => remoteFile(basename(file)))
  const composeCommand = (args: string): string => {
    const files = remoteComposePaths.map((path) => `-f ${shellQuote(path)}`).join(' ')
    return (
      `API_IMAGE=${shellQuote(image)} docker compose` +
      ` --project-directory ${shellQuote(remoteDir)} ${files} -p ${shellQuote(config.projectName)} ${args}`
    )
  }

  try {
    step('Pré-verificações no servidor', () => {
      const checks = [
        'command -v docker >/dev/null 2>&1 || { echo "docker ausente"; exit 1; }',
        'docker compose version >/dev/null 2>&1 || { echo "docker compose ausente"; exit 1; }',
        `test -f ${shellQuote(remoteFile('.env'))} || { echo ".env ausente em ${remoteDir}"; exit 1; }`,
        ...config.requiredNetworks.map(
          (network) =>
            `docker network inspect ${shellQuote(network)} >/dev/null 2>&1 || { echo "rede ${network} ausente"; exit 1; }`,
        ),
      ]
      must(ssh(`set -e\n${checks.join('\n')}`), 'Pré-verificações no servidor')
    })

    step('Verificando a arquitetura do servidor', () => {
      const result = must(ssh('docker info --format "{{.Architecture}}"'), 'Verificação de arquitetura')
      const remoteArchitecture = result.stdout.trim()
      if (remoteArchitecture !== config.expectedArchitecture) {
        throw new DeployError(
          `Arquitetura do servidor (${remoteArchitecture}) difere da esperada (${config.expectedArchitecture})`,
        )
      }
    })

    step('Preparando o diretório de release', () => {
      must(ssh(`mkdir -p ${shellQuote(remoteDir)}`), 'Criação do diretório de release')
    })

    step('Transferindo imagem e arquivos de configuração', () => {
      const sources = [config.imageArchive, ...config.composeFiles]
      must(
        exec('scp', [...scpBaseArgs(config), ...sources, `${target}:${remoteDir}/`]),
        'Transferência SCP',
      )
    })

    step('Conferindo o checksum da imagem', () => {
      const command =
        `cd ${shellQuote(remoteDir)} && ` +
        `printf '%s  %s\\n' ${shellQuote(config.imageSha256)} ${shellQuote(archiveName)} | sha256sum -c -`
      must(ssh(command), 'Conferência de checksum')
    })

    step('Importando a imagem (docker load)', () => {
      must(ssh(`docker load --input ${shellQuote(remoteFile(archiveName))}`), 'docker load')
    })

    step('Confirmando a imagem importada', () => {
      const result = must(ssh(`docker image inspect ${shellQuote(image)} --format '{{.Id}}'`), 'docker image inspect')
      if (!result.stdout.trim()) {
        throw new DeployError(`Imagem ${image} não foi encontrada após o load`)
      }
    })

    // Guarda a imagem em execução para orientar um rollback em caso de falha.
    const previousPs = exec('ssh', [...sshBaseArgs(config), target, composeCommand(`ps -q ${shellQuote(config.service)}`)])
    const previousId = previousPs.stdout.trim().split('\n').filter(Boolean)[0]
    if (previousPs.status === 0 && previousId) {
      const previousInspect = exec(
        'ssh',
        [...sshBaseArgs(config), target, `docker inspect --format '{{.Config.Image}}' ${shellQuote(previousId)}`],
      )
      if (previousInspect.status === 0 && previousInspect.stdout.trim()) {
        previousImage = previousInspect.stdout.trim()
        log(`Imagem em execução antes do deploy: ${previousImage}`)
      }
    }

    step('Atualizando o serviço', () => {
      must(
        ssh(composeCommand(`up -d --no-build --pull never ${shellQuote(config.service)}`)),
        'docker compose up',
      )
    })

    step('Confirmando a imagem do container', () => {
      const ps = must(ssh(composeCommand(`ps -q ${shellQuote(config.service)}`)), 'docker compose ps')
      const containerId = ps.stdout.trim().split('\n').filter(Boolean)[0]
      if (!containerId) {
        throw new DeployError('Container do serviço não encontrado após a atualização')
      }
      const inspect = must(
        ssh(`docker inspect --format '{{.Config.Image}}' ${shellQuote(containerId)}`),
        'docker inspect do container',
      )
      const runningImage = inspect.stdout.trim()
      if (runningImage !== image) {
        throw new DeployError(`O container está usando ${runningImage}, esperado ${image}`)
      }
    })

    step('Verificando a disponibilidade (healthcheck)', () => {
      for (let attempt = 1; attempt <= config.healthcheckRetries; attempt += 1) {
        const result = ssh(
          `curl -fsS -o /dev/null --max-time ${config.healthcheckTimeoutSeconds} ${shellQuote(config.healthcheckUrl)}`,
        )
        if (result.status === 0) {
          log(`  healthcheck respondeu na tentativa ${attempt}`)
          return
        }
        if (attempt === config.healthcheckRetries) {
          throw new DeployError(
            `Healthcheck falhou após ${config.healthcheckRetries} tentativas: ${config.healthcheckUrl}`,
          )
        }
        sleep(config.healthcheckIntervalMs)
      }
    })

    if (config.cleanupArchive) {
      step('Removendo temporários remotos', () => {
        const result = ssh(`rm -f ${shellQuote(remoteFile(archiveName))}`)
        if (result.status !== 0) {
          log('  aviso: não foi possível remover o arquivo da imagem no servidor')
        }
      })
    }
  } catch (error) {
    if (error instanceof DeployError && previousImage) {
      throw new DeployError(`${error.message}\nImagem anterior para rollback: ${previousImage}`)
    }
    throw error
  }

  return { image, previousImage, steps }
}

/** Ponto de entrada: lê o ambiente, executa o deploy e devolve o código de saída. */
export function main(env: NodeJS.ProcessEnv = process.env): number {
  const log = (message: string): void => {
    process.stdout.write(`[deploy] ${message}\n`)
  }

  let config: DeployConfig
  try {
    config = readConfig(env)
  } catch (error) {
    process.stderr.write(`[deploy] Configuração inválida: ${error instanceof Error ? error.message : String(error)}\n`)
    return 1
  }

  try {
    const result = runDeploy(config, { exec: defaultExec, log })
    log(`Deploy concluído: ${result.image}`)
    if (result.previousImage) {
      log(`Imagem anterior (rollback): ${result.previousImage}`)
    }
    return 0
  } catch (error) {
    process.stderr.write(`[deploy] Falha no deploy: ${error instanceof Error ? error.message : String(error)}\n`)
    return 1
  }
}

function isDirectRun(): boolean {
  const entry = process.argv[1]
  if (!entry) return false
  try {
    return fileURLToPath(import.meta.url) === entry
  } catch {
    return false
  }
}

if (isDirectRun()) {
  process.exitCode = main()
}
