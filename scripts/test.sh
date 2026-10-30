#!/usr/bin/env sh
# Suíte completa local: compose, formatação, Clippy e testes com cobertura.
# No dia a dia rode só os testes afetados: cargo nextest run -E 'test(/<modulo>::/)'.
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
cd "$ROOT_DIR"

fail() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

command -v cargo >/dev/null 2>&1 || fail 'cargo is required; install Rust 1.98.0 with rustup for the current user'
command -v cc >/dev/null 2>&1 || fail 'cc is required; install the host C build toolchain (build-essential on Ubuntu)'
command -v cargo-nextest >/dev/null 2>&1 || fail 'cargo-nextest is required; run: curl -LsSf https://get.nexte.st/latest/linux | tar zxf - -C ~/.cargo/bin'
command -v cargo-llvm-cov >/dev/null 2>&1 || fail 'cargo-llvm-cov is required; run: cargo install cargo-llvm-cov --locked'

# Os testes nunca leem o .env: cada um monta o próprio ambiente e SQLite em tempdir.
APP_ENV=test
RUST_LOG=warn
LOG_FORMAT=compact
LOG_ANSI=false
export APP_ENV RUST_LOG LOG_FORMAT LOG_ANSI

./scripts/check-compose.sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo llvm-cov nextest --locked --workspace --all-features --all-targets --summary-only
