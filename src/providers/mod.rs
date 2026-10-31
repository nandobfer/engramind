//! Clientes dos provedores externos. Só os workers e as operações de teste explícitas
//! (`test_redator`) chamam daqui; a requisição de um agente nunca espera um provedor.

pub mod llm;
