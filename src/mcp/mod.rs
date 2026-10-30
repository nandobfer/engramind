//! MCP em `POST /mcp` (Streamable HTTP, stateless; portado do sap-middleman): as ferramentas
//! de operação e de documentação do `core`, filtradas pelo token (D-004).

pub mod contract;
pub mod tools;

use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::never::NeverSessionManager,
};

pub use crate::config::AllowedHosts;
use crate::{core::Core, store::AppStore};

use self::tools::EngramindServer;

#[derive(Clone, Debug, Default)]
pub struct McpSettings {
    pub allowed_hosts: AllowedHosts,
}

pub type McpService = StreamableHttpService<EngramindServer, NeverSessionManager>;

pub fn service(core: Core<AppStore>, settings: &McpSettings, max_body_bytes: usize) -> McpService {
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None)
        .with_sse_retry(None)
        .with_max_request_body_bytes(max_body_bytes);
    let config = match &settings.allowed_hosts {
        AllowedHosts::Any => config.disable_allowed_hosts(),
        AllowedHosts::List(hosts) => config.with_allowed_hosts(hosts.clone()),
    };
    let server = EngramindServer::new(core);
    StreamableHttpService::new(
        move || Ok(server.clone()),
        std::sync::Arc::new(NeverSessionManager::default()),
        config,
    )
}
