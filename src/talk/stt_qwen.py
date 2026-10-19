"""Transcrição com Qwen3-ASR (torch).

The lifecycle here is the opposite of the CTranslate2 one in stt_whisper.py:
dropping the model object is not enough, because the torch allocator keeps the
blocks reserved — nvidia-smi still shows them — until empty_cache() says
otherwise. That is why this engine, and not the dispatcher, owns the hooks.
"""

from __future__ import annotations

import gc
import os
import sys
import threading
import time

import numpy as np

from . import audio, cuda
from .stt import SttError

# transformers warms the allocator with one segment the size of the model.
# A few MB that outlive unload() stay inside it, and a segment with anything
# alive cannot be returned: empty_cache() kept 3.3 GB of a 6 GB card reserved.
# Expandable segments release the free pages instead. Read when CUDA first
# allocates, so it must be set before torch touches the card.
os.environ.setdefault("PYTORCH_CUDA_ALLOC_CONF", "expandable_segments:True")

CAPABILITIES = frozenset({"auto_language", "time_stamps"})
DEFAULT_MODEL = "Qwen/Qwen3-ASR-1.7B"

# One `language` knob in the config feeds two vocabularies: Whisper speaks ISO
# codes ("pt"), Qwen3-ASR speaks canonical names ("Portuguese") and raises
# ValueError deep inside its own utils for anything else. The codes below are
# the ISO-639-1 form of every name the package accepts, so the config keeps
# working after the swap and an unmapped value raises below, naming what is
# accepted, instead of dying in a forward pass.
LANGUAGE_NAMES = {
    "zh": "Chinese",
    "en": "English",
    "yue": "Cantonese",
    "ar": "Arabic",
    "de": "German",
    "fr": "French",
    "es": "Spanish",
    "pt": "Portuguese",
    "id": "Indonesian",
    "it": "Italian",
    "ko": "Korean",
    "ru": "Russian",
    "th": "Thai",
    "vi": "Vietnamese",
    "ja": "Japanese",
    "tr": "Turkish",
    "hi": "Hindi",
    "ms": "Malay",
    "nl": "Dutch",
    "sv": "Swedish",
    "da": "Danish",
    "fi": "Finnish",
    "pl": "Polish",
    "cs": "Czech",
    "tl": "Filipino",
    "fa": "Persian",
    "el": "Greek",
    "ro": "Romanian",
    "hu": "Hungarian",
    "mk": "Macedonian",
}
_CANONICO = {name.lower(): name for name in LANGUAGE_NAMES.values()}


def language_name(value: str | None) -> str | None:
    """The config's `language` in the vocabulary this engine speaks.

    None, empty or "auto" is the documented "let the model name what it heard";
    a canonical name is accepted whatever its casing (the package lower-cases
    it itself) and a code goes through LANGUAGE_NAMES. Anything else raises
    here, before obter() would buy 4.4 GB of weights to die in a forward pass.
    """
    if value is None:
        return None
    text = str(value).strip()
    if text == "" or text.lower() == "auto":
        return None
    if (name := _CANONICO.get(text.lower())):
        return name
    if (name := LANGUAGE_NAMES.get(text.lower())):
        return name
    raise SttError(
        f'qwen does not know language {text!r}; use a name like "Portuguese" '
        f"or a code like \"pt\" (accepted: {', '.join(sorted(set(LANGUAGE_NAMES)))})"
    )


_handle = None
_key = None
_carregando = threading.Lock()


def _packages():
    """Lazy importer: the app must stay importable without the qwen extra.

    Same shape as the voz engines: importing this module costs nothing, and
    only asking the engine for weights says which extra is missing.
    """
    try:
        from qwen_asr import Qwen3ASRModel
    except ImportError as exc:
        raise SttError(
            "Qwen ASR não instalado — rode `uv sync --extra qwen` "
            "no diretório do projeto"
        ) from exc
    return Qwen3ASRModel


def _dtype(compute_type: str):
    """compute_type is a CTranslate2 vocabulary; torch only speaks dtypes.

    There is no int8 path wired for this engine yet, so anything quantized
    comes back as bfloat16: a wrong dtype is worse than a slower one.
    """
    import torch

    if compute_type == "float32":
        return torch.float32
    return torch.bfloat16


