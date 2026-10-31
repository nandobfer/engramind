//! Árvore de comandos. O binário atende pelos nomes `engramind` e `eng` (D-003). `serve`,
//! `migrate` e `doctor` abrem o banco; os demais comandos são clientes HTTP da API, um por
//! operação do catálogo (`core::operations`), com os mesmos parâmetros e códigos de erro.

pub mod client;
pub mod help;
pub mod output;
pub mod profiles;
pub mod remote;

use std::process::ExitCode;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use crate::{
    HttpSettings, Settings, build_app,
    config::DatabaseSettings,
    core::{
        self, Core,
        bootstrap::BootstrapOutcome,
        pepper::{self, PepperCheck},
    },
    doctor::{self, CheckStatus},
    mcp::McpSettings,
    observability::{self, LogTarget},
    store::SqliteStore,
};

#[derive(Debug, Parser)]
#[command(
    name = "engramind",
    version,
    about = "Rede de memórias para agentes de IA",
    long_about = "Rede de memórias para agentes de IA. Também invocável como `eng`.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Saída em JSON.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(flatten)]
    pub connection: ConnectionArgs,
    #[command(subcommand)]
    pub command: Command,
}

/// Conexão com a API: flags, depois variáveis de ambiente, depois o perfil.
#[derive(Debug, Clone, Default, Args)]
pub struct ConnectionArgs {
    /// URL da API (ou ENGRAMIND_URL).
    #[arg(long, global = true, env = "ENGRAMIND_URL", value_name = "URL")]
    pub url: Option<String>,
    /// Token `eng.<token_id>.<segredo>` (ou ENGRAMIND_TOKEN).
    #[arg(
        long,
        global = true,
        env = "ENGRAMIND_TOKEN",
        hide_env_values = true,
        value_name = "TOKEN"
    )]
    pub token: Option<String>,
    /// Canais padrão da conexão, separados por vírgula (ou ENGRAMIND_CHANNELS).
    #[arg(long, global = true, env = "ENGRAMIND_CHANNELS", value_name = "CANAIS")]
    pub channels: Option<String>,
    /// Perfil de `~/.config/engramind/config.toml` (padrão: `default_profile`).
    #[arg(long, global = true, value_name = "PERFIL")]
    pub profile: Option<String>,
}

/// Chave de idempotência das mutações.
#[derive(Debug, Clone, Default, Args)]
pub struct IdempotencyArgs {
    /// Repete a resposta gravada para a mesma chave e o mesmo corpo por 24 h.
    #[arg(long, value_name = "CHAVE")]
    pub idempotency_key: Option<String>,
}

