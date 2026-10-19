"""Transcrição com faster-whisper (CTranslate2).

The lifecycle here is CTranslate2-specific: the model releases its VRAM when
the model object is collected, and the cache is what holds the reference. The
Qwen engine has a different lifecycle, which is why each engine owns these
hooks and the dispatcher in stt.py stays engine-agnostic.
"""

from __future__ import annotations

import gc
import os
import re
import sys
import threading
import zlib
import time
from functools import lru_cache

import numpy as np

from . import cuda

CAPABILITIES = frozenset({"translate", "hotwords", "beam_size", "vad"})
DEFAULT_MODEL = "large-v3-turbo"


def _is_degenerate(text: str) -> bool:
    """Detecta a alucinação em laço do Whisper — '))))))', 'Obrigado. Obrigado.
    Obrigado.' — que aparece quando ele recebe silêncio ou ruído.

    Texto humano não comprime tão bem; um laço, sim.
    """
    stripped = text.strip()
    if len(stripped) < 16:
        return False

    # Pouquíssimos caracteres distintos: '))))', '... ... ...'
    if len(set(stripped) - {" "}) <= 3:
        return True

    # Poucas palavras distintas: a mesma frase repetida em laço. Em texto curto
    # o zlib não chega a comprimir o bastante para acusar isso sozinho.
    words = re.findall(r"\w+", stripped.lower())
    if len(words) >= 6 and len(set(words)) / len(words) < 0.4:
        return True

    raw = stripped.encode()
    return len(raw) / len(zlib.compress(raw, 1)) > 3.5


@lru_cache(maxsize=2)
def _load(model: str, device: str, compute_type: str):
    cuda.preload()
    from faster_whisper import WhisperModel

    return WhisperModel(_local(model), device=device, compute_type=compute_type)


def _local(model: str) -> str:
    """O caminho do modelo no cache, sem rede, quando ele já foi baixado.

    Dado só o nome, o WhisperModel pergunta ao Hugging Face se há versão nova a
    cada carga — e o modelo recarrega depois de todo período ocioso. Uma conexão
    que trava sem resposta (já aconteceu) prende a ditada em "transcribing" sem
    prazo. A rede fica só para quando o modelo ainda não está no disco.
    """
    if os.path.isdir(model):
        return model
    from faster_whisper.utils import download_model

    try:
        return download_model(model, local_files_only=True)
    except Exception:
        return model


_carregando = threading.Lock()


def obter(model: str, device: str, compute_type: str):
    """Carrega o modelo, garantindo que só uma thread o faça por vez.

    O lru_cache não é atômico: duas threads em falta simultânea entram as duas
    no construtor, carregam o modelo duas vezes e uma das cópias é jogada fora —
    ~2,4 GB de pico numa placa que só tem espaço para 1,2. Isso passou a ser
    alcançável quando a ditada por voz começou a aquecer o Whisper em paralelo à
    gravação: fala curta e modelo frio põem as duas threads na mesma janela.
    """
    with _carregando:
        return _load(model, device, compute_type)


def is_loaded() -> bool:
    return _load.cache_info().currsize > 0


def unload() -> None:
    """Libera o modelo da VRAM.

    O CTranslate2 devolve a memória quando o objeto do modelo é coletado, e
    quem segura a referência é o cache. Limpar o cache sem coletar não libera
    nada, então o gc.collect() aqui não é supersticioso.
    """
    if not is_loaded():
        return
    _load.cache_clear()
    gc.collect()


def run(
    audio: np.ndarray,
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

    # Rede de segurança: o Whisper normaliza o espectrograma por seu máximo, e
    # um único NaN na entrada transforma TODA a matriz de features em NaN — a
    # transcrição vira um fragmento sem sentido. Vale mais perder uma amostra
    # do que a frase inteira.
    if not np.isfinite(audio).all():
        bad = int((~np.isfinite(audio)).sum())
        print(f"[stt] {bad} amostra(s) inválida(s) no áudio; zeradas.", file=sys.stderr)
        audio = np.nan_to_num(audio, nan=0.0, posinf=0.0, neginf=0.0)

    try:
        whisper = obter(model, device, compute_type)
    except Exception as exc:  # GPU indisponível / libs CUDA faltando
        if device != "cpu":
            print(f"[stt] GPU indisponível ({exc}); caindo para CPU.", file=sys.stderr)
            whisper = obter(model, "cpu", "int8")
        else:
            raise

    def keep(segment) -> bool:
        # O VAD deixa passar ruído baixo, e aí o Whisper "ouve" fala onde não
        # há. Essas duas medidas são as que ele próprio usa para se autoavaliar.
        return segment.no_speech_prob < 0.7 and segment.compression_ratio < 2.4

    def attempt(engine):
        segments, info = engine.transcribe(
            audio,
            language=language,
            # Whisper translates only INTO English; the task is what asks for it.
            task="translate" if translate else "transcribe",
            beam_size=beam_size,
            initial_prompt=initial_prompt,
            vad_filter=vad,
            vad_parameters={"min_silence_duration_ms": 500},
            # ditado são falas curtas e independentes: arrastar contexto entre
            # segmentos é a principal fonte de alucinação em loop do Whisper.
            condition_on_previous_text=False,
        )
        # A geração é preguiçosa: só aqui a GPU é realmente exercitada, então é
        # aqui que um out-of-memory aparece — não no carregamento do modelo.
        text = " ".join(s.text.strip() for s in segments if keep(s)).strip()
        return ("" if _is_degenerate(text) else text), info

    try:
        text, info = attempt(whisper)
    except RuntimeError as exc:
        if "out of memory" not in str(exc).lower() or device == "cpu":
            raise
        # Numa placa pequena isso acontece quando um jogo ou outro modelo já
        # tomou a VRAM. Transcrever devagar é muito melhor que perder a fala.
        print(
            "[stt] sem VRAM disponível; refazendo na CPU (mais lento).",
            file=sys.stderr,
        )
        text, info = attempt(obter(model, "cpu", "int8"))

    return text, info.language, info.language_probability


def download(model: str) -> str:
    """Só busca os pesos para o cache local. Não toca na GPU.

    Separado do warmup de propósito: baixar é preparar a máquina, carregar é
    usar a máquina. Quem está instalando não quer ocupar VRAM, e quem tem VRAM
    ocupada ainda assim precisa conseguir baixar.
    """
    from faster_whisper.utils import download_model

    return download_model(model)


def warmup(model: str, device: str, compute_type: str) -> bool:
    """Baixa e carrega o modelo. Devolve se ele ficou residente.

    É melhor esforço de propósito: o objetivo do warmup é tirar o download do
    caminho, e o download já terminou quando a VRAM entra em cena. Se não há
    memória agora — um daemon já rodando, um jogo aberto — isso não é motivo
    para falhar; o modelo entra na primeira ditada.
    """
    try:
        whisper = obter(model, device, compute_type)
        list(whisper.transcribe(np.zeros(16_000, dtype=np.float32), language=None)[0])
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
