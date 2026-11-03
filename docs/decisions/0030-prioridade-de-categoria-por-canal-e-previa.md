# 0030 — Prioridade de categoria por canal, linha de títulos e prévia do texto

- Status: aceita (0.11.0)
- Contexto: desenho §8; decisão 0011.

## Decisão

- `context.category_priorities` (mapa `{ CATEGORIA: -1000..1000 }`, com sobreposição por canal)
  sobrepõe a prioridade de `categories.list` na ordem do contexto: a global e depois a do canal,
  aplicada às memórias daquele canal. As chaves precisam existir em `categories.list`
  (`400 SETTING_INVALID`), e retirar de `categories.list` uma categoria citada dá
  `409 CATEGORY_IN_USE`.
- A faixa de títulos traz a categoria e o canal: `#id [CAT] @canal: título` no `tagged`,
  `- [CAT] @canal: título` no `markdown` e `{ id, title, category, channel }` no `json`. O orçamento
  mede a linha nova.
- `context.text_max_chars` (0 a 4000, padrão 0, por canal) corta a linha da faixa 1 em caracteres
  (fronteira de `char`, com `…`, sem quebras), antes do escape; o orçamento mede o texto cortado. A
  busca e as dicas devolvem o texto inteiro.
- As duas chaves entram em `CONTEXT_RENDER_KEYS`. Como a linha de títulos mudou, o domínio do hash
  da `version` passa de `engramind.context.v1` a `v2`: uma `version` da 0.10 nunca recebe `304`
  sobre o formato novo.

## Consequências

- Todos os consumidores recarregam o bloco uma vez no upgrade.
- "Sem foco, igual a hoje" vale dentro da 0.11.0: com e sem foco, não entre versões.
