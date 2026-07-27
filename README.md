# talk

Ditado por voz 100% local para Linux. Grava do microfone, transcreve com Whisper
na GPU e passa o texto por um LLM que corrige pontuação e remove muletas de fala.
Aperta um atalho, fala, aperta de novo — o texto aparece onde o cursor estiver.

Nada sai da máquina. Sem API key, sem nuvem, sem assinatura.

```
microfone → pw-record → faster-whisper (GPU) → LLM via Ollama → cola no cursor
```

## Os modelos

| papel | modelo | VRAM | observação |
|---|---|---|---|
| transcrição | Whisper `large-v3-turbo` via faster-whisper | ~1,2 GB | int8_float16, ~30x tempo real |
| detecção de voz | Silero VAD v6 | — | 1,2 MB, roda na CPU |
| limpeza do texto | `qwen3:4b-instruct` via Ollama | ~3,1 GB | Q4_K_M |

**Total em repouso: ~4,3 GB.** Medido numa RTX 3060 Laptop de 6 GB com o desktop
rodando. Sem GPU, cai para CPU automaticamente (bem mais lento).

O segundo modelo **não resume** — em ditado, resumir destrói o que você falou.
Ele edita: pontuação, acentuação, remoção de "é...", "tipo", "né", e conversão de
pontuação falada ("menos d" → `-d`, "barra home barra dev" → `/home/dev`).
Resumir existe como modo opcional.

---

## Instalação

Testado em Pop!_OS 24.04 (COSMIC/Wayland) com GPU NVIDIA. Deve funcionar em
qualquer distro com PipeWire.

### 1. Dependências do sistema

```bash
sudo apt install wl-clipboard libnotify-bin ydotool
```

- `wl-clipboard` — área de transferência no Wayland (use `xclip` no X11)
- `libnotify-bin` — as notificações de estado
- `ydotool` — emite o `Ctrl+V` que cola o texto no cursor

O `pw-record` (captura de áudio) já vem com o PipeWire em qualquer desktop
moderno.

### 2. uv

O gerenciador de pacotes Python usado aqui:

```bash
curl -LsSf https://astral.sh/uv/install.sh | sh
```

### 3. Ollama

Serve o modelo de limpeza:

```bash
curl -fsSL https://ollama.com/install.sh | sh
```

### 4. O projeto

```bash
git clone https://github.com/fioravante-dev/talk.git
cd talk
uv sync
```

### 5. Os modelos

O modelo de limpeza precisa ser baixado explicitamente (2,5 GB):

```bash
ollama pull qwen3:4b-instruct
```

O Whisper (1,6 GB) **baixa sozinho** do Hugging Face no primeiro uso, para
`~/.cache/huggingface/`. Para baixar agora em vez de na primeira ditada:

```bash
uv run talk --warmup
```

### 6. O serviço e o atalho

Carregar o Whisper leva ~4 s, o que inviabiliza chamá-lo a cada tecla. Um daemon
mantém o modelo na VRAM e o comando do atalho vira um cliente instantâneo.

```bash
./install.sh
```

Isso instala e liga um serviço de usuário do systemd, e põe um item no menu de
aplicativos para ligar/desligar sem terminal.

Falta registrar os atalhos. No COSMIC: *Settings → Keyboard → Shortcuts →
Custom*. No GNOME: *Configurações → Teclado → Atalhos personalizados*.

| tecla sugerida | comando |
|---|---|
| `Super+D` | `/CAMINHO/PARA/talk/.venv/bin/talk -l pt` |
| `Super+Alt+D` | `/CAMINHO/PARA/talk/.venv/bin/talk -l en` |

Use o caminho absoluto que o `install.sh` imprimir no final.

### 7. Permissão do /dev/uinput

O `ydotool` precisa escrever nesse dispositivo. Em muitas distros já vem
liberado pela tag `uaccess` do udev — confira antes de mexer:

```bash
getfacl /dev/uinput | grep "^user:"    # existe uma linha user:SEU_USUARIO:rw- ?
```

Se não existir, para a sessão atual:

```bash
sudo setfacl -m u:$USER:rw /dev/uinput
```

E para tornar permanente:

