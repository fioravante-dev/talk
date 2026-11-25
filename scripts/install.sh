#!/usr/bin/env bash
# Instala (ou atualiza) o talk no Linux, do zero até o serviço rodando.
#
#   curl -fsSL https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.sh | bash
#   ./scripts/install.sh            # de dentro de um checkout
#
# Pode rodar de novo: só faz o que falta e recompila. Pede sudo para as
# dependências do apt e para a permissão do /dev/uinput (o teclado virtual que
# cola o texto). Precisa de uma placa NVIDIA com o driver instalado.
#
# Variáveis: TALK_DIR (onde fica o código, padrão ~/.local/src/talk),
# CUDA_ARCH (padrão: a da placa, pelo nvidia-smi), TALK_MODEL=turbo para o
# modelo mais rápido (1,3 GB) em vez do large-v3 q5 (1,1 GB, mais preciso).
# TALK_SPEECH=1 instala também a fala (`talk-rs speak`, ~3 GB a mais); quem já
# tem a fala a mantém atualizada sem pedir, e TALK_SPEECH=0 pula sempre.
# TALK_UPDATE=1 é o modo da atualização pela bandeja (`talk-rs update`): roda
# sem terminal, então nunca pede sudo, e avisa no fim por notificação.

set -euo pipefail

say() { printf '\033[1m[talk]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[talk]\033[0m %s\n' "$*" >&2; exit 1; }

REPO=https://github.com/fioravante-dev/talk.git
BIN="$HOME/.local/bin"
MODELS="${XDG_CACHE_HOME:-$HOME/.cache}/talk/models"
HF=https://huggingface.co

# 1. A placa: sem NVIDIA não há CUDA, e o talk transcreve na GPU.
command -v nvidia-smi >/dev/null || die "nvidia-smi não encontrado: instale o driver da NVIDIA primeiro"
if [[ -z "${CUDA_ARCH:-}" ]]; then
    CUDA_ARCH=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader | head -1 | tr -d ' .')
fi
say "placa: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1) (sm_$CUDA_ARCH)"

