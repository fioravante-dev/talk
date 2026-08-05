"""Leitura em voz alta: o caminho inverso do resto do projeto.

O talk transforma fala em texto. Isto transforma texto em fala — você seleciona
um trecho na tela, aperta o atalho, e ele lê. Não é leitura da tela: é leitura
do que você **selecionou**. A diferença importa, porque num recurso usado sem
olhar para o monitor você não tem como perceber que um OCR errou uma palavra.

Piper (VITS exportado para ONNX) na CPU. Medido nesta máquina: 233 ms até a
primeira amostra e 24,9x o tempo real na síntese — ou seja, ela nunca é o
gargalo, a reprodução é. Por isso a síntese é frase a frase e vai direto para o
player: esperar o texto inteiro atrasaria a primeira palavra por nada.

Nada disso toca a GPU. O Whisper e o LLM continuam donos da VRAM.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import threading
from pathlib import Path

# Medido na fase 0 com pt_BR-faber-medium: 19,6 caracteres por segundo de fala.
# Serve para avisar a duração antes de começar — quem seleciona um artigo
# inteiro merece saber que são quatro minutos antes de ouvir o primeiro deles.
_CHARS_POR_SEGUNDO = 19.6

VOZES = {"pt": "pt_BR-faber-medium", "en": "en_US-lessac-medium"}

# Formato fixo do Piper medium. Os players precisam saber, porque PCM cru não
# carrega cabeçalho nenhum.
_TAXA = 22_050


class VozError(RuntimeError):
    pass


def vozes_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache")
    return Path(base) / "talk" / "voices"


def caminho(voz: str) -> Path:
    return vozes_dir() / f"{voz}.onnx"


def faltando(*vozes: str) -> list[Path]:
    return [caminho(v) for v in vozes if not caminho(v).is_file()]


def _importar():
    """Importa o Piper só quando ele é necessário.

    A leitura é um extra opcional (`uv sync --extra voz`). Um import no topo do
    módulo faria o `talk` inteiro deixar de subir para quem só dita.
    """
    try:
        from piper import PiperVoice
    except ImportError as exc:
        raise VozError(
            "Piper não instalado — rode `uv sync --extra voz` no diretório do projeto"
        ) from exc
    return PiperVoice


def disponivel() -> bool:
    """O extra está instalado? Usado para não pedir vozes a quem não lê."""
    try:
        _importar()
        return True
    except VozError:
        return False


def baixar(voz: str) -> Path:
    _importar()
    from piper.download_voices import download_voice

    destino = vozes_dir()
    destino.mkdir(parents=True, exist_ok=True)
    download_voice(voz, destino)
    return caminho(voz)


# -- idioma ---------------------------------------------------------------


# Palavras funcionais: as mais frequentes de cada idioma e as que menos aparecem
# no outro. Contar isto decide o idioma sem modelo nenhum — um LLM faria o mesmo
# com um segundo de latência e 3 GB de VRAM no caminho.
_PT = {"de", "que", "não", "uma", "para", "com", "os", "as", "do", "da", "em",
       "no", "na", "é", "ao", "se", "por", "mais", "como", "mas", "ou", "você"}
_EN = {"the", "of", "and", "to", "in", "is", "that", "for", "it", "with", "as",
       "was", "on", "are", "be", "this", "have", "from", "or", "you", "at"}


def idioma(texto: str, padrao: str = "pt") -> str:
    """Decide entre 'pt' e 'en' contando palavras funcionais.

    Empate ou texto curto demais devolve o padrão: com três palavras não há
    evidência, e chutar errado soa muito pior do que usar a voz configurada.
    """
    palavras = re.findall(r"[a-zà-ÿ]+", texto.lower())
    if len(palavras) < 4:
        return padrao
    pt = sum(1 for p in palavras if p in _PT)
    en = sum(1 for p in palavras if p in _EN)
    if pt == en:
        return padrao
    return "pt" if pt > en else "en"


# -- preparo do texto -----------------------------------------------------


_BLOCO_CODIGO = re.compile(r"```.*?```", re.S)
_CODIGO_INLINE = re.compile(r"`([^`]*)`")
_URL = re.compile(r"https?://\S+|www\.\S+")
_MARCACAO = re.compile(r"(\*\*|__|\*|_|~~|^#{1,6}\s+|^\s*[-*+]\s+|^\s*>\s*)", re.M)
_ESPACO = re.compile(r"[ \t]+")
_LINHAS = re.compile(r"\n{3,}")


def preparar(texto: str) -> str:
    """Tira do texto o que só faz sentido para os olhos.

    O Piper lê literalmente: uma URL vira uma soletração de trinta segundos e
    `**negrito**` vira "asterisco asterisco negrito asterisco asterisco". Estas
    são regras fixas, não um modelo — o recurso existe para ler o que está lá, e
    um LLM no meio poderia reescrever sem você ter como conferir de ouvido.
    """
    texto = _BLOCO_CODIGO.sub(" (bloco de código) ", texto)
    texto = _URL.sub(" (link) ", texto)
    texto = _CODIGO_INLINE.sub(r"\1", texto)
    texto = _MARCACAO.sub("", texto)
    texto = _ESPACO.sub(" ", texto)
    texto = _LINHAS.sub("\n\n", texto)
    return texto.strip()


_FIM_DE_FRASE = re.compile(r"(?<=[.!?:;])\s+|\n+")


def frases(texto: str, maximo: int = 400) -> list[str]:
    """Quebra em pedaços para a síntese ser incremental.

    O corte é por sentença, e um pedaço longo demais é partido na vírgula: um
    parágrafo sem ponto final travaria a primeira palavra até ele terminar.
    """
    saida: list[str] = []
    for parte in _FIM_DE_FRASE.split(texto):
        parte = parte.strip()
        if not parte:
            continue
        while len(parte) > maximo:
            corte = parte.rfind(",", 0, maximo)
            if corte <= 0:
                corte = maximo
            saida.append(parte[: corte + 1].strip())
            parte = parte[corte + 1 :].strip()
        if parte:
            saida.append(parte)
    return saida


def duracao_estimada(texto: str) -> float:
    return len(texto) / _CHARS_POR_SEGUNDO


# -- reprodução -----------------------------------------------------------


def _player() -> list[str]:
    """PCM cru não tem cabeçalho, então o player precisa do formato na mão.

    O pw-play sem --raw tenta interpretar o fluxo como arquivo e fecha o pipe na
    primeira escrita — foi assim que este comando nasceu com o --raw.
    """
    if shutil.which("pw-play"):
        return ["pw-play", "--raw", "--format", "s16",
                "--rate", str(_TAXA), "--channels", "1", "-"]
    if shutil.which("paplay"):
        return ["paplay", "--raw", "--format", "s16le",
                "--rate", str(_TAXA), "--channels", "1"]
    raise VozError("nenhum player disponível — instale pipewire-bin ou pulseaudio-utils")


def _vigia(proc, parar: threading.Event, fim: threading.Event) -> None:
    """Mata o player assim que a parada for pedida.

    Sem isto a parada não funciona. A síntese é ~25x mais rápida que a
    reprodução, então o pipe do player enche e `proc.stdin.write()` BLOQUEIA
    esperando ele consumir. O laço de escrita só consulta `parar` entre uma
    escrita e outra, e fica preso lá dentro: medido, 2,5 s de fala continuavam
    saindo depois de mandar calar. Matar o processo de fora derruba junto o que
    já estava no buffer dele — que é exatamente o áudio que se quer descartar.
    """
    while not fim.is_set():
        if parar.wait(0.05):
            if proc.poll() is None:
                proc.kill()
            return


class Leitor:
    """Sintetiza e toca, frase a frase, podendo ser calado no meio."""

    def __init__(self, voz: str, *, velocidade: float = 1.0):
        PiperVoice = _importar()
        alvo = caminho(voz)
        if not alvo.is_file():
            raise VozError(f"voz não baixada ({alvo.name}) — rode `talk --download`")
        self.voz = voz
        self.motor = PiperVoice.load(str(alvo))
        # No Piper a escala é o inverso da velocidade: 0,5 fala em dobro.
        self.length_scale = 1.0 / velocidade if velocidade > 0 else 1.0

    def falar(self, texto: str, *, parar: threading.Event | None = None) -> bool:
        """Lê o texto. Devolve False se foi interrompido antes do fim.

        A parada mata o player E interrompe o laço de síntese. Só fechar a
        entrada do player deixaria tocar o que já foi escrito no buffer dele —
        que é justamente o áudio que você mandou calar.
        """
        pedacos = frases(preparar(texto))
        if not pedacos:
            return True

        parar = parar or threading.Event()
        proc = subprocess.Popen(
            _player(), stdin=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        fim = threading.Event()
        threading.Thread(target=_vigia, args=(proc, parar, fim), daemon=True).start()
        completo = True
        try:
            for pedaco in pedacos:
                if parar.is_set():
                    completo = False
                    break
                for chunk in self._sintetizar(pedaco):
                    if parar.is_set():
                        completo = False
                        break
                    try:
                        proc.stdin.write(chunk)
                    except BrokenPipeError:  # o player morreu ou foi morto
                        completo = False
                        break
                if not completo:
                    break
        finally:
            fim.set()
            if parar.is_set():
                completo = False
            if completo:
                # Fecha e espera: senão o processo morre com o fim da fala ainda
                # no buffer, e a última palavra some.
                try:
                    proc.stdin.close()
                    proc.wait(timeout=max(10.0, duracao_estimada(texto)))
                except (OSError, subprocess.TimeoutExpired):
                    proc.kill()
            else:
                proc.kill()
                proc.wait(timeout=2)
        return completo

    def _sintetizar(self, texto: str):
        from piper import SynthesisConfig

        cfg = SynthesisConfig(length_scale=self.length_scale)
        for chunk in self.motor.synthesize(texto, syn_config=cfg):
            yield chunk.audio_int16_bytes
