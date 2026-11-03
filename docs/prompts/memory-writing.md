Antes de gravar uma memória no Engramind, confira:

- Um fato por memória, que continue verdadeiro por semanas ou meses: uma regra do projeto, uma restrição, um valor de configuração ou uma convenção de nomes.
- Genérico e autossuficiente: quem lê não tem o contexto desta conversa.
- Segredos e dados pessoais só quando forem necessários e o canal for o lugar certo para eles. Códigos de tarefa, URLs, rotas, nomes de comandos e identificadores podem entrar quando forem úteis e duráveis. Se o app que você usa tem uma política de memória própria, ela prevalece sobre esta lista.
- Na categoria certa de `categories.list` (`list_categories`; as padrão são `PROJECT_RULES`, `ARCHITECTURE`, `CONSTRAINTS`, `CONFIG_VALUES` e `NAMING`). Falta uma? Crie com `create_category`.
- Antes de criar, busque (`search_memories`). Depois de criar, olhe `similar`: se já existe uma memória parecida, prefira `update_memory` a ter duas quase iguais.
- A memória vai para outros agentes como instrução. Escreva o fato, não a conversa.
