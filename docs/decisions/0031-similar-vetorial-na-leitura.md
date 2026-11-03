# 0031 — Similar vetorial na leitura

- Status: aceita (0.11.0); revisa em parte a 0021.
- Contexto: desenho §9; decisão 0021.

## Contexto

A 0021 manteve o `similar` da escrita textual porque a escrita nunca espera o embedding. Na leitura,
o vetor da memória já existe, e o Jaccard não acha paráfrases.

## Decisão

- A escrita continua como na 0021 (`method: text`).
- `get_memory` devolve `similar` calculado na hora: com vetor do modelo vigente, até três memórias
  `active`/`permanent` do mesmo canal com cosseno ≥ `memory.vector_similar_threshold` (`method:
  vector`, `duplicate` a partir de `memory.vector_duplicate_threshold`); sem vetor, o textual.
- As chaves seguem o prefixo `memory.*` do projeto (a especificação da tarefa dizia `memories.*`).
- Cada dica de `get_hints` traz `score`, `method` e `duplicate`. Sem embeddings, as dicas caem para o
  textual (candidatos do FTS com Jaccard ≥ `memory.similar_threshold`) em vez de vir vazias.

## Consequências

- `duplicate` é só aviso: nada é recusado nem fundido.
- `get_hints` sem embeddings deixa de responder lista vazia (mudança de comportamento no CHANGELOG).
