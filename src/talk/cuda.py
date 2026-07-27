"""Torna as libs CUDA dos wheels nvidia-* visíveis para o CTranslate2.

O CTranslate2 (motor do faster-whisper) faz dlopen de libcudnn/libcublas pelo
nome, sem procurar dentro do site-packages. Sem isso, uma instalação por pip
só acha as libs se o usuário exportar LD_LIBRARY_PATH na mão. Carregar as
libs com RTLD_GLOBAL antes do import resolve os símbolos no processo.
"""

from __future__ import annotations

import ctypes
import glob
import os
import sys

_PRELOAD = (
    "nvidia/cublas/lib/libcublas.so.*",
    "nvidia/cublas/lib/libcublasLt.so.*",
    "nvidia/cudnn/lib/libcudnn.so.*",
    "nvidia/cudnn/lib/libcudnn_ops.so.*",
    "nvidia/cudnn/lib/libcudnn_cnn.so.*",
    "nvidia/cudnn/lib/libcudnn_engines_precompiled.so.*",
    "nvidia/cudnn/lib/libcudnn_engines_runtime_compiled.so.*",
    "nvidia/cudnn/lib/libcudnn_heuristic.so.*",
    "nvidia/cudnn/lib/libcudnn_graph.so.*",
)

_done = False


def preload() -> None:
    """Carrega as libs CUDA empacotadas. Idempotente e silencioso se faltarem."""
    global _done
    if _done:
        return
    _done = True

    roots = [p for p in sys.path if p.endswith("site-packages")]
    for pattern in _PRELOAD:
        for root in roots:
            matches = sorted(glob.glob(os.path.join(root, pattern)))
            if not matches:
                continue
            try:
                ctypes.CDLL(matches[-1], mode=ctypes.RTLD_GLOBAL)
            except OSError:
                pass
            break
