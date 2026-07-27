# talk

Ditado por voz 100% local para Linux. Grava do microfone, transcreve com Whisper
na GPU e passa o texto por um LLM que corrige pontuação e remove muletas de fala.
Aperta um atalho, fala, aperta de novo — o texto aparece onde o cursor estiver.

Nada sai da máquina. Sem API key, sem nuvem, sem assinatura.

```
microfone → pw-record → faster-whisper (GPU) → LLM via Ollama → cola no cursor
```

## Os modelos

| papel | modelo | VRAM |
|---|---|---|
| transcrição | Whisper `large-v3-turbo` via faster-whisper | ~1,2 GB |
| detecção de voz | Silero VAD v6 | 1,2 MB, na CPU |
| limpeza do texto | `qwen3:4b-instruct` via Ollama | ~3,1 GB |

Os dois são baixados por `talk --download`, ou sozinhos na primeira ditada se
você pular esse passo. Em repouso nada fica na VRAM: carregam quando você dita e
saem depois de ociosos.

O segundo modelo **não resume** — em ditado, resumir destrói o que você falou.
Ele edita: pontuação, acentuação, remoção de "é...", "tipo", "né", e conversão de
pontuação falada ("menos d" → `-d`, "barra home barra dev" → `/home/dev`).
Resumir existe como modo opcional.

---

## Instalação

Testado em Pop!_OS 24.04 (COSMIC/Wayland). Deve funcionar em qualquer distro com
PipeWire.

**Pré-requisitos:** driver NVIDIA instalado (`nvidia-smi` precisa responder) e
cerca de **7 GB de disco** — 2,6 GB de dependências Python, das quais a maior
parte são as bibliotecas CUDA, mais 1,6 GB do Whisper e 2,5 GB do modelo de
limpeza. Sem GPU funciona igual, só bem mais devagar, na CPU.

### 1. Dependências do sistema

```bash
sudo apt install wl-clipboard libnotify-bin ydotool
```

- `wl-clipboard` — área de transferência no Wayland (use `xclip` no X11)
- `libnotify-bin` — notificações de estado
- `ydotool` — emite o `Ctrl+V` que cola o texto no cursor

O `pw-record`, que captura o áudio, já vem com o PipeWire.