```bash
echo 'KERNEL=="uinput", MODE="0660", TAG+="uaccess"' \
  | sudo tee /etc/udev/rules.d/80-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

> O `ydotool` do Debian/Ubuntu é a série 0.1.x, que fala direto com o
> `/dev/uinput` e **não** tem `ydotoold`. O aviso `ydotoold backend unavailable`
> é esperado e inofensivo — o daemon só existe da versão 1.0 em diante.

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

`paste` é o padrão dos atalhos porque entrega o texto num bloco só. O `type`
emula as teclas uma a uma: é lento em texto longo e alguns aplicativos embaralham
a ordem. A contrapartida é que `paste` sobrescreve a área de transferência.

Se você ditar dentro de um terminal, a combinação de colar é outra:

```bash
talk --paste-key ctrl+shift+v
```

### Idioma

O idioma que vai para o LLM é o que o **Whisper detectou**, não o que você pediu
com `-l`. Se você ditar em inglês com o atalho de português, o texto é inglês e a
limpeza fica em inglês.

O prompt nomeia a língua explicitamente e usa exemplos por idioma. Sem isso, uma
fala que *menciona* idiomas ("traduz isso pro inglês") faz o modelo trocar de
língua no meio.

---

## Configuração

```bash
talk --init      # escreve ~/.config/talk/config.toml
```

Todo campo também aceita variável de ambiente (`TALK_MODEL`, `TALK_LLM`,
`TALK_MODE`…) e flag de linha de comando, nessa ordem de precedência.

O campo `vocab` vira o `initial_prompt` do Whisper. Coloque ali os nomes próprios
e jargões que ele erra:

```toml
vocab = "Kubernetes, PostgreSQL, Terraform, faster-whisper, systemd"
```

### Controle do serviço

```bash
talk --status    # daemon de pé?
talk --stop      # encerra a gravação em andamento
talk --quit      # desliga o daemon
talk --no-daemon # ignora o daemon nesta chamada
```

Sem terminal: `Super` → digite **talk** → o clique alterna ligado/desligado.
Botão direito no ícone da dock dá *Ligar*, *Desligar*, *Ver estado*. Útil para
liberar a VRAM para um jogo e religar depois.

---

## A fala nunca se perde

O áudio vai para o disco **enquanto** é capturado, antes de qualquer modelo ser
carregado. Só é apagado depois que o texto chegou ao destino *e* parece plausível.

Se a transcrição falhar, faltar VRAM, o serviço morrer ou a colagem não
funcionar, a gravação continua em `~/.cache/talk/recordings/` e a notificação diz
onde:

```bash
talk --recordings   # o que sobrou
talk --last         # reprocessa a última, do começo
talk --last -m raw  # ... sem passar pelo LLM
```

Durante a captura o arquivo é float32 cru, sem cabeçalho — assim um travamento no
meio deixa áudio íntegro em vez de um WAV corrompido. Vira WAV ao encerrar.

Uma transcrição curta demais para a duração do áudio (menos de ~0,5 palavra por
segundo) é considerada suspeita: o texto é entregue normalmente, mas a gravação é
preservada. Entregar com sucesso não é o mesmo que transcrever certo.

---

## Por que estes modelos

Medido nesta máquina, não copiado de benchmark.

### Transcrição

O [Parakeet TDT 0.6B v3](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) da
NVIDIA é menor, mais preciso no agregado e muito mais rápido. Mas o próprio model
card avisa que o treino usa **português europeu**, e a transcrição aqui já leva
0,7 s numa ditada de 20 s — velocidade não é gargalo. Ganharia onde não dói e
arriscaria onde dói.

### Limpeza

Três candidatos, nos casos que sabidamente quebram (ditado em imperativo, menção
a idiomas, pontuação falada):

| modelo | VRAM | latência | veredito |
|---|---|---|---|
| `qwen3:4b-instruct` | 3.168 MiB | 0,57 s | **escolhido** |
| `qwen2.5:3b-instruct` | 2.204 MiB | 0,48 s | falha na pontuação falada |
| `qwen3:1.7b` | 1.774 MiB | 1,84 s | modelo de raciocínio, gasta tokens pensando |

O caso que decidiu, ditando um comando:

> **cru:** *"roda o comando docker compose up menos d na pasta barra home barra dev barra projetos"*
>
> **4B:** `docker compose up -d na pasta /home/dev/projetos` ✅
>
> **3B:** `docker-compose up, menos d na pasta barra/home/barra/dev/projetos` ❌

Aquele 1 GB extra é exatamente o que converte fala em caminho de arquivo. Para
quem dita comandos, não é gordura.

O `phi4-mini` foi descartado antes: ele **executava** o ditado em vez de editá-lo
— você fala "cria um script em Python" e ele devolve o script.

---

## Resolução de problemas

### A transcrição sai como um fragmento sem sentido

Sintoma: 26 segundos de fala viram "E aí." — a duração está certa, o texto não.

Foi um bug real deste projeto, corrigido. O `pw-record`, mesmo com `--format f32`,
não escreve PCM cru no stdout: escreve um arquivo **AU (.snd)** com 24 bytes de
cabeçalho. Ler esse cabeçalho como áudio injeta seis amostras falsas, e uma delas
é `0xFFFFFFFF` — **NaN** em float32.

Um único NaN é catastrófico: o Whisper normaliza o espectrograma por
`log_spec.max()`, o NaN contamina o máximo e toda a matriz de features vira NaN.

Era intermitente porque o filtro VAD às vezes cortava esses milissegundos antes do
cálculo. Hoje o cabeçalho é descartado corretamente e há uma segunda barreira que
zera qualquer NaN antes do modelo. Se aparecer `[stt] N amostra(s) inválida(s)`
no log, a barreira agiu.

### O texto não cola, só fica na área de transferência

Verifique a notificação. Se disser *"não deu para colar"*, o `ydotool` falhou —
veja a permissão do `/dev/uinput` acima. O texto nunca é perdido: quando a
colagem falha, ele cai na área de transferência e a notificação avisa.

### Colagem funciona quase sempre, falha de vez em quando

O `wl-copy` retorna antes de o compositor publicar a seleção; colar nessa janela
cola o conteúdo anterior. O programa agora confirma que a área de transferência
contém o texto antes de emitir o `Ctrl+V`.

Se persistir, o suspeito é o `ydotool` criando um dispositivo virtual novo a cada
chamada. A alternativa é o `wtype`, que usa o protocolo `virtual-keyboard` do
Wayland e não passa pelo `/dev/uinput`.

### `CUDA failed with error out of memory`

Não deve aparecer como traceback — o programa detecta e refaz na CPU, com aviso
no log. Fica bem mais lento (~45 s em vez de ~4 s). Libere VRAM desligando o
serviço pelo menu, ou baixe `keep_alive` para o LLM sair da VRAM entre ditadas.

### O daemon não vê o microfone / não cola

O serviço systemd precisa das variáveis da sessão gráfica. Ele está ancorado em
`graphical-session.target` justamente por isso. Confira:

```bash
systemctl --user show -p MainPID --value talk.service   # pega o PID
tr '\0' '\n' < /proc/PID/environ | grep WAYLAND_DISPLAY
```

---

## Arquitetura

```
src/talk/
  cli.py      argumentos, orquestração, cliente do daemon
  daemon.py   socket unix, modelo quente na VRAM, notificações
  audio.py    captura via pw-record, cabeçalho AU, gravação em disco
  stt.py      faster-whisper, fallback para CPU, filtro de alucinação
  polish.py   prompts do LLM, anti-injeção, fixação de idioma
  output.py   stdout / clipboard / paste / type, com fallbacks
  config.py   TOML < env < flags
  cuda.py     carrega as libs CUDA dos wheels nvidia-*
