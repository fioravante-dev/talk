#!/usr/bin/env bash
# talk-rs numa máquina sem o talk: repassa a fala ao talk de outra pelo SSH.
#
#   install -m 755 scripts/talk-relay.sh ~/.local/bin/talk-rs
#   TALK_HOST=popos talk-rs speak "Pronto, terminei."    # padrão: popos
#
# Para o Claude de uma máquina sem placa (o celular, um servidor) falar pelo
# alto-falante do PC. Dentro de uma conversa do T3, manda junto o título dela
# (lido do banco do T3 daqui), e a fala espera aquela conversa ganhar foco lá.
# Os outros comandos (hush, again, status) vão direto.

set -euo pipefail

host=${TALK_HOST:-popos}
remote='~/.local/bin/talk-rs'

# O título da conversa do T3 de onde isto foi chamado; vazio fora do T3.
thread_title() {
    [[ -n "${T3CODE_THREAD_ID:-}" ]] || return 0
    python3 -I - "$T3CODE_THREAD_ID" "${T3CODE_HOME:-$HOME/.t3}/userdata/statev2.sqlite" <<'EOF' 2>/dev/null || true
import sqlite3, sys
db = sqlite3.connect(f"file:{sys.argv[2]}?mode=ro", uri=True)
row = db.execute("SELECT title FROM orchestration_v2_projection_threads WHERE thread_id = ?", (sys.argv[1],)).fetchone()
print(row[0] if row and row[0] else "")
EOF
}

# O ssh junta os argumentos numa linha para o shell de lá: cada um vai escapado.
quoted() {
    local out=() a
    for a in "$@"; do out+=("$(printf '%q' "$a")"); done
    printf '%s ' "${out[@]}"
}

ssh_talk() {
    ssh -o BatchMode=yes -o ConnectTimeout=5 "$host" "$remote $(quoted "$@")"
}

cmd=${1:-}
if [[ "$cmd" == speak && "${2:-}" != -* ]]; then
    shift
    if [[ $# -gt 0 ]]; then text="$*"; else text=$(cat); fi
    args=(speak)
    title=$(thread_title)
    [[ -n "$title" ]] && args+=(--thread "$title")
    printf '%s' "$text" | ssh_talk "${args[@]}"
elif [[ "$cmd" == speak ]]; then
    ssh_talk "$@" # com opções (--help, --thread): o talk de lá decide
else
    ssh_talk "$@" </dev/null
fi
