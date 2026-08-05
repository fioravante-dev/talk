"""Ouvinte de palavra-chave: "hey jarvis" abre uma ditada sem tocar no teclado.

Não substitui o atalho — soma a ele. Às vezes a mão já está no teclado e a tecla
é mais rápida; às vezes ela está ocupada, e a voz é a única entrada que sobra.

Roda o openWakeWord em ONNX na CPU: melspectrograma → embedding de 96 dimensões
→ um classificador minúsculo sobre as 16 últimas janelas. Medido nesta máquina,
2,4 ms para cada quadro de 80 ms — ~3 % de um núcleo, contra os ~1,2 GB de VRAM
que o Whisper ocupa quando acorda.

Os modelos NÃO vivem no repositório. Os pré-treinados são CC-BY-NC-SA 4.0 e este
projeto é MIT; misturar as duas licenças no mesmo tarball seria errado. Eles são
baixados sob demanda para ~/.cache/talk/wake, junto das outras coisas grandes que
o talk busca da rede.
"""

from __future__ import annotations

import os
import time
from pathlib import Path

import numpy as np

# Nomes de arquivo, não nomes amigáveis: o openWakeWord batiza cada modelo pelo
# basename sem extensão, e é ESSA chave que volta no dicionário de scores
# ("hey_jarvis_v0.1", não "hey_jarvis").
MODELOS = {
    "hey_jarvis": "hey_jarvis_v0.1.onnx",
    "alexa": "alexa_v0.1.onnx",
    "hey_mycroft": "hey_mycroft_v0.1.onnx",
    "hey_rhasspy": "hey_rhasspy_v0.1.onnx",
}

_MELSPEC = "melspectrogram.onnx"
_EMBEDDING = "embedding_model.onnx"


class WakeError(RuntimeError):
    pass


def modelos_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache")
    return Path(base) / "talk" / "wake"


def caminho(modelo: str) -> Path:
    """Resolve um apelido conhecido ou o caminho de um .onnx treinado por você.

    Aceitar caminho é o que deixa um modelo em português entrar sem código novo:
    treine, jogue o arquivo aqui e aponte `wake_model` para ele.
    """
    if modelo.endswith(".onnx"):
        return Path(modelo).expanduser()
    if modelo not in MODELOS:
        raise WakeError(
            f"palavra-chave desconhecida: {modelo!r} — conhecidas: "
            f"{', '.join(MODELOS)}; ou passe o caminho de um .onnx"
        )
    return modelos_dir() / MODELOS[modelo]


def _importar():
    """Importa o openWakeWord só quando ele é realmente necessário.

    A palavra-chave é um extra opcional (`uv sync --extra wake`), porque arrasta
    scipy e scikit-learn. Um import no topo do módulo faria o `talk` inteiro
    deixar de subir para quem só usa o atalho.
    """
    try:
        from openwakeword.model import Model
    except ImportError as exc:
        raise WakeError(
            "openWakeWord não instalado — rode `uv sync --extra wake` "
            "no diretório do projeto"
        ) from exc
    return Model


def baixar(modelo: str = "hey_jarvis") -> Path:
    """Busca os ONNX: o classificador da palavra e os dois de features."""
    _importar()  # a mensagem boa vem daqui, antes do import interno feio
    import openwakeword.utils

    destino = modelos_dir()
    destino.mkdir(parents=True, exist_ok=True)
    if modelo.endswith(".onnx"):
        # Modelo próprio: só as features são baixáveis; o classificador é seu.
        if not caminho(modelo).is_file():
            raise WakeError(f"modelo não encontrado: {caminho(modelo)}")
        openwakeword.utils.download_models(model_names=[], target_directory=str(destino))
    else:
        openwakeword.utils.download_models(
            model_names=[modelo], target_directory=str(destino)
        )
    return caminho(modelo)


def faltando(modelo: str = "hey_jarvis") -> list[Path]:
    """Quais dos três arquivos necessários ainda não estão no disco."""
    d = modelos_dir()
    return [p for p in (caminho(modelo), d / _MELSPEC, d / _EMBEDDING) if not p.is_file()]


class Detector:
    """Um modelo carregado, alimentado bloco a bloco.

    O limiar, a paciência e o descanso são nossos, não do openWakeWord: os dele
    zeram o score quando não disparam, e o `talk --wake-test` precisa do número
    cru para você conseguir calibrar olhando.
    """

    def __init__(
        self,
        *,
        modelo: str = "hey_jarvis",
        threshold: float = 0.5,
        patience: int = 2,
        cooldown: float = 3.0,
    ):
        Model = _importar()
        pendentes = faltando(modelo)
        if pendentes:
            raise WakeError(
                f"modelo da palavra-chave não baixado ({pendentes[0].name}) — "
                "rode `talk --download`"
            )
        d = modelos_dir()
        self.model = Model(
            wakeword_models=[str(caminho(modelo))],
            inference_framework="onnx",
            # Sem isto o openWakeWord procura os modelos de features dentro do
            # site-packages, e o wheel dele (60 KB) não traz nenhum.
            melspec_model_path=str(d / _MELSPEC),
            embedding_model_path=str(d / _EMBEDDING),
            # vad_threshold fica em 0 de propósito: ligá-lo faz o openWakeWord
            # instanciar um VAD apontando para um silero_vad.onnx que não existe
            # no site-packages. A guarda de falso positivo daqui é outra e é
            # melhor — `wake_grace_secs` escuta se veio fala DEPOIS do disparo.
        )
        self.nome = caminho(modelo).stem
        self.threshold = threshold
        self.patience = max(1, int(patience))
        self.cooldown = cooldown
        self.ultimo = 0.0
        self._acima = 0
        self._mudo_ate = 0.0

    def disparou(self, bloco: np.ndarray) -> bool:
        """Alimenta um bloco float32 e diz se a palavra-chave acabou de sair."""
        # O openWakeWord só aceita PCM de 16 bits; a captura do talk é float32
        # em [-1, 1]. O nan_to_num não é paranoia: um NaN escapado do cabeçalho
        # AU vira lixo indefinido no astype e contamina a janela de features.
        pcm = (np.clip(np.nan_to_num(bloco), -1.0, 1.0) * 32767).astype(np.int16)
        self.ultimo = float(max(self.model.predict(pcm).values(), default=0.0))

        agora = time.monotonic()
        if agora < self._mudo_ate:
            self._acima = 0
            return False
        self._acima = self._acima + 1 if self.ultimo >= self.threshold else 0
        if self._acima < self.patience:
            return False
        self._acima = 0
        self._mudo_ate = agora + self.cooldown
        return True

    def reset(self) -> None:
        """Zera os buffers depois de uma ditada.

        Sem isto o áudio da própria ditada continua na janela de features, e a
        palavra-chave que a abriu dispara de novo assim que o ouvinte volta. Não
        é grátis (~100 ms reembalando ruído pelo embedding), então é uma vez por
        ditada, nunca por bloco.
        """
        self.model.reset()
        self._acima = 0
        self.ultimo = 0.0
        self._mudo_ate = time.monotonic() + self.cooldown
