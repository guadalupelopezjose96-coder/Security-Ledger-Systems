#!/bin/bash
set -e

echo "[1/3] Ejecutando Clippy con prohibición de unwrap, expect y panic..."
cargo clippy --all-targets --all-features -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic

echo "[2/3] Compilando suite de tests en modo verificación offline..."
cargo test --all-targets --all-features --no-run

echo "[3/3] Comprobación de licencias y advisories (si cargo-deny está disponible)..."
if command -v cargo-deny >/dev/null 2>&1; then
    cargo deny check licenses advisories bans
else
    echo "cargo-deny no encontrado en entorno local (se validará en GitLab CI)."
fi

echo "Verificación técnica local superada al 100%."
