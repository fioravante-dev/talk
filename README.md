# talk

Ditado por voz 100% local para Linux. Grava do microfone, transcreve com Whisper
na GPU e passa o texto por um LLM que corrige pontuação e remove muletas de fala.
Aperta um atalho, fala, aperta de novo — o texto aparece onde o cursor estiver.
Opcionalmente, uma palavra-chave abre a ditada sem tocar no teclado.

Nada sai da máquina. Sem API key, sem nuvem, sem assinatura.

```
      atalho ↘
                microfone → pw-record → faster-whisper (GPU) → LLM via Ollama → cola no cursor
palavra-chave ↗
```

## Os modelos

| papel | modelo | VRAM |
|---|---|---|
| transcrição | Whisper `large-v3-turbo` via faster-whisper | ~1,2 GB |
| detecção de voz | Silero VAD v6 | 1,2 MB, na CPU |
| limpeza do texto | `qwen3:4b-instruct` via Ollama | ~3,1 GB |
| palavra-chave (opcional) | `hey_jarvis` via openWakeWord | ~3,7 MB, na CPU |
| leitura em voz alta (opcional) | Piper `faber` (pt) e `lessac` (en) | ~121 MB, na CPU |

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
| `Super+Shift+D` | `/CAMINHO/PARA/talk/.venv/bin/talk -l pt -m email` |

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

