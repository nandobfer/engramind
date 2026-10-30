//! Quem chama: o token autenticado, com tipo, escopos, app e agente (desenho §4).

use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Tipo do token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// Opera memórias nos canais assinados pelo agente.
    Agent,
    /// Gere a rede pelas ferramentas administrativas.
    Admin,
}

impl TokenKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(Self::Agent),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    ToSchema,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Read,
    Write,
    Delete,
}

impl Scope {
    pub const ALL: [Self; 3] = [Self::Read, Self::Write, Self::Delete];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Delete => "delete",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "write" => Some(Self::Write),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }

    fn bit(self) -> u8 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::Delete => 4,
        }
    }
}

/// Conjunto válido de escopos. Invariante: `write` e `delete` exigem `read`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeSet(u8);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeSetError {
    Empty,
    Unknown(String),
    MissingRead,
    Duplicate(Scope),
}

impl fmt::Display for ScopeSetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("informe ao menos o escopo read"),
            Self::Unknown(value) => write!(formatter, "escopo desconhecido: {value}"),
            Self::MissingRead => formatter.write_str("write e delete exigem read"),
            Self::Duplicate(scope) => write!(formatter, "escopo repetido: {}", scope.as_str()),
        }
    }
}

impl ScopeSet {
    pub const READ: Self = Self(1);
    pub const ALL: Self = Self(7);

    pub fn from_scopes(scopes: &[Scope]) -> Result<Self, ScopeSetError> {
        if scopes.is_empty() {
            return Err(ScopeSetError::Empty);
        }
        let mut bits = 0;
        for scope in scopes {
            if bits & scope.bit() != 0 {
                return Err(ScopeSetError::Duplicate(*scope));
            }
            bits |= scope.bit();
        }
        if bits & Scope::Read.bit() == 0 {
            return Err(ScopeSetError::MissingRead);
        }
        Ok(Self(bits))
    }

    /// Lê a forma persistida/CLI: `read,write`.
    pub fn parse(value: &str) -> Result<Self, ScopeSetError> {
        let scopes = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(|part| Scope::parse(part).ok_or_else(|| ScopeSetError::Unknown(part.to_owned())))
            .collect::<Result<Vec<_>, _>>()?;
        Self::from_scopes(&scopes)
    }

    pub fn contains(self, scope: Scope) -> bool {
        self.0 & scope.bit() != 0
    }

    pub fn is_subset_of(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    /// Escopos presentes nos dois conjuntos. Ambos contêm `read`, então o resultado é válido.
    pub fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    pub fn scopes(self) -> Vec<Scope> {
        Scope::ALL
            .into_iter()
            .filter(|scope| self.contains(*scope))
            .collect()
    }

    /// Forma persistida: `read,write,delete`.
    pub fn to_csv(self) -> String {
        self.scopes()
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl fmt::Debug for ScopeSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "ScopeSet({})", self.to_csv())
    }
}

/// Referência pública a um app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct AppRef {
    #[schema(example = "app_3f9e1a7c")]
    pub id: String,
    #[schema(example = "app-a")]
    pub slug: String,
}

/// Referência pública a um agente; `slug` é `app/agente`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct AgentRef {
    #[schema(example = "ag_5b1c9d2e")]
    pub id: String,
    #[schema(example = "app-a/curador")]
    pub slug: String,
}

/// Token autenticado de uma requisição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub token_id: String,
    pub app: AppRef,
    pub agent: Option<AgentRef>,
    pub kind: TokenKind,
    pub scopes: ScopeSet,
}

impl Principal {
    pub fn is_admin(&self) -> bool {
        self.kind == TokenKind::Admin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_four_documented_combinations_are_valid() {
        let valid = ["read", "read,write", "read,delete", "read,write,delete"];
        for value in valid {
            assert_eq!(ScopeSet::parse(value).expect(value).to_csv(), value);
        }
        assert_eq!(
            ScopeSet::parse("write,read").expect("order").to_csv(),
            "read,write"
        );
        assert_eq!(ScopeSet::parse(""), Err(ScopeSetError::Empty));
        assert_eq!(ScopeSet::parse("write"), Err(ScopeSetError::MissingRead));
        assert_eq!(
            ScopeSet::parse("delete,write"),
            Err(ScopeSetError::MissingRead)
        );
        assert_eq!(
            ScopeSet::parse("read,admin"),
            Err(ScopeSetError::Unknown("admin".to_owned()))
        );
        assert_eq!(
            ScopeSet::parse("read,read"),
            Err(ScopeSetError::Duplicate(Scope::Read))
        );
    }

    #[test]
    fn subset_detects_scope_escalation() {
        let read = ScopeSet::READ;
        let read_write = ScopeSet::parse("read,write").expect("rw");
        assert!(read.is_subset_of(read_write));
        assert!(!read_write.is_subset_of(read));
        assert!(read_write.is_subset_of(ScopeSet::ALL));
        assert!(read_write.contains(Scope::Write));
        assert!(!read_write.contains(Scope::Delete));
        let read_delete = ScopeSet::parse("read,delete").expect("rd");
        assert_eq!(read_write.intersection(read_delete), ScopeSet::READ);
        assert_eq!(ScopeSet::ALL.intersection(read_write), read_write);
    }
}
