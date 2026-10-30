//! Registro do `sqlite-vec` como extensão automática do SQLite.
//!
//! Este é o único módulo do crate com `unsafe` (`docs/decisions/0004-unsafe-sqlite-vec.md`).
#![allow(unsafe_code)]

use std::{
    ffi::{c_char, c_int},
    sync::OnceLock,
};

use rusqlite::ffi::{SQLITE_OK, sqlite3, sqlite3_api_routines, sqlite3_auto_extension};

type ExtensionEntryPoint =
    unsafe extern "C" fn(*mut sqlite3, *mut *mut c_char, *const sqlite3_api_routines) -> c_int;

static REGISTERED: OnceLock<c_int> = OnceLock::new();

/// Registra o `sqlite-vec` uma vez por processo; toda conexão aberta depois já o carrega.
pub(super) fn register() -> bool {
    let code = *REGISTERED.get_or_init(|| {
        // SAFETY: `sqlite3_vec_init` é o ponto de entrada de extensão compilado pela crate
        // `sqlite-vec`, com a assinatura `sqlite3_extension_init` do SQLite; a crate o
        // declara sem parâmetros, e o transmute só restaura a assinatura real, como no
        // exemplo oficial da crate. `sqlite3_auto_extension` é seguro entre threads, e o
        // `OnceLock` garante um único registro.
        unsafe {
            let entry_point = std::mem::transmute::<*const (), ExtensionEntryPoint>(
                sqlite_vec::sqlite3_vec_init as *const (),
            );
            sqlite3_auto_extension(Some(entry_point))
        }
    });
    code == SQLITE_OK
}
