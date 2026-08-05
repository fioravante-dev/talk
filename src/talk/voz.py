"""Leitura em voz alta: o caminho inverso do resto do projeto.

O talk transforma fala em texto. Isto transforma texto em fala — você seleciona
um trecho na tela, aperta o atalho, e ele lê. Não é leitura da tela: é leitura
do que você **selecionou**. A diferença importa, porque num recurso usado sem
olhar para o monitor você não tem como perceber que um OCR errou uma palavra.

Dois motores, os dois em ONNX na CPU e sem tocar na VRAM:

  kokoro  padrão. Mais natural — venceu um teste cego pareado contra o Piper.
          347 MB no disco, ~4,5x o tempo real.
  piper   o leve. 61 MB por voz e ~25x o tempo real, ao custo de soar mais
          sintético. É a escolha certa em máquina fraca ou onde o disco pesa.

A síntese é frase a frase e vai direto para o player: esperar o texto inteiro
atrasaria a primeira palavra por nada, já que os dois motores sintetizam bem
mais rápido do que se fala.
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import threading
from pathlib import Path

import numpy as np

MOTORES = ("kokoro", "piper")

# Vozes padrão de cada motor. As do Kokoro saíram de teste de escuta; as do
# Piper são as menos sintéticas das quatro que existem em pt-BR.
PADRAO = {
    "kokoro": {"pt": "pm_alex", "en": "am_michael"},
    "piper": {"pt": "pt_BR-faber-medium", "en": "en_US-lessac-medium"},
}

# Caracteres por segundo de fala, medidos com a voz padrão de cada motor. Serve
# para avisar a duração antes de começar — quem seleciona um artigo inteiro
# merece saber que são quatro minutos antes de ouvir o primeiro deles.
_CHARS_POR_SEGUNDO = {"kokoro": 17.0, "piper": 19.6}

_KOKORO_URL = "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0"
_KOKORO_ARQUIVOS = ("kokoro-v1.0.onnx", "voices-v1.0.bin")

_LANG_KOKORO = {"pt": "pt-br", "en": "en-us"}


class VozError(RuntimeError):
    pass


def vozes_dir() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or (Path.home() / ".cache")
    return Path(base) / "talk" / "voices"


def voz_para(motor: str, lang: str, escolhida: str = "") -> str:
    """Resolve a voz: a configurada, ou o padrão do motor para o idioma.

    Vazio no config significa "o que o motor recomenda" — assim trocar de motor
    não obriga a trocar também os nomes das vozes, que não têm nada em comum
    entre os dois catálogos.
    """
    return escolhida or PADRAO[motor].get(lang, PADRAO[motor]["pt"])


# -- disponibilidade e download -------------------------------------------


def _kokoro_classe():
    try:
        from kokoro_onnx import Kokoro
    except ImportError as exc:
        raise VozError(
            "Kokoro não instalado — rode `uv sync --extra voz` no diretório do projeto"
        ) from exc
    return Kokoro


def _piper_classe():
    try:
        from piper import PiperVoice
    except ImportError as exc:
        raise VozError(
            "Piper não instalado — rode `uv sync --extra voz-leve` no diretório do projeto"
        ) from exc
    return PiperVoice


def disponivel(motor: str) -> bool:
    """O motor está instalado? Usado para não pedir modelos a quem não lê."""
    try:
        _kokoro_classe() if motor == "kokoro" else _piper_classe()
        return True
    except VozError:
        return False


def faltando(motor: str, *vozes: str) -> list[Path]:
    d = vozes_dir()
    if motor == "kokoro":
        # Um modelo só serve todas as 54 vozes: elas vivem no voices-v1.0.bin.
        return [d / "kokoro" / n for n in _KOKORO_ARQUIVOS
                if not (d / "kokoro" / n).is_file()]
    return [d / f"{v}.onnx" for v in vozes if not (d / f"{v}.onnx").is_file()]


def baixar(motor: str, voz: str = "") -> Path:
    if motor == "kokoro":
        return _baixar_kokoro()
    _piper_classe()
    from piper.download_voices import download_voice

    destino = vozes_dir()
    destino.mkdir(parents=True, exist_ok=True)
    download_voice(voz, destino)
    return destino / f"{voz}.onnx"


def _baixar_kokoro() -> Path:
    """Busca o modelo e o pacote de vozes das releases do projeto.

    São 347 MB e o download é longo o bastante para parecer travado sem
    indicação nenhuma — daí a barra.
    """
    import httpx

    destino = vozes_dir() / "kokoro"
    destino.mkdir(parents=True, exist_ok=True)
    for nome in _KOKORO_ARQUIVOS:
        alvo = destino / nome
        if alvo.is_file():
            continue
        parcial = alvo.with_suffix(alvo.suffix + ".parcial")
        with httpx.stream("GET", f"{_KOKORO_URL}/{nome}", follow_redirects=True,
                          timeout=120.0) as r:
            r.raise_for_status()
            total = int(r.headers.get("content-length", 0))
            baixado = 0
            with parcial.open("wb") as saida:
                for pedaco in r.iter_bytes(1 << 20):
                    saida.write(pedaco)
                    baixado += len(pedaco)
                    if total and sys.stderr.isatty():
                        pct = baixado * 100 // total
                        sys.stderr.write(
                            f"\r  {nome} {pct:3d}% ({baixado >> 20}/{total >> 20} MB)"
                        )
                        sys.stderr.flush()
        # Renomeia só no fim: um download interrompido não pode deixar para trás
        # um arquivo com o nome certo e o conteúdo pela metade.
        parcial.replace(alvo)
        if sys.stderr.isatty():
            sys.stderr.write("\n")
    return destino


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

    Os dois motores leem literalmente: uma URL vira uma soletração de trinta
    segundos e `**negrito**` vira "asterisco asterisco negrito asterisco
    asterisco". Estas são regras fixas, não um modelo — o recurso existe para
    ler o que está lá, e um LLM no meio poderia reescrever sem você ter como
    conferir de ouvido.
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


def duracao_estimada(texto: str, motor: str = "kokoro") -> float:
    return len(texto) / _CHARS_POR_SEGUNDO.get(motor, 18.0)


# -- motores --------------------------------------------------------------


class _Kokoro:
    """82M de parâmetros, Apache 2.0. Mais natural, e mais pesado no disco."""

    taxa = 24_000

    def __init__(self, nome: str, lang: str, velocidade: float, **_):
        Kokoro = _kokoro_classe()
        d = vozes_dir() / "kokoro"
        pendentes = faltando("kokoro")
        if pendentes:
            raise VozError(
                f"modelo do Kokoro não baixado ({pendentes[0].name}) — "
                "rode `talk --download`"
            )
        self.motor = Kokoro(str(d / _KOKORO_ARQUIVOS[0]), str(d / _KOKORO_ARQUIVOS[1]))
        self.nome = nome
        self.lang = _LANG_KOKORO.get(lang, "pt-br")
        self.velocidade = velocidade

    def sintetizar(self, texto: str):
        audio, _ = self.motor.create(
            texto, voice=self.nome, speed=self.velocidade, lang=self.lang
        )
        # O Kokoro devolve float32 em [-1, 1]; o player quer PCM de 16 bits.
        yield (np.clip(audio, -1.0, 1.0) * 32767).astype("<i2").tobytes()


class _Piper:
    """O leve: 61 MB por voz e ~25x o tempo real. Soa mais sintético."""

    taxa = 22_050

    def __init__(self, nome: str, lang: str, velocidade: float, cadencia: float = 1.2):
        PiperVoice = _piper_classe()
        alvo = vozes_dir() / f"{nome}.onnx"
        if not alvo.is_file():
            raise VozError(f"voz não baixada ({alvo.name}) — rode `talk --download`")
        self.motor = PiperVoice.load(str(alvo))
        self.nome = nome
        # No Piper a escala é o inverso da velocidade: 0,5 fala em dobro.
        self.length_scale = 1.0 / velocidade if velocidade > 0 else 1.0
        # Variação de duração dos fonemas: é o que tira o ritmo de metrônomo.
        # Acima de ~1,5 começa a produzir tremido em vogal sustentada.
        self.cadencia = cadencia

    def sintetizar(self, texto: str):
        from piper import SynthesisConfig

        cfg = SynthesisConfig(
            length_scale=self.length_scale, noise_w_scale=self.cadencia
        )
        for chunk in self.motor.synthesize(texto, syn_config=cfg):
            yield chunk.audio_int16_bytes


_CLASSES = {"kokoro": _Kokoro, "piper": _Piper}


# -- reprodução -----------------------------------------------------------


def _player(taxa: int) -> list[str]:
    """PCM cru não tem cabeçalho, então o player precisa do formato na mão.

    O pw-play sem --raw tenta interpretar o fluxo como arquivo e fecha o pipe na
    primeira escrita — foi assim que este comando nasceu com o --raw. E a taxa
    vem do motor: o Kokoro sai em 24 kHz e o Piper em 22,05 kHz; errar isso não
    dá erro, só deixa a voz mais grave ou mais aguda.
    """
    if shutil.which("pw-play"):
        return ["pw-play", "--raw", "--format", "s16",
                "--rate", str(taxa), "--channels", "1", "-"]
    if shutil.which("paplay"):
        return ["paplay", "--raw", "--format", "s16le",
                "--rate", str(taxa), "--channels", "1"]
    raise VozError("nenhum player disponível — instale pipewire-bin ou pulseaudio-utils")


def _vigia(proc, parar: threading.Event, fim: threading.Event) -> None:
    """Mata o player assim que a parada for pedida.

    Sem isto a parada não funciona. A síntese é bem mais rápida que a
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

    def __init__(self, motor: str, nome: str, *, lang: str = "pt",
                 velocidade: float = 1.0, cadencia: float = 1.2):
        if motor not in _CLASSES:
            raise VozError(f"motor desconhecido: {motor!r} (use kokoro ou piper)")
        self.chave = (motor, nome, lang)
        self.motor_nome = motor
        self.engine = _CLASSES[motor](nome, lang, velocidade, cadencia=cadencia)

    def falar(self, texto: str, *, parar: threading.Event | None = None) -> bool:
        """Lê o texto. Devolve False se foi interrompido antes do fim."""
        pedacos = frases(preparar(texto))
        if not pedacos:
            return True

        parar = parar or threading.Event()
        proc = subprocess.Popen(
            _player(self.engine.taxa), stdin=subprocess.PIPE, stderr=subprocess.DEVNULL
        )
        fim = threading.Event()
        threading.Thread(target=_vigia, args=(proc, parar, fim), daemon=True).start()
        completo = True
        try:
            for pedaco in pedacos:
                if parar.is_set():
                    completo = False
                    break
                for bloco in self.engine.sintetizar(pedaco):
                    if parar.is_set():
                        completo = False
                        break
                    try:
                        proc.stdin.write(bloco)
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
                    proc.wait(timeout=max(10.0, duracao_estimada(texto, self.motor_nome)))
                except (OSError, subprocess.TimeoutExpired):
                    proc.kill()
            else:
                proc.kill()
                proc.wait(timeout=2)
        return completo
