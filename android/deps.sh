#!/usr/bin/env bash
# Baixa o sherpa-onnx (o motor do Parakeet no celular), uma vez.
set -euo pipefail
cd "$(dirname "$0")"
v=1.13.8
aar=app/libs/sherpa-onnx-$v.aar
[ -f "$aar" ] && exit 0
mkdir -p app/libs
curl -fsSL -o "$aar.part" "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$v/sherpa-onnx-$v.aar"
mv "$aar.part" "$aar"
