//! Autenticação Bearer: formato e HMAC dos tokens e o middleware das rotas protegidas.

pub mod middleware;
pub mod token;

pub use middleware::{Auth, require_auth};
