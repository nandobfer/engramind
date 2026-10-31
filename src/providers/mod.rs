//! Clientes dos provedores externos. Só os workers e as operações de teste explícitas
//! (`test_redator`, `test_embedding`, `list_embedding_models`, a validação de uma troca de
//! embedding) chamam daqui, além do vetor da consulta da busca, com prazo curto; a escrita de um
//! agente nunca espera um provedor.

pub mod embeddings;
pub mod llm;