/// Paginação das listas.
#[derive(Debug, Clone, Default, Args)]
pub struct PageArgs {
    /// Cursor devolvido em `next_cursor`.
    #[arg(long)]
    pub cursor: Option<String>,
    /// Itens por página, de 1 a 200 (padrão 50).
    #[arg(long)]
    pub limit: Option<u32>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Sobe o servidor HTTP (aplica as migrações pendentes antes).
    Serve,
    /// Aplica as migrações pendentes e sai.
    Migrate,
    /// Confere integridade, migrações, extensões e a idade da última réplica.
    Doctor,
    /// Descreve os comandos (`--json`: árvore completa, base da skill e dos testes).
    Help,
    /// Gerencia os perfis de conexão locais.
    #[command(subcommand)]
    Profiles(ProfilesCommand),
    /// Identifica o token e lista as operações permitidas.
    Whoami,
    /// Apps (token admin).
    #[command(subcommand)]
    Apps(AppsCommand),
    /// Agentes (token admin).
    #[command(subcommand)]
    Agents(AgentsCommand),
    /// Tokens (token admin).
    #[command(subcommand)]
    Tokens(TokensCommand),
    /// Canais (leitura: qualquer token; mudanças: token admin).
    #[command(subcommand)]
    Channels(ChannelsCommand),
    /// Assinaturas (leitura: qualquer token; mudanças: token admin).
    #[command(subcommand)]
    Subscriptions(SubscriptionsCommand),
    /// Imprime o bloco de memórias para o prompt (`--json`: o objeto completo com `version`).
    Context {
        /// `tagged`, `markdown` ou `json` (padrão `context.default_format`).
        #[arg(long)]
        format: Option<String>,
        /// `version` já em uso: sem mudança, nada é impresso (`--json`: `not_modified: true`)
        /// e a saída é 0.
        #[arg(long)]
        if_none_match: Option<String>,
    },
    /// Memórias dos canais alcançados (`--channels` restringe).
    #[command(subcommand)]
    Memories(MemoriesCommand),
    /// Dicas baratas (`#id · título · canal`) para um texto, pela busca vetorial com limiar alto.
    Hints {
        /// Texto do turno; as palavras são unidas por espaço.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
        /// De 1 a 20 (padrão `search.hint_limit`).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Configurações (token admin).
    #[command(subcommand)]
    Settings(SettingsCommand),
    /// Segredos de configuração (token admin).
    #[command(subcommand)]
    Secrets(SecretsCommand),
    /// Embeddings: reindexação (token admin).
    #[command(subcommand)]
    Embeddings(EmbeddingsCommand),
    /// Mostra versão, banco, backup, contagens, fila, Redator e embeddings (token admin).
    Status,
    /// Fila de jobs (token admin).
    #[command(subcommand)]
    Jobs(JobsCommand),
    /// Chamadas ao LLM (token admin).
    #[command(subcommand, name = "llm-calls")]
    LlmCalls(LlmCallsCommand),
    /// Auditoria (token admin).
    #[command(subcommand)]
    Audit(AuditCommand),
    /// Documentação da versão em execução.
    #[command(subcommand)]
    Docs(DocsCommand),
}

#[derive(Debug, Subcommand)]
pub enum ProfilesCommand {
    /// Grava um perfil com `--url`, `--token` e `--channels` (arquivo com permissão 0600).
    Add {
        name: String,
        /// Torna este o perfil padrão.
        #[arg(long)]
        default: bool,
    },
    /// Lista os perfis, com o token mascarado.
    List,
    /// Remove um perfil.
    Remove { name: String },
    /// Define o perfil padrão.
    Use { name: String },
}

#[derive(Debug, Subcommand)]
pub enum AppsCommand {
    /// Lista os apps.
    List {
        #[command(flatten)]
        page: PageArgs,
    },
    /// Mostra um app (id ou slug).
    Get { app: String },
    /// Cria um app.
    Create {
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Altera ou desativa um app.
    Update {
        app: String,
        #[command(flatten)]
        changes: UpdateArgs,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Exclui um app com seus agentes e tokens.
    Delete {
        app: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

/// Campos de `update` de apps e agentes.
#[derive(Debug, Clone, Default, Args)]
pub struct UpdateArgs {
    #[arg(long)]
    pub name: Option<String>,
    /// Nova descrição; `""` limpa.
    #[arg(long)]
    pub description: Option<String>,
    /// Desativa: os tokens deixam de autenticar.
    #[arg(long, conflicts_with = "enable")]
    pub disable: bool,
    /// Reativa.
    #[arg(long)]
    pub enable: bool,
}

#[derive(Debug, Subcommand)]
pub enum AgentsCommand {
    /// Lista os agentes.
    List {
        /// Só os agentes deste app.
        #[arg(long)]
        app: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Mostra um agente (id ou app/agente).
    Get { agent: String },
    /// Cria um agente num app.
    Create {
        #[arg(long)]
        app: String,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Altera ou desativa um agente.
    Update {
        agent: String,
        #[command(flatten)]
        changes: UpdateArgs,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Exclui um agente com seus tokens.
    Delete {
        agent: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum TokensCommand {
    /// Lista os tokens, sem segredos.
    List {
        #[arg(long)]
        app: Option<String>,
        #[arg(long)]
        agent: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Emite um token; o segredo aparece só nesta saída.
    Issue {
        /// App dono (id ou slug); opcional com --agent.
        #[arg(long)]
        app: Option<String>,
        /// Agente (id ou app/agente); obrigatório para --kind agent.
        #[arg(long)]
        agent: Option<String>,
        /// `agent` ou `admin`.
        #[arg(long, default_value = "agent")]
        kind: String,
        /// `read`, `read,write`, `read,delete` ou `read,write,delete`.
        #[arg(long, default_value = "read")]
        scopes: String,
        #[arg(long)]
        label: Option<String>,
        /// RFC 3339 no futuro.
        #[arg(long)]
        expires_at: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Altera rótulo, escopos ou expiração de um token.
    Update {
        /// Id do token (`tk_…`).
        #[arg(value_name = "TOKEN_ID")]
        token_id: String,
        /// Novo rótulo; `""` limpa.
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        scopes: Option<String>,
        #[arg(long, conflicts_with = "no_expiration")]
        expires_at: Option<String>,
        /// Remove a expiração.
        #[arg(long)]
        no_expiration: bool,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Revoga um token imediatamente.
    Revoke {
        /// Id do token (`tk_…`).
        #[arg(value_name = "TOKEN_ID")]
        token_id: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum ChannelsCommand {
    /// Lista os canais alcançados pelo token (`--channels` restringe).
    List {
        /// Inclui os canais arquivados.
        #[arg(long)]
        include_archived: bool,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Mostra um canal (id ou slug) com o alcance do token.
    Get { channel: String },
    /// Cria um canal.
    Create {
        /// Hierárquico com `/`, por exemplo `eng/global`; imutável.
        #[arg(long)]
        slug: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        /// De -1000 a 1000 (padrão 0).
        #[arg(long, allow_negative_numbers = true)]
        priority: Option<i64>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Altera nome, descrição ou prioridade de um canal.
    Update {
        channel: String,
        #[arg(long)]
        name: Option<String>,
        /// Nova descrição; `""` limpa.
        #[arg(long)]
        description: Option<String>,
        #[arg(long, allow_negative_numbers = true)]
        priority: Option<i64>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Arquiva um canal.
    Archive {
        channel: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Restaura um canal arquivado.
    Restore {
        channel: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Exclui um canal arquivado (com as memórias dele).
    Delete {
        channel: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Contagens das memórias de um canal.
    Stats { channel: String },
    /// Sobreposições de configuração de um canal (token admin).
    #[command(subcommand)]
    Settings(ChannelSettingsCommand),
}

#[derive(Debug, Subcommand)]
pub enum ChannelSettingsCommand {
    /// Lê as configurações do canal com as sobreposições.
    Get { channel: String },
    /// Altera uma sobreposição; o valor é lido como JSON (`null` remove a sobreposição).
    Set {
        channel: String,
        key: String,
        value: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

/// Status e campos da saída das memórias.
#[derive(Debug, Clone, Default, Args)]
pub struct FieldsArgs {
    /// `full` (padrão) ou `title` (só id, título, canal, categoria e status).
    #[arg(long)]
    pub fields: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum MemoriesCommand {
    /// Lista as memórias dos canais alcançados (`--channels` restringe).
    List {
        /// Categorias separadas por vírgula.
        #[arg(long)]
        categories: Option<String>,
        /// `active`, `permanent`, `archived`, separados por vírgula (padrão active,permanent).
        #[arg(long)]
        status: Option<String>,
        /// App (id ou slug) ou agente (id ou app/agente).
        #[arg(long)]
        author: Option<String>,
        /// Só as alteradas a partir deste instante (RFC 3339).
        #[arg(long)]
        since: Option<String>,
        #[command(flatten)]
        fields: FieldsArgs,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Mostra uma memória.
    Get {
        /// Id da memória (`m_…`).
        memory: String,
    },
    /// Busca memórias por sentido e por texto (acentos e caixa são ignorados).
    Search {
        /// Consulta; as palavras são unidas por espaço.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Categorias separadas por vírgula.
        #[arg(long)]
        categories: Option<String>,
        /// De 1 a 50 (padrão `search.default_limit`).
        #[arg(long)]
        limit: Option<u32>,
        /// Inclui as memórias arquivadas.
        #[arg(long)]
        include_archived: bool,
        /// Ids a deixar de fora, separados por vírgula.
        #[arg(long)]
        exclude_ids: Option<String>,
        /// Deixa de fora as memórias com texto completo no contexto (`engramind context`).
        #[arg(long)]
        exclude_context: bool,
        #[command(flatten)]
        fields: FieldsArgs,
    },
    /// Lista as revisões de uma memória.
    History {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Grava uma memória num canal.
    Create {
        /// Canal (id ou slug) com write efetivo.
        #[arg(long)]
        channel: String,
        /// Texto da memória; `-` lê da entrada padrão.
        #[arg(long)]
        text: String,
        /// Categoria (padrão: a primeira de categories.list).
        #[arg(long)]
        category: Option<String>,
        /// `active` (padrão) ou `permanent`.
        #[arg(long)]
        status: Option<String>,
        /// Origem, por exemplo `curator`.
        #[arg(long)]
        origin_source: Option<String>,
        /// Referência da origem, por exemplo o caso que gerou a memória.
        #[arg(long, requires = "origin_source")]
        origin_label: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Edita o texto ou a categoria de uma memória.
    Update {
        /// Id da memória (`m_…`).
        memory: String,
        /// Texto novo; `-` lê da entrada padrão.
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Versão lida; divergente dá MEMORY_VERSION_CONFLICT.
        #[arg(long)]
        expected_version: Option<i64>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Arquiva uma memória.
    Archive {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Restaura uma memória arquivada.
    Restore {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Fixa uma memória (status permanent).
    Pin {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Desafixa uma memória.
    Unpin {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Move uma memória para outro canal.
    Move {
        /// Id da memória (`m_…`).
        memory: String,
        /// Canal de destino (id ou slug).
        #[arg(long)]
        to: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Restaura o conteúdo de uma versão anterior numa revisão nova.
    Revert {
        /// Id da memória (`m_…`).
        memory: String,
        /// Versão do histórico a restaurar.
        #[arg(long = "version", value_name = "N")]
        to_version: i64,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Exclui uma memória, mantendo a última revisão.
    Delete {
        /// Id da memória (`m_…`).
        memory: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum SubscriptionsCommand {
    /// Lista as assinaturas (token agent: só as próprias).
    List {
        /// Só as deste agente (id ou app/agente).
        #[arg(long)]
        agent: Option<String>,
        /// Só as que alcançam este canal (id ou slug).
        #[arg(long)]
        channel: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Assina um agente a um canal exato ou a um prefixo (`eng/*`).
    Add {
        /// Agente (id ou app/agente).
        #[arg(long)]
        agent: String,
        /// Canal exato ou prefixo terminado em `*`.
        #[arg(long)]
        target: String,
        /// `inject`, `search` ou `manage`.
        #[arg(long, default_value = "inject")]
        mode: String,
        /// Restringe os escopos do token: `read`, `read,write`…
        #[arg(long)]
        access: Option<String>,
        /// Sobrepõe a prioridade do canal (-1000 a 1000).
        #[arg(long, allow_negative_numbers = true)]
        priority: Option<i64>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Altera modo, acesso ou prioridade de uma assinatura.
    Update {
        /// Id da assinatura (`sub_…`).
        subscription: String,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long, conflicts_with = "token_access")]
        access: Option<String>,
        /// Volta aos escopos do token.
        #[arg(long)]
        token_access: bool,
        #[arg(
            long,
            allow_negative_numbers = true,
            conflicts_with = "channel_priority"
        )]
        priority: Option<i64>,
        /// Volta à prioridade do canal.
        #[arg(long)]
        channel_priority: bool,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Cancela uma assinatura.
    Remove {
        /// Id da assinatura (`sub_…`).
        subscription: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum SettingsCommand {
    /// Lê as configurações (todas ou uma chave).
    Get { key: Option<String> },
    /// Altera uma configuração; o valor é lido como JSON (`600`, `true`, `null` volta ao padrão).
    Set {
        key: String,
        value: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Testa o Redator com um texto, sem gravar a memória.
    TestRedator {
        /// Texto de exemplo; `-` lê da entrada padrão.
        #[arg(long)]
        text: String,
        /// Configuração proposta, só para o teste, como objeto JSON de chaves `redator.*`.
        #[arg(long, value_name = "JSON")]
        settings: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Testa a configuração de embedding com um texto, sem gravar nada.
    TestEmbedding {
        /// Texto de exemplo; `-` lê da entrada padrão.
        #[arg(long)]
        text: String,
        /// Configuração proposta, só para o teste, como objeto JSON de chaves `embedding.*`.
        #[arg(long, value_name = "JSON")]
        settings: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
    /// Lista os modelos de embedding de um provedor (`local`, `ollama`, `openai_compatible`).
    EmbeddingModels {
        #[arg(long)]
        provider: String,
        /// URL do provedor remoto; vazia usa `embedding.base_url`.
        #[arg(long)]
        base_url: Option<String>,
        /// Lê a chave do provedor da entrada padrão (vazia usa `embedding.api_key`).
        #[arg(long)]
        api_key_stdin: bool,
        /// Chave lida da entrada padrão.
        #[arg(skip)]
        api_key: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum EmbeddingsCommand {
    /// Revetoriza todas as memórias com o modelo vigente (progresso em `engramind status`).
    Reembed {
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum SecretsCommand {
    /// Grava um segredo lido da entrada padrão (nunca de argumento); `--delete` apaga.
    Set {
        /// Chave do segredo, por exemplo `redator.api_key`.
        key: String,
        /// Apaga o segredo em vez de gravar.
        #[arg(long)]
        delete: bool,
        /// Valor lido da entrada padrão.
        #[arg(skip)]
        value: Option<String>,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum JobsCommand {
    /// Lista os jobs da fila, do mais recente ao mais antigo.
    List {
        /// `queued`, `running`, `done` ou `failed`.
        #[arg(long)]
        state: Option<String>,
        /// Tipo do job, por exemplo `redact`.
        #[arg(long)]
        kind: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
    /// Recoloca na fila um job que falhou.
    Retry {
        /// Id numérico do job.
        job: String,
        #[command(flatten)]
        idempotency: IdempotencyArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum LlmCallsCommand {
    /// Lista as chamadas ao LLM, da mais recente à mais antiga.
    List {
        /// Só a partir deste instante (RFC 3339).
        #[arg(long)]
        since: Option<String>,
        /// `redact` ou `test`.
        #[arg(long)]
        purpose: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Lista os eventos, do mais recente ao mais antigo.
    List {
        #[arg(long)]
        entity: Option<String>,
        #[arg(long)]
        entity_id: Option<String>,
        #[arg(long)]
        action: Option<String>,
        #[command(flatten)]
        page: PageArgs,
    },
}

#[derive(Debug, Subcommand)]
pub enum DocsCommand {
    /// Visão geral do serviço e dos guias disponíveis ao token.
    Overview,
    /// Lista os guias com seções e operações.
    Guides,
    /// Lê um guia ou uma seção dele.
    Guide {
        guide: String,
        #[arg(long)]
        section: Option<String>,
    },
    /// Busca nos guias e nas operações por palavras-chave.
    Search {
        #[arg(required = true, num_args = 1..)]
        terms: Vec<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Lista as operações visíveis ao token.
    Operations {
        #[arg(long)]
        guide: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Contrato completo de uma operação.
    Operation { operation: String },
    /// Esquema OpenAPI com as referências.
    Schema { schema: String },
}

pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Serve => serve().await.map(|()| ExitCode::SUCCESS),
        Command::Migrate => migrate(cli.json).await,
        Command::Doctor => run_doctor(cli.json).await,
        Command::Help => Ok(help::run(cli.json)),
        Command::Profiles(command) => Ok(remote::profiles(command, &cli.connection, cli.json)
            .unwrap_or_else(|error| {
                output::error_envelope(
                    &client::local_error("CLI_CONFIG_INVALID", format!("{error:#}")),
                    cli.json,
                );
                ExitCode::from(client::local_exit_code("CLI_CONFIG_INVALID"))
            })),
        command => Ok(remote::run(command, &cli.connection, cli.json).await),
    }
}

async fn serve() -> anyhow::Result<()> {
    observability::init_logging(LogTarget::Stdout).context("observabilidade invalida")?;
    let settings = Settings::from_env().context("configuracao invalida")?;
    let store = SqliteStore::open(&settings.database.path, settings.database.read_pool_size)
        .context("falha ao abrir o banco")?;
    let migrations = store.migrate().await.context("falha ao migrar o banco")?;
    if migrations.newly_applied() > 0 {
        tracing::info!(
            event = "database.migrated",
            before = migrations.before,
            after = migrations.after,
            "migracoes aplicadas"
        );
    }
    match pepper::enforce(&store, &settings.token_pepper)
        .await
        .context("falha ao validar TOKEN_PEPPER")?
    {
        PepperCheck::Enrolled => {
            tracing::info!(
                event = "security.pepper_enrolled",
                "verificador do TOKEN_PEPPER registrado"
            );
        }
        PepperCheck::Verified => {}
    }

    let core = Core::new(
        store,
        settings.token_pepper.clone(),
        settings.secrets_key.clone(),
        settings.database.clone(),
        settings.models.clone(),
    );
    core.reload_settings()
        .await
        .context("falha ao carregar as configuracoes")?;
    core.sync_embeddings()
        .await
        .context("falha ao sincronizar o modelo de embedding")?;
    match core
        .bootstrap(settings.bootstrap_admin_token.as_ref())
        .await
        .context("falha no bootstrap do token admin")?
    {
        BootstrapOutcome::Created(token_id) => tracing::info!(
            event = "security.bootstrap_created",
            token_id = %token_id,
            "token admin do bootstrap criado"
        ),
        BootstrapOutcome::Ignored => tracing::warn!(
            event = "security.bootstrap_ignored",
            "BOOTSTRAP_ADMIN_TOKEN ignorado: o banco ja tem tokens"
        ),
        BootstrapOutcome::MissingWithEmptyDatabase => tracing::warn!(
            event = "security.bootstrap_missing",
            "nenhum token cadastrado e BOOTSTRAP_ADMIN_TOKEN ausente: ninguem consegue administrar"
        ),
        BootstrapOutcome::NotConfigured => {}
    }
    let background = core::background::spawn(core.clone());
    // Os runners da fila processam também o backlog enfileirado antes desta versão.
    let workers = core.spawn_workers();
    // Carrega a tabela BPE do estimador fora do caminho da primeira requisição (decisão 0014).
    let estimator = core::context::estimator::Tiktoken::from_setting(
        &core.settings_snapshot().token_estimator(),
    );
    tokio::task::spawn_blocking(move || estimator.warm());

    let mcp_settings = McpSettings {
        allowed_hosts: settings.mcp_allowed_hosts.clone(),
    };
    let app = build_app(core.clone(), HttpSettings::from(&settings), &mcp_settings);
    let listener = tokio::net::TcpListener::bind(settings.address())
        .await
        .context("falha ao abrir porta HTTP")?;
    tracing::info!(
        event = "application.started",
        version = env!("CARGO_PKG_VERSION"),
        environment = %settings.environment,
        host = %settings.host,
        port = settings.port,
        litestream = settings.database.litestream_enabled,
        "Engramind iniciado"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("servidor HTTP falhou")?;
    background.abort();
    for worker in workers {
        worker.abort();
    }
    if let Err(error) = core.flush_usage().await {
        tracing::warn!(event = "tokens.touch_failed", error = %error, "falha ao gravar last_used_at");
    }
    if let Err(error) = core.flush_counters().await {
        tracing::warn!(event = "memories.counters_failed", error = %error, "falha ao gravar os contadores das memorias");
    }
    tracing::info!(event = "application.stopped", "Engramind encerrado");
    Ok(())
}

async fn migrate(json: bool) -> anyhow::Result<ExitCode> {
    observability::init_logging(LogTarget::Stderr).context("observabilidade invalida")?;
    let settings = DatabaseSettings::from_env().context("configuracao invalida")?;
    let store = SqliteStore::open(&settings.path, settings.read_pool_size)
        .context("falha ao abrir o banco")?;
    let outcome = store.migrate().await.context("falha ao migrar o banco")?;
    if json {
        output::json(&serde_json::json!({
            "applied": outcome.newly_applied(),
            "before": outcome.before,
            "after": outcome.after,
            "expected": outcome.expected,
        }));
    } else {
        output::line(&format!(
            "{} migracao(oes) aplicada(s); schema em {} de {}",
            outcome.newly_applied(),
            outcome.after,
            outcome.expected
        ));
    }
    Ok(ExitCode::SUCCESS)
}

async fn run_doctor(json: bool) -> anyhow::Result<ExitCode> {
    observability::init_logging(LogTarget::Stderr).context("observabilidade invalida")?;
    let settings = DatabaseSettings::from_env().context("configuracao invalida")?;
    let report = doctor::run(&settings).await;
    if json {
        output::json(&report);
    } else {
        output::line(&format!(
            "engramind {} - {}",
            report.version, report.database_path
        ));
        for check in &report.checks {
            let mark = match check.status {
                CheckStatus::Ok => "ok  ",
                CheckStatus::Fail => "FALHA",
                CheckStatus::Skip => "pula",
            };
            output::line(&format!("[{mark}] {}: {}", check.name, check.detail));
        }
    }
    Ok(if report.healthy {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
