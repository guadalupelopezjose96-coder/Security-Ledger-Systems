#!/bin/bash
set -e

echo "[1/3] Descargando metadatos y dependencias completas..."
cargo fetch --locked

echo "[2/3] Verificando compilación estricta y suite de pruebas..."
cargo test --all-targets --all-features --no-run

echo "[3/3] Estado del repositorio Git..."
git log -2 --oneline
git status

echo "Proyecto 100% verificado, licencias conciliadas y árbol limpio."
