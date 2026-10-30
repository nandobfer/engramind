//! Autorização por tipo e escopo, aplicada no `core` antes de qualquer acesso a dados.

use serde_json::json;

use super::{
    operations::{OPERATIONS, OperationSpec, RequiredKind},
    principal::Principal,
};
use crate::error::ApiError;

/// Se o token pode executar a operação (sem considerar canais).
pub fn allows(principal: &Principal, operation: &OperationSpec) -> bool {
    let kind_ok = match operation.kind {
        RequiredKind::Any => true,
        RequiredKind::Admin => principal.is_admin(),
    };
    kind_ok
        && operation
            .scope
            .is_none_or(|scope| principal.scopes.contains(scope))
}

/// `403 SCOPE_REQUIRED` com `details: { required_kind, required_scope }` quando o token não basta.
pub fn require(principal: &Principal, operation: &OperationSpec) -> Result<(), ApiError> {
    if allows(principal, operation) {
        return Ok(());
    }
    let message = match (operation.kind, operation.scope) {
        (RequiredKind::Admin, Some(scope)) => format!(
            "A operacao {} exige um token admin com o escopo {}",
            operation.name,
            scope.as_str()
        ),
        (RequiredKind::Admin, None) => {
            format!("A operacao {} exige um token admin", operation.name)
        }
        (RequiredKind::Any, Some(scope)) => format!(
            "A operacao {} exige o escopo {}",
            operation.name,
            scope.as_str()
        ),
        (RequiredKind::Any, None) => format!("A operacao {} nao e permitida", operation.name),
    };
    Err(
        ApiError::new("SCOPE_REQUIRED", message).with_details(json!({
            "required_kind": operation.kind.as_str(),
            "required_scope": operation.scope.map(|scope| scope.as_str()),
        })),
    )
}

/// Nomes das operações permitidas ao token, na ordem do catálogo.
pub fn allowed_operations(principal: &Principal) -> Vec<&'static str> {
    OPERATIONS
        .iter()
        .filter(|operation| allows(principal, operation))
        .map(|operation| operation.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{
        operations::spec,
        principal::{AppRef, ScopeSet, TokenKind},
    };

    fn principal(kind: TokenKind, scopes: &str) -> Principal {
        Principal {
            token_id: "tk_00000001".to_owned(),
            app: AppRef {
                id: "app_00000001".to_owned(),
                slug: "app-a".to_owned(),
            },
            agent: None,
            kind,
            scopes: ScopeSet::parse(scopes).expect("scopes"),
        }
    }

    #[test]
    fn agent_tokens_never_reach_admin_operations() {
        let agent = principal(TokenKind::Agent, "read,write,delete");
        for operation in OPERATIONS {
            assert_eq!(
                allows(&agent, operation),
                operation.kind == RequiredKind::Any,
                "{}",
                operation.name
            );
        }
        let error = require(&agent, spec("list_apps")).expect_err("denied");
        assert_eq!(error.code, "SCOPE_REQUIRED");
        assert_eq!(
            error.details,
            Some(json!({ "required_kind": "admin", "required_scope": "read" }))
        );
    }

    #[test]
    fn admin_tokens_are_limited_by_their_scopes() {
        let read_only = principal(TokenKind::Admin, "read");
        assert!(allows(&read_only, spec("list_tokens")));
        assert!(!allows(&read_only, spec("issue_token")));
        assert!(!allows(&read_only, spec("revoke_token")));
        let read_delete = principal(TokenKind::Admin, "read,delete");
        assert!(allows(&read_delete, spec("revoke_token")));
        assert!(!allows(&read_delete, spec("update_token")));
        assert!(allowed_operations(&read_only).contains(&"whoami"));
    }
}
