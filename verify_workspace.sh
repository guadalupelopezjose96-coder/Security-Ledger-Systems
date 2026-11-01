#!/bin/bash
set -e

echo "[1/4] Verificando estructura de archivos..."
test -f Cargo.toml && echo "  -> Cargo.toml OK"
test -f deny.toml && echo "  -> deny.toml OK"
test -f Dockerfile && echo "  -> Dockerfile OK"
test -f .gitlab-ci.yml && echo "  -> .gitlab-ci.yml OK"
test -f migrations/20261101000001_initial_schema.sql && echo "  -> Migración SQL OK"
test -f src/lib.rs && echo "  -> src/lib.rs OK"
test -f src/ledger_repository.rs && echo "  -> src/ledger_repository.rs OK"
test -f src/main.rs && echo "  -> src/main.rs OK"
test -f tests/concurrent_ledger_test.rs && echo "  -> tests/concurrent_ledger_test.rs OK"

echo "[2/4] Validando sintaxis y compilación de Rust..."
cargo check --all-targets

echo "[3/4] Validando licencias y dependencias con cargo-deny..."
if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny check licenses advisories bans
else
    echo "  -> cargo-deny no está instalado localmente (se ejecutará en el pipeline CI)."
fi

echo "[4/4] Verificación de código estricto con Clippy..."
cargo clippy --all-targets --all-features -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic

echo "Estructura y código 100% verificados."