### 2. uv e Ollama

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
curl -fsSL https://ollama.com/install.sh | sh
```

O instalador do `uv` acrescenta `~/.local/bin` ao seu PATH, mas só nas sessões
seguintes — **reabra o terminal** antes de continuar.

### 3. O projeto

```bash
git clone https://github.com/fioravante-dev/talk.git
cd talk
uv sync
```

O comando fica em `.venv/bin/talk`, dentro do clone. Para chamá-lo de qualquer
lugar:

```bash
mkdir -p ~/.local/bin
ln -s "$PWD/.venv/bin/talk" ~/.local/bin/talk
```

Sem isso, use `uv run talk` de dentro do diretório do projeto.

### 4. Permissão do /dev/uinput

O `ydotool` precisa escrever nesse dispositivo. Em muitas distros já vem
liberado pela tag `uaccess` do udev — confira antes de mexer:

```bash
getfacl /dev/uinput | grep "^user:"    # existe uma linha user:SEU_USUARIO:rw- ?
```

Se não existir:

```bash
sudo setfacl -m u:$USER:rw /dev/uinput                    # sessão atual
echo 'KERNEL=="uinput", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/80-uinput.rules            # permanente
sudo udevadm control --reload-rules && sudo udevadm trigger
```

### 5. Os atalhos

Registre nas configurações do seu ambiente — no COSMIC, *Settings → Keyboard →
Shortcuts → Custom*; no GNOME, *Configurações → Teclado → Atalhos
personalizados*:

| tecla sugerida | comando |
|---|---|
| `Super+D` | `/CAMINHO/PARA/talk/.venv/bin/talk -l pt` |
| `Super+Alt+D` | `/CAMINHO/PARA/talk/.venv/bin/talk -l en` |

Use o caminho absoluto do seu clone. É só isso — não há serviço para instalar.

### 6. Os modelos

```bash
talk --download
```

Baixa os dois — Whisper e o modelo de limpeza — num comando só, com barra de
progresso. São ~4 GB, então leva alguns minutos. Nada é carregado na VRAM: isso
é preparação de máquina, não uso.

Você pode pular este passo: sem ele, os modelos são baixados sozinhos na
primeira ditada, e as notificações mostram o andamento. É só a espera que muda
de lugar.

---

## Uso

**Pelo atalho:** aperta, fala, aperta de novo. O texto é colado onde o cursor
estiver. As notificações mostram o estado: abrindo microfone → pode falar →
transcrevendo → pronto.

**Pelo terminal:**

```bash
talk                          # grava, limpa, imprime
talk -m raw                   # só a transcrição, sem LLM
talk -m prompt --to clipboard # vira um prompt bem-formado e copia
talk --loop                   # ditado contínuo até Ctrl-C
talk -f reuniao.m4a -m note   # transcreve um arquivo e organiza como nota
talk -l en                    # ditado em inglês
```

**A gravação vai até você mandar parar** — `Enter` ou `Espaço` no terminal, ou o
mesmo atalho de novo. Ela não corta sozinha no silêncio: pausar para pensar no
meio de uma frase é normal, e perder a fala por isso é pior do que apertar uma
tecla. Para cortar por silêncio, `--silence-secs 1.5`.

### Modos (`-m`)

| modo | o que faz |
|---|---|
| `raw` | transcrição crua, sem LLM |
| `clean` | pontuação e limpeza, preservando suas palavras (padrão) |
| `prompt` | transforma fala solta em um pedido claro para um assistente |
| `note` | organiza como nota em markdown |
| `summary` | resume em bullets |

### Saída (`--to`)

| valor | destino |
|---|---|
| `auto` | num terminal imprime; por atalho, cola no cursor (padrão) |
| `stdout` | imprime |
| `clipboard` | copia, sem colar |
| `paste` | copia e emite `Ctrl+V` — o texto entra de uma vez |
| `type` | digita caractere a caractere, via `ydotool` |

Se você ditar dentro de um terminal, onde `Ctrl+V` não cola:

```bash
talk --paste-key ctrl+shift+v
```

---

## Configuração

```bash
talk --init      # escreve ~/.config/talk/config.toml
```

Todo campo também aceita variável de ambiente (`TALK_MODEL`, `TALK_LLM`,
`TALK_MODE`…) e flag de linha de comando, nessa ordem de precedência.

Os principais:

| campo | padrão | o que faz |
|---|---|---|
| `mode` | `clean` | modo de pós-processamento |
| `to` | `auto` | destino do texto |
| `llm` | `qwen3:4b-instruct` | modelo de limpeza |
| `whisper_keep_alive` | `5m` | ocioso por esse tempo, o Whisper sai da VRAM |
| `keep_alive` | `5m` | o mesmo para o LLM, via Ollama |
| `vocab` | vazio | termos que o Whisper costuma errar |

`whisper_keep_alive` aceita `30s`, `5m`, `1h`, `0` (libera após cada ditada) ou
`never` (mantém sempre). Com o padrão de 5 minutos, ditadas em sequência são
instantâneas e a VRAM é devolvida quando você para.

O `vocab` vira o `initial_prompt` do Whisper:

```toml
vocab = "Kubernetes, PostgreSQL, Terraform, faster-whisper, systemd"
```

### Controle do daemon

O daemon sobe sozinho na primeira ditada e mantém o modelo quente entre uma e
outra. Parado, ele não ocupa VRAM.

```bash
talk --status    # de pé?
talk --stop      # encerra a gravação em andamento
talk --quit      # desliga
talk --no-daemon # ignora o daemon nesta chamada
```

---

## A fala nunca se perde

O áudio vai para o disco **enquanto** é capturado, antes de qualquer modelo ser
carregado. Só é apagado depois que o texto chegou ao destino e parece plausível.

Se a transcrição falhar, faltar VRAM ou a colagem não funcionar, a gravação
continua em `~/.cache/talk/recordings/` e a notificação diz onde:

```bash
talk --recordings   # o que sobrou
talk --last         # reprocessa a última, do começo
talk --last -m raw  # ... sem passar pelo LLM
```

---

## Arquitetura

```
src/talk/
  cli.py      argumentos, orquestração, cliente do daemon
  daemon.py   socket unix, ciclo de vida do modelo, notificações
  audio.py    captura via pw-record, gravação em disco
  stt.py      faster-whisper, fallback para CPU, filtro de alucinação
  polish.py   prompts do LLM, anti-injeção, fixação de idioma
  output.py   stdout / clipboard / paste / type, com fallbacks
  config.py   TOML < env < flags
  cuda.py     carrega as libs CUDA dos wheels nvidia-*
```

Sem atalho de teclado, `talk --no-daemon` faz tudo num processo só.

## Licença

MIT — veja [LICENSE](LICENSE).