def _load(model: str, device: str, compute_type: str):
    cuda.preload()
    Qwen3ASRModel = _packages()

    return Qwen3ASRModel.from_pretrained(
        model,
        dtype=_dtype(compute_type),
        device_map=None if device == "cpu" else device,
        max_inference_batch_size=1,
        # A ditada são falas curtas; 256 tokens já é o ceiling de uma frase
        # Comfortável e o costuma caparra a runaway generation.
        max_new_tokens=256,
    )


def obter(model: str, device: str, compute_type: str):
    """Carrega o modelo, garantindo que só uma thread o faça por vez.

    Same reason as in stt_whisper.py: the check-then-load below is not atomic,
    and the wake word warms the engine in parallel to the recording, so two
    threads can be in this window at once — two copies of a 4 GB model on a
    card that has room for one.
    """
    global _handle, _key
    with _carregando:
        if _handle is not None and _key == (model, device, compute_type):
            return _handle
        if _handle is not None:
            unload()
        _handle = _load(model, device, compute_type)
        _key = (model, device, compute_type)
        return _handle


def is_loaded() -> bool:
    return _handle is not None


def unload() -> None:
    """Libera o modelo da VRAM.

    With torch, dropping the reference only makes the blocks freeable: the
    allocator keeps them reserved, so nvidia-smi still reports them used and
    the next engine still fails to fit. empty_cache() is what returns them.
    """
    global _handle, _key
    if _handle is None:
        return
    _handle = None
    _key = None
    gc.collect()
    try:
        import torch

        torch.cuda.empty_cache()
    except (ImportError, RuntimeError):
        pass


def run(
    samples: np.ndarray,
    *,
    model: str,
    device: str,
    compute_type: str,
    language: str | None,
    translate: bool,
    initial_prompt: str | None,
    beam_size: int,
    vad: bool,
) -> tuple[str, str, float]:
    started = time.monotonic()

    # Resolved first, on purpose: a language the engine cannot honour must not
    # cost a model load to say so.
    spoken = language_name(language)

    # Same guard as the Whisper engine: one NaN does not cost one frame of the
    # transcription, it corrupts whatever the model derives from the samples.
    if not np.isfinite(samples).all():
        bad = int((~np.isfinite(samples)).sum())
        print(f"[stt] {bad} amostra(s) inválida(s) no áudio; zeradas.", file=sys.stderr)
        samples = np.nan_to_num(samples, nan=0.0, posinf=0.0, neginf=0.0)

    def attempt(where: str, how: str) -> tuple[str, str, float]:
        asr = obter(model, where, how)
        # The engine takes (samples, samplerate): it resamples for itself, so
        # the dispatcher never has to know the model's expected rate.
        out = asr.transcribe(
            audio=(samples, audio.SAMPLE_RATE),
            # None = let the model name the language it heard.
            language=spoken,
        )[0]
        heard = getattr(out, "language", None) or (spoken or "")
        # The model reports no confidence; 1.0 only when the caller pinned the
        # language, otherwise unknown — and unknown is not "certain".
        return (out.text or "").strip(), heard, 1.0 if spoken else 0.0

    try:
        return attempt(device, compute_type)
    except RuntimeError as exc:
        if "out of memory" not in str(exc).lower() or device == "cpu":
            raise
        # Same policy as the Whisper engine: losing the VRAM race is worth an
        # hour of waiting, losing the speech is worth nothing.
        print(
            "[stt] sem VRAM disponível; refazendo na CPU (mais lento).",
            file=sys.stderr,
        )
        unload()
        return attempt("cpu", "float32")


def download(model: str) -> str:
    """Só busca os pesos para o cache local. Não toca na GPU.

    Same split as in stt_whisper.py: installing must work on a machine whose
    VRAM is busy, so this path never touches the allocator.
    """
    from huggingface_hub import snapshot_download

    return snapshot_download(model)


def warmup(model: str, device: str, compute_type: str) -> bool:
    """Baixa e carrega o modelo. Devolve se ele ficou residente."""
    try:
        asr = obter(model, device, compute_type)
        asr.transcribe(
            audio=(np.zeros(16_000, dtype=np.float32), audio.SAMPLE_RATE),
            language=None,
        )
        return True
    except RuntimeError as exc:
        if "out of memory" not in str(exc).lower():
            raise
        unload()
        print(
            "[stt] modelo baixado, mas sem VRAM livre para carregá-lo agora; "
            "ele entrará na primeira ditada.",
            file=sys.stderr,
        )
        return False
