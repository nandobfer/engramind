//! Único ponto de escrita no terminal da CLI. O servidor nunca escreve aqui: ele usa `tracing`.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use serde::Serialize;

pub fn json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(error) => eprintln!("erro: falha ao serializar a saida: {error}"),
    }
}

pub fn line(text: &str) {
    println!("{text}");
}

pub fn failure(error: &anyhow::Error) {
    eprintln!("erro: {error:#}");
}