# 2. Dependências do sistema.
if command -v apt-get >/dev/null; then
    # Ferramentas: vale o comando, venha de onde vier (pip, snap, pkgconf...).
    missing=()
    for t in clang cmake git curl pkg-config wl-copy notify-send; do
        command -v "$t" >/dev/null && continue
        case $t in wl-copy) missing+=(wl-clipboard) ;; notify-send) missing+=(libnotify-bin) ;; *) missing+=("$t") ;; esac
    done
    command -v nvcc >/dev/null || [[ -x /usr/local/cuda/bin/nvcc ]] || missing+=(nvidia-cuda-toolkit)
    # Bibliotecas: vale o pacote.
    for p in libclang-dev libwebkit2gtk-4.1-dev libgtk-3-dev libgtk-layer-shell-dev; do
        dpkg -s "$p" >/dev/null 2>&1 || missing+=("$p")
    done
    if ((${#missing[@]})) && [[ -n "${TALK_UPDATE:-}" ]]; then
        die "faltam pacotes (${missing[*]}): rode o scripts/install.sh num terminal"
    fi
    if ((${#missing[@]})); then
        say "instalando: ${missing[*]}"
        sudo apt-get update -qq && sudo apt-get install -y "${missing[@]}"
    fi
else
    say "sem apt: confira que tem CUDA toolkit, clang/libclang, cmake, wl-clipboard, libnotify, webkit2gtk-4.1, gtk3 e gtk-layer-shell"
fi
command -v nvcc >/dev/null || [[ -x /usr/local/cuda/bin/nvcc ]] || die "nvcc não encontrado: instale o CUDA toolkit"

# 3. Rust.
if ! command -v cargo >/dev/null; then
    [[ -x "$HOME/.cargo/bin/cargo" ]] || { say "instalando o Rust (rustup)"; curl -fsSL https://sh.rustup.rs | sh -s -- -y; }
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# 4. O código: este checkout, ou um clone em TALK_DIR.
here=$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." 2>/dev/null && pwd || true)
if [[ -n "$here" && -f "$here/crates/talk/Cargo.toml" ]]; then
    src=$here
else
    src=${TALK_DIR:-$HOME/.local/src/talk}
    if [[ -d "$src/.git" ]]; then
        say "atualizando $src"; git -C "$src" pull --ff-only
    else
        say "baixando o código em $src"; git clone --depth 1 "$REPO" "$src"
    fi
fi

# 5. Compila e instala como cópia (recompilar troca o arquivo debaixo do
#    serviço em execução, e ele perderia o trabalhador).
say "compilando (a primeira vez leva uns 10 minutos: é o whisper.cpp com CUDA)"
(cd "$src" && CMAKE_CUDA_ARCHITECTURES=$CUDA_ARCH cargo build --release -p talk -p talk-ui -p talk-pill)
mkdir -p "$BIN"
pkill -x talk-rs 2>/dev/null && sleep 1 || true
for b in talk-rs talk-ui talk-pill; do install -m 755 "$src/target/release/$b" "$BIN/"; done

# 6. Os modelos.
mkdir -p "$MODELS"
fetch() { [[ -s "$MODELS/$1" ]] || { say "baixando $1"; curl -fL --progress-bar -o "$MODELS/$1.part" "$2" && mv "$MODELS/$1.part" "$MODELS/$1"; }; }
fetch ggml-silero-v5.1.2.bin "$HF/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin"
if [[ "${TALK_MODEL:-}" == turbo ]]; then
    fetch ggml-large-v3-turbo-q8_0.bin "$HF/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin"
    say "para usar o turbo: Ajustes → modelo, ou ggml_model no config.toml"
else
    fetch ggml-large-v3-q5_0.bin "$HF/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin"
fi

# 6b. A fala (`talk-rs speak`): o Qwen3-TTS só roda em Python, num ambiente
#     próprio. As versões vão fixas: o transformers 5.24 quebra o modelo.
#     Opcional: só com TALK_SPEECH=1, ou se a fala já está instalada (aí a
#     atualização a mantém em dia). TALK_SPEECH=0 pula sempre.
SPEECH="${XDG_CACHE_HOME:-$HOME/.cache}/talk/speech"
want_speech=${TALK_SPEECH:-}
[[ -z "$want_speech" && -x "$SPEECH/venv/bin/python" ]] && want_speech=1
if [[ "$want_speech" == 1 ]]; then
    if ! command -v uv >/dev/null && [[ ! -x "$BIN/uv" ]]; then
        say "instalando o uv (o Python da fala)"; curl -LsSf https://astral.sh/uv/install.sh | sh
    fi
    uv=$(command -v uv || echo "$BIN/uv")
    mkdir -p "$SPEECH"
    [[ -x "$SPEECH/venv/bin/python" ]] || "$uv" venv -q -p 3.12 "$SPEECH/venv"
    say "ambiente da fala (faster-qwen3-tts; a primeira vez baixa o PyTorch, ~3 GB)"
    "$uv" pip install -q -p "$SPEECH/venv" "faster-qwen3-tts==0.5.4" "transformers==5.15.1"
    say "baixando a voz (Qwen3-TTS 0.6B)"
    "$SPEECH/venv/bin/python" -c 'from huggingface_hub import snapshot_download as d; d("Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice")' >/dev/null
fi

# 7. O teclado virtual escreve no /dev/uinput.
if [[ -n "${TALK_UPDATE:-}" ]]; then
    : # a permissão veio na instalação; a atualização não pede sudo
elif ! getfacl -p /dev/uinput 2>/dev/null | grep -q "^user:$USER:"; then
    say "dando permissão ao /dev/uinput (sudo)"
    echo 'KERNEL=="uinput", MODE="0660", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/80-uinput.rules >/dev/null
    sudo udevadm control --reload-rules && sudo udevadm trigger
fi

# 8. Sobe no login, e anota de onde veio: é daqui que o `talk-rs update` puxa.
mkdir -p "$HOME/.config/autostart" "$HOME/.cache/talk"
printf "# Escrito pelo scripts/install.sh: de onde o talk-rs update puxa e recompila.\nsrc = '%s'\ncuda_arch = '%s'\n" \
    "$src" "$CUDA_ARCH" >"$HOME/.cache/talk/install.toml"
cat >"$HOME/.config/autostart/talk-rs.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=talk-rs
Comment=Ditado por voz local: bandeja, atalhos e colagem
Exec=sh -c "exec $BIN/talk-rs serve >>$HOME/.cache/talk/talk-rs-serve.log 2>&1"
Terminal=false
NoDisplay=true
X-GNOME-Autostart-enabled=true
EOF

# 9. E agora.
# Pelo systemd, se o ambiente já sobe o autostart como unidade (COSMIC, GNOME):
# assim o serviço não fica preso a este terminal.
unit='app-talk\x2drs@autostart.service'
if systemctl --user cat "$unit" >/dev/null 2>&1; then
    systemctl --user daemon-reload && systemctl --user restart "$unit"
elif command -v systemd-run >/dev/null; then
    systemd-run --user --quiet --collect --unit=talk-rs sh -c "exec $BIN/talk-rs serve >>$HOME/.cache/talk/talk-rs-serve.log 2>&1"
else
    setsid "$BIN/talk-rs" serve >>"$HOME/.cache/talk/talk-rs-serve.log" 2>&1 </dev/null &
fi
sleep 2
"$BIN/talk-rs" status >/dev/null || die "o serviço não subiu: veja ~/.cache/talk/talk-rs-serve.log"
if [[ -n "${TALK_UPDATE:-}" ]]; then
    notify-send -a talk "talk atualizado" "$("$BIN/talk-rs" --version)" || true
fi
say "pronto: $("$BIN/talk-rs" --version), na bandeja e no próximo login."
say "atalhos: abra o talk (bandeja → Abrir talk) → Ajustes → novo atalho; ou teste agora com: talk-rs toggle"
