#!/usr/bin/env bash
# Instala o daemon do talk como serviço de usuário do systemd.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN="$REPO/.venv/bin/talk"
UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

[ -x "$BIN" ] || { echo "não encontrei $BIN — rode 'uv sync' primeiro" >&2; exit 1; }

mkdir -p "$UNIT_DIR"
cat > "$UNIT_DIR/talk.service" <<EOF
[Unit]
Description=talk — daemon de ditado por voz
After=pipewire.service graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=$BIN --serve
Restart=on-failure
RestartSec=3
# O daemon é ocioso quase o tempo todo; não deixe que ele atrapalhe o desktop.
Nice=5

[Install]
WantedBy=graphical-session.target
EOF

# Botão no menu de aplicativos: alterna o serviço sem terminal.
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$APPS"
sed "s|@BIN@|$REPO/bin/talk-service|g" "$REPO/talk.desktop.in" > "$APPS/talk.desktop"
chmod +x "$REPO/bin/talk-service"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS" || true

systemctl --user daemon-reload
systemctl --user reenable talk.service
systemctl --user restart talk.service

echo "serviço instalado. Estado:"
sleep 3
systemctl --user --no-pager status talk.service | head -5

cat <<EOF

Agora registre os atalhos no seu ambiente apontando para:

  português : $BIN -l pt
  inglês    : $BIN -l en

O primeiro toque começa a gravar; o segundo encerra e o texto é digitado
onde o cursor estiver. Acrescente '--to clipboard' se preferir só copiar.
EOF
