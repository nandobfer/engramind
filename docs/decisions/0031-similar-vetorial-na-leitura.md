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
- Cada dica de `get_hints` traz `score`, `method` e `duplicate`. Sem embeddings, a lista continua
  vazia, como antes da 0.11.0: o consumidor que chama `get_hints` a cada turno nunca passa a receber
  dicas textuais sem pedir (item 19 da tarefa: nenhuma mudança sem parâmetro novo).

## Consequências

- `duplicate` é só aviso: nada é recusado nem fundido.
- O `similar` textual existe só na escrita e em `get_memory` sem vetor; nas dicas, `method` é sempre
  `vector` hoje e o campo fica para uma variante futura.