**Pela voz:** diga a palavra-chave e emende a frase; ela para sozinha quando você
cala. Desligada por padrão — veja [Palavra-chave](#palavra-chave-hey-jarvis).

**Pelo terminal:**

```bash
talk                          # grava, limpa, imprime
talk -m raw                   # só a transcrição, sem LLM
talk -m prompt --to clipboard # vira um prompt bem-formado e copia
talk --loop                   # ditado contínuo até Ctrl-C
talk -f reuniao.m4a -m note   # transcreve um arquivo e organiza como nota
talk -l en                    # ditado em inglês
talk -m email                 # vira um e-mail formatado
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
| `email` | formata como e-mail: assunto, saudação, parágrafos, despedida |
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
| `wake` | `false` | ouvinte de palavra-chave (veja a seção abaixo) |
| `wake_threshold` | `0.5` | limiar de disparo; calibre com `--wake-test` |
| `wake_silence_secs` | `2.0` | silêncio que encerra a ditada aberta por voz |
| `voz_velocidade` | `1.0` | velocidade da leitura em voz alta |
| `read_language` | `auto` | voz da leitura; `auto` escolhe pelo texto |

`whisper_keep_alive` aceita `30s`, `5m`, `1h`, `0` (libera após cada ditada) ou
`never` (mantém sempre). Com o padrão de 5 minutos, ditadas em sequência são
instantâneas e a VRAM é devolvida quando você para.

O `vocab` vira o `initial_prompt` do Whisper:

```toml
vocab = "Kubernetes, PostgreSQL, Terraform, faster-whisper, systemd"
```

### Controle do daemon

O daemon sobe sozinho na primeira ditada e mantém o modelo quente entre uma e
outra. Não há nada para instalar nem serviço para habilitar: o cliente o cria
quando não encontra nenhum, herdando o ambiente gráfico de quem chamou — que é
justamente o que o clipboard e as notificações precisam.

Parado, ele não ocupa VRAM, só o processo.

```bash
talk --status    # de pé?
talk --stop      # encerra a gravação em andamento
talk --quit      # desliga
talk --no-daemon # ignora o daemon nesta chamada
```

O log dele fica em `~/.cache/talk/daemon.log`.

---

## Palavra-chave (`hey jarvis`)

Um **segundo gatilho**, que soma ao atalho em vez de substituí-lo. Às vezes a mão
já está no teclado e a tecla é mais rápida; às vezes ela está ocupada, e a voz é
a única entrada que sobra. Os dois convivem, e vem desligada por padrão.

### Ligar

```bash
uv sync --extra wake                 # no diretório do projeto
```

Depois, em `~/.config/talk/config.toml`:

```toml
wake = true
```

E baixe o modelo:

```bash
talk --download
talk --quit                          # o daemon precisa subir de novo para ouvir
```

> Um `uv sync` sem `--extra wake` **remove** esses pacotes. Use sempre o extra
> depois de ligar a palavra-chave.

### Como ela para

Esta é a diferença que importa em relação ao atalho: não há tecla para apertar de
novo. A ditada aberta por voz **para sozinha** após `wake_silence_secs` (2 s) de
silêncio, ou no teto de `wake_max_secs` (60 s). O `talk --stop` e o atalho de
teclado também a encerram, se você quiser cortar antes.

O corte por silêncio só arma depois de `wake_min_speech_secs` (0,5 s) de fala
somada. Isso não é um detalhe: o rabo da própria palavra-chave sobra na fila
quando o detector dispara, e sem esse acumulado ele contaria como fala — a pausa
natural de quem espera o "pode falar" estouraria os 2 s de silêncio antes da
primeira palavra, e a ditada sairia vazia.

E se ela disparar por engano — um ruído que soou como a palavra-chave — e essa
fala mínima não vier em `wake_grace_secs` (5 s), ela desiste calada: nenhuma
notificação, nenhum arquivo, e o Whisper nem chega a ser acordado. O motivo fica
registrado em `~/.cache/talk/daemon.log`, para um descarte indevido não se
parecer com "não escutou".

### Calibrar

```bash
talk --wake-test
```

Mostra o score ao vivo, de 0 a 1. Fale a palavra-chave umas dez vezes e anote o
pico; depois converse, tussa, digite, e anote o teto do ruído. O limiar
(`wake_threshold`, padrão 0.5) mora entre os dois — mais perto do ruído se você
prefere repetir, mais perto do pico se prefere não ser interrompido no meio de
uma conversa. Com um daemon de pé, o teste o cala enquanto roda e o devolve como
estava ao sair.

### Ouvir desde o login

O ouvinte só existe enquanto o daemon existe, e o daemon nasce na primeira
ditada. Para tê-lo escutando desde o login, crie
`~/.config/autostart/talk.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=talk (ouvinte de palavra-chave)
Exec=/CAMINHO/PARA/talk/.venv/bin/talk --serve
X-GNOME-Autostart-enabled=true
```

### O que custa

Vale saber antes de ligar, não depois:

- **O microfone fica aberto o tempo todo.** Em muitos desktops o indicador de
  captura e o LED do microfone ficam permanentemente acesos, e o `talk` aparece
  para sempre na lista de aplicativos usando o áudio.
- **~6 % de um núcleo**, contínuo (medido numa RTX 3060 Laptop; o modelo em si
  leva 2,4 ms por quadro de 80 ms, o resto é a captura e o laço).
- **+~55 MB de dependências** — o openWakeWord importa scipy e scikit-learn no
  topo do módulo, para um treinador de verificadores que este projeto não usa. É
  por isso que ela é um extra opcional.
- **Zero VRAM em repouso**: o Whisper continua só entrando quando você dita.
- O áudio continua sem sair da máquina. A detecção é local, como o resto.

### Só em inglês, por enquanto

Os modelos prontos são `hey_jarvis`, `alexa`, `hey_mycroft` e `hey_rhasspy` — a
palavra-chave é em inglês, a ditada continua em português. `hey_jarvis` é o mais
robusto dos quatro contra falso positivo.

Para uma em português, o openWakeWord tem um notebook que gera as amostras com
TTS sintético e sai com um `.onnx` em menos de uma hora numa GPU. O campo aceita
caminho, então não há código a mudar:

```toml
wake_model = "~/.cache/talk/wake/ei_computador.onnx"
```

Boas candidatas em pt-BR têm três ou quatro sílabas e não são palavras comuns —
"ei computador", "escuta talk". Uma palavra que aparece em conversa normal vira
ditada fantasma.

### Licença dos modelos

O código do openWakeWord é Apache-2.0, mas os **modelos pré-treinados são
CC-BY-NC-SA 4.0** — uso não comercial. É por isso que eles são baixados para
`~/.cache/talk/wake/` em vez de virem no repositório, que é MIT.

---

## Ler em voz alta

O caminho inverso do resto do projeto: em vez de voz virar texto, texto vira
voz. Você destaca um trecho com o mouse, aperta o atalho, e ele lê.

**É leitura da seleção, não da tela.** A diferença importa: o texto lido é
exatamente o que está lá, sem OCR no meio. Num recurso usado justamente quando
você não está olhando para o monitor, um erro de reconhecimento passaria sem
você ter como perceber.

### Ligar

```bash
uv sync --extra voz     # no diretório do projeto
talk --download         # baixa as duas vozes (~121 MB)
```

Registre um atalho, do mesmo jeito que os de ditado:

| tecla sugerida | comando |
|---|---|
| `Super+L` | `/CAMINHO/PARA/talk/.venv/bin/talk --read` |

### Como funciona

Destaque o texto e acione. **O mesmo atalho cala** — é o liga/desliga da
ditada, e o único jeito de descobrir que era o texto errado é ouvir os
primeiros segundos, então a saída precisa estar a um toque.

A notificação mostra o começo do texto e a duração estimada antes de começar
(`🔊 Lendo · PT · ~2 min`). Não há limite de tamanho: truncar calado esconderia
a perda. Se nada estiver destacado, ele avisa em vez de ficar mudo.

A voz é escolhida pelo texto — uma contagem de palavras funcionais decide entre
português e inglês. Para fixar, `read_language = "pt"` no config.

**Três coisas param a leitura:**

- o mesmo atalho, ou `talk --stop`;
- **começar uma ditada** (por tecla ou por voz) — falar por cima da máquina
  nunca é a intenção, e o microfone captaria o próprio Piper;
- **a palavra-chave**, se você a tiver ligada. Durante a leitura, dizer
  "hey jarvis" significa *calar*, não ditar. Isso te dá interrupção por voz sem
  tirar a mão do que está fazendo, e resolve por construção o fato de o
  alto-falante alimentar o microfone: se o próprio texto lido disparar o
  detector, a consequência é ele se calar — o que remove a causa.

### O que custa

- **~121 MB** das duas vozes, em `~/.cache/talk/voices/`.
- **Zero VRAM.** O Piper roda na CPU; o Whisper e o LLM seguem donos da GPU.
- Medido nesta máquina: 233 ms até a primeira amostra, e a síntese roda a ~25x
  o tempo real — a reprodução é sempre o gargalo, nunca o modelo.
- O texto passa por uma limpeza determinística antes: URL vira "link", markdown
  perde os marcadores, bloco de código é pulado. Regras fixas, não um LLM —
  ele poderia reescrever, e o ponto é ler o que está lá.

### Uma armadilha que não tem cura

A seleção primária **não tem idade**. Ela guarda o último trecho que você
destacou, seja há dois segundos ou há duas horas, e o programa não tem como
distinguir. Por isso a notificação mostra o começo do texto: se vier algo de
vinte minutos atrás, você vê na hora e cala num toque.

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
  audio.py    captura via pw-record, fluxo compartilhado, gravação em disco
  wake.py     ouvinte de palavra-chave, openWakeWord em ONNX (opcional)
  voz.py      leitura em voz alta com Piper, síntese frase a frase (opcional)
  stt.py      faster-whisper, fallback para CPU, filtro de alucinação
  polish.py   prompts do LLM, anti-injeção, fixação de idioma
  output.py   stdout / clipboard / paste / type, com fallbacks
  config.py   TOML < env < flags
  cuda.py     carrega as libs CUDA dos wheels nvidia-*
```

Sem atalho de teclado, `talk --no-daemon` faz tudo num processo só.

## Licença

MIT — veja [LICENSE](LICENSE).
