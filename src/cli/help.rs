//! `engramind help --json`: a árvore completa de comandos, cada um ligado à operação do
//! catálogo que chama (ou marcado como local). É a base da skill e dos testes de contrato.

use std::process::ExitCode;

use clap::{Arg, ArgAction, CommandFactory};
use serde::Serialize;

use super::{Cli, client::LOCAL_ERRORS, output};
use crate::{core::operations::OPERATIONS, error::ERROR_CODES};

/// Comandos que não chamam a API.
pub const LOCAL_COMMANDS: &[&[&str]] = &[
    &["serve"],
    &["migrate"],
    &["doctor"],
    &["help"],
    &["profiles", "add"],
    &["profiles", "list"],
    &["profiles", "remove"],
    &["profiles", "use"],
];

#[derive(Debug, Serialize)]
pub struct ArgDoc {
    pub name: String,
    /// `--nome` das opções; `null` para posicionais.
    pub long: Option<String>,
    pub positional: bool,
    pub required: bool,
    pub multiple: bool,
    pub flag: bool,
    pub env: Option<String>,
    pub default: Option<String>,
    pub help: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CommandDoc {
    /// Caminho sem o nome do binário, por exemplo `tokens issue`.
    pub path: String,
    pub about: Option<String>,
    /// Operação do catálogo chamada pelo comando; `null` nos comandos locais.
    pub operation: Option<&'static str>,
    pub local: bool,
    pub args: Vec<ArgDoc>,
}

#[derive(Debug, Serialize)]
pub struct ExitCodeDoc {
    pub code: &'static str,
    pub exit: u8,
}

#[derive(Debug, Serialize)]
pub struct HelpDoc {
    pub name: &'static str,
    pub version: &'static str,
    pub global_args: Vec<ArgDoc>,
    pub commands: Vec<CommandDoc>,
    pub exit_codes: Vec<ExitCodeDoc>,
}

fn arg_doc(arg: &Arg) -> ArgDoc {
    let flag = matches!(
        arg.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
    );
    ArgDoc {
        name: arg.get_id().to_string(),
        long: arg.get_long().map(|long| format!("--{long}")),
        positional: arg.is_positional(),
        required: arg.is_required_set(),
        multiple: matches!(arg.get_action(), ArgAction::Append)
            || arg
                .get_num_args()
                .is_some_and(|range| range.max_values() > 1),
        flag,
        env: arg.get_env().map(|env| env.to_string_lossy().into_owned()),
        default: arg
            .get_default_values()
            .first()
            .map(|value| value.to_string_lossy().into_owned()),
        help: arg.get_help().map(ToString::to_string),
    }
}

fn is_builtin(arg: &Arg) -> bool {
    matches!(arg.get_id().as_str(), "help" | "version")
}

fn walk(command: &clap::Command, prefix: &mut Vec<String>, output: &mut Vec<CommandDoc>) {
    for sub in command.get_subcommands() {
        prefix.push(sub.get_name().to_owned());
        if sub.has_subcommands() {
            walk(sub, prefix, output);
        } else {
            let path: Vec<&str> = prefix.iter().map(String::as_str).collect();
            let operation = OPERATIONS
                .iter()
                .find(|operation| operation.cli == path.as_slice())
                .map(|operation| operation.name);
            output.push(CommandDoc {
                path: path.join(" "),
                about: sub.get_about().map(ToString::to_string),
                operation,
                local: LOCAL_COMMANDS.contains(&path.as_slice()),
                args: sub
                    .get_arguments()
                    .filter(|arg| !arg.is_global_set() && !is_builtin(arg))
                    .map(arg_doc)
                    .collect(),
            });
        }
        prefix.pop();
    }
}

pub fn document() -> HelpDoc {
    let command = Cli::command();
    let mut commands = Vec::new();
    walk(&command, &mut Vec::new(), &mut commands);
    let mut exit_codes: Vec<ExitCodeDoc> = ERROR_CODES
        .iter()
        .map(|doc| ExitCodeDoc {
            code: doc.code,
            exit: doc.exit_code,
        })
        .collect();
    exit_codes.extend(
        LOCAL_ERRORS
            .iter()
            .map(|(code, exit, _)| ExitCodeDoc { code, exit: *exit }),
    );
    HelpDoc {
        name: "engramind",
        version: env!("CARGO_PKG_VERSION"),
        global_args: command
            .get_arguments()
            .filter(|arg| !is_builtin(arg))
            .map(arg_doc)
            .collect(),
        commands,
        exit_codes,
    }
}

pub fn run(as_json: bool) -> ExitCode {
    if as_json {
        output::json(&document());
    } else {
        output::line(&Cli::command().render_long_help().to_string());
    }
    ExitCode::SUCCESS
}
