"""Entrega do texto: stdout, área de transferência ou digitação no app focado."""

from __future__ import annotations

import shutil
import subprocess
import sys
import time

_CLIPBOARD = (
    (["wl-copy"], "wl-clipboard"),
    (["xclip", "-selection", "clipboard"], "xclip"),
    (["xsel", "--clipboard", "--input"], "xsel"),
)

_TYPE = (
    # --delay dá tempo ao compositor de reconhecer o dispositivo virtual antes
    # dos primeiros caracteres; sem isso o começo do texto às vezes se perde.
    (["ydotool", "type", "--delay", "250", "--file", "-"], "ydotool"),
    (["wtype", "-"], "wtype"),
    (["xdotool", "type", "--clearmodifiers", "--file", "-"], "xdotool"),
)

# Para colar: o texto vai para a área de transferência e mandamos o atalho de
# colar. O @KEY@ é substituído pela combinação configurada.
_PASTE = (
    (["ydotool", "key", "--delay", "250", "@KEY@"], "ydotool"),
    (["xdotool", "key", "--clearmodifiers", "@KEY@"], "xdotool"),
)


# O caminho inverso: ler o que está selecionado, para a leitura em voz alta.
# É a seleção PRIMÁRIA — o texto que você acabou de destacar com o mouse, sem
# precisar de Ctrl+C. O clipboard não serve aqui: obrigaria a copiar antes e
# atropelaria o que você tivesse guardado para outra finalidade.
_SELECTION = (
    (["wl-paste", "--primary", "--no-newline"], "wl-clipboard"),
    (["xclip", "-selection", "primary", "-out"], "xclip"),
    (["xsel", "--primary", "--output"], "xsel"),
)


class OutputError(RuntimeError):
    pass


def selecao() -> str:
    """Texto destacado agora. Vazio se não houver nada.

    A seleção primária não tem idade: ela guarda o último trecho destacado,
    seja há dois segundos ou há duas horas, e não há como distinguir. Quem
    chama precisa mostrar o que vai fazer com ela antes de fazer.
    """
    argv, _ = _first_available(_SELECTION)
    if argv is None:
        raise OutputError("nenhuma ferramenta de seleção — instale wl-clipboard")
    try:
        out = subprocess.run(argv, capture_output=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    # Seleção vazia faz o wl-paste sair com erro; isso não é falha, é ausência.
    if out.returncode != 0:
        return ""
    return out.stdout.decode("utf-8", "replace").strip()


def _first_available(candidates):
    for argv, package in candidates:
        if shutil.which(argv[0]):
            return argv, package
    return None, None


def to_stdout(text: str) -> None:
    print(text)


def to_clipboard(text: str) -> None:
    argv, _ = _first_available(_CLIPBOARD)
    if argv is None:
        raise OutputError(
            "nenhuma ferramenta de clipboard encontrada — instale wl-clipboard "
            "(Wayland) ou xclip (X11)"
        )
    subprocess.run(argv, input=text.encode(), check=True)


def to_keyboard(text: str) -> None:
    argv, package = _first_available(_TYPE)
    if argv is None:
        raise OutputError(
            "nenhuma ferramenta de digitação encontrada — instale ydotool "
            "(funciona em qualquer compositor Wayland) ou xdotool (X11)"
        )
    try:
        subprocess.run(argv, input=text.encode(), check=True)
    except subprocess.CalledProcessError as exc:
        hint = ""
        if package == "ydotool":
            # O ydotool 0.1.x (o empacotado no Debian/Ubuntu) fala direto com o
            # /dev/uinput e não tem ydotoold; o daemon só existe a partir da 1.0.
            hint = (
                "\n  ydotool precisa de permissão de escrita em /dev/uinput:"
                "\n    sudo setfacl -m u:$USER:rw /dev/uinput"
                "\n  (na versão 1.0+ é preciso também: systemctl --user start ydotoold)"
            )
        raise OutputError(f"{package} falhou ({exc}){hint}") from exc


_CLIPBOARD_READ = (
    (["wl-paste", "--no-newline"], "wl-clipboard"),
    (["xclip", "-selection", "clipboard", "-o"], "xclip"),
    (["xsel", "--clipboard", "--output"], "xsel"),
)


def _await_clipboard(text: str, timeout: float = 2.0) -> bool:
    """Espera a área de transferência realmente conter o texto.

    O `wl-copy` retorna assim que delega a seleção a um processo filho, antes
    de o compositor publicá-la. Colar nessa janela cola o conteúdo ANTERIOR —
    é a explicação mais provável para uma colagem que funciona quase sempre e
    falha de vez em quando.
    """
    argv, _ = _first_available(_CLIPBOARD_READ)
    if argv is None:
        return False

    want = text.strip()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            out = subprocess.run(argv, capture_output=True, timeout=1.0)
        except (OSError, subprocess.SubprocessError):
            return False
        if out.stdout.decode("utf-8", "replace").strip() == want:
            return True
        time.sleep(0.02)
    return False


def to_paste(text: str, key: str = "ctrl+v") -> None:
    """Cola o texto de uma vez: copia e emite o atalho de colar.

    Bem mais rápido e confiável que digitar caractere a caractere, e o
    aplicativo recebe o texto como um bloco só.
    """
    to_clipboard(text)

    if not _await_clipboard(text):
        # Sem confirmação, emitir a tecla é aposta: pode colar o texto velho.
        print("[output] a área de transferência não confirmou o texto", file=sys.stderr)

    argv, package = _first_available(_PASTE)
    if argv is None:
        raise OutputError(
            "nenhuma ferramenta de teclado encontrada para colar — instale "
            "ydotool (Wayland) ou xdotool (X11). O texto ficou na área de "
            "transferência; cole com Ctrl+V."
        )
    argv = [key if a == "@KEY@" else a for a in argv]
    try:
        subprocess.run(argv, check=True)
    except subprocess.CalledProcessError as exc:
        raise OutputError(
            f"{package} falhou ao emitir {key} ({exc}) — o texto está na área "
            "de transferência, cole com Ctrl+V"
        ) from exc


_HANDLERS = {
    "stdout": to_stdout,
    "clipboard": to_clipboard,
    "type": to_keyboard,
    "paste": to_paste,
}


def deliver(text: str, targets: list[str], *, paste_key: str = "ctrl+v") -> list[str]:
    """Envia para todos os destinos pedidos; erros não impedem os demais.

    Devolve os destinos que realmente receberam o texto.
    """
    errors: list[str] = []
    delivered: list[str] = []

    for target in targets:
        try:
            if target == "paste":
                to_paste(text, paste_key)
            else:
                _HANDLERS[target](text)
            delivered.append(target)
        except (OutputError, subprocess.CalledProcessError) as exc:
            errors.append(f"{target}: {exc}")
            # Digitar e colar são os destinos que, ao falhar, não deixam rastro
            # nenhum — o texto some. O clipboard é a rede de segurança.
            if target in ("type", "paste") and "clipboard" not in targets:
                try:
                    to_clipboard(text)
                    delivered.append("clipboard")
                except (OutputError, subprocess.CalledProcessError):
                    pass

    for err in errors:
        print(f"[output] {err}", file=sys.stderr)
    # Lista vazia não é falha: é o caso de quem pediu só stdout, que o cliente
    # imprime por conta própria. Só há erro se algo foi pedido e nada funcionou.
    if targets and not delivered:
        raise OutputError("; ".join(errors) or "nenhum destino de saída funcionou")
    return delivered