```

O daemon existe só por causa do tempo de carregamento do modelo. Sem atalho de
teclado, `talk --no-daemon` faz tudo num processo só.

---

## Limitações conhecidas

- Modelos de limpeza pequenos às vezes *obedecem* o texto ditado em vez de
  editá-lo. O prompt usa delimitadores, exemplo e reforço por recência para
  evitar isso, mas abaixo de 3B ainda acontece. Na dúvida, `-m raw`.
- O `paste` sobrescreve a área de transferência a cada ditada.
- Não há detecção de qual aplicativo está em foco, então a combinação de colar é
  fixa: `ctrl+v` por padrão, `ctrl+shift+v` para terminais.
- Empacotamento é manual — não há `.deb`, AppImage nem assistente de instalação.
  Se você quer algo pronto para distribuir, veja
  [VoxType](https://github.com/peteonrails/voxtype) ou
  [Handy](https://github.com/cjpais/Handy).

## Projetos parecidos

Esta é uma categoria bem povoada; a lista curada
[awesome-voice-typing](https://github.com/primaprashant/awesome-voice-typing) tem
dezenas. [VoxType](https://github.com/peteonrails/voxtype) é o mais completo que
encontrei — cadeia de fallback wtype/dotool/ydotool, push-to-talk por evento de
soltar tecla, pausa de mídia via MPRIS e modo reunião.

O que este projeto faz e eles não: salvar o áudio antes de processar e recuperar
ditadas que falharam, e fixar o idioma do LLM pela detecção do Whisper.

## Licença

MIT — veja [LICENSE](LICENSE).
