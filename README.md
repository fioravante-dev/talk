# talk

Ditado por voz 100% local para Linux. Aperta um atalho, fala, aperta de novo, e
o texto aparece onde o cursor estiver. Nada sai da máquina: sem API key, sem
nuvem, sem assinatura.

```
atalho → pw-record → Silero VAD → Whisper (whisper.cpp, GPU) → vocabulário → cola no cursor
```

Escrito em Rust. Em repouso o serviço ocupa ~100 MB de RAM e nenhuma VRAM: o
modelo só carrega quando você aperta o atalho, enquanto você fala, e sai da GPU
depois de ocioso.

## As peças

| binário | o que é | quando existe |
|---|---|---|
| `talk-rs serve` | o serviço: atalhos, bandeja, colagem, histórico | sempre (autostart) |
| `talk-rs worker` | o único processo que toca na CUDA; termina depois de ocioso | do atalho até `whisper_keep_alive` |
| `talk-ui` | a janela: histórico, vocabulário, ajustes | só enquanto está aberta |
| `talk-pill` | a pílula de gravação no rodapé da tela (layer-shell, sem foco) | só enquanto grava |

O código fica em `crates/`: `talk-core` (config, vocabulário, histórico, o fio
com o trabalhador; tudo testável sem GPU), `talk` (o serviço), `talk-ui`,
`talk-pill` e `talk-bench` (mede o whisper.cpp num corpus de WAVs).

## Os modelos

| papel | modelo | VRAM |
|---|---|---|
| transcrição (padrão) | Whisper large-v3, ggml q5_0 | ~2,2 GB |
| transcrição (mais rápido) | Whisper large-v3-turbo, ggml q8_0 | ~1,3 GB |
| detecção de voz | Silero v5.1.2, ggml | CPU |

Em português o large-v3 q5 ganhou do turbo num teste às cegas; o turbo é ~1,7×
mais rápido. Os dois ficam em `~/.cache/talk/models/`:

```bash
mkdir -p ~/.cache/talk/models && cd ~/.cache/talk/models
base=https://huggingface.co
curl -LO $base/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin
curl -LO $base/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin
curl -LO $base/ggml-org/whisper-vad/resolve/main/ggml-silero-v5.1.2.bin
```

## Instalação

Testado em Pop!_OS 24.04 (COSMIC, Wayland) com uma RTX 3060 de 6 GB.

```bash
sudo apt install nvidia-cuda-toolkit clang libclang-dev wl-clipboard libnotify-bin \
  libwebkit2gtk-4.1-dev libgtk-3-dev libgtk-layer-shell-dev
CMAKE_CUDA_ARCHITECTURES=86 cargo build --release   # 86 = RTX 30xx
for b in talk-rs talk-ui talk-pill; do install -m 755 target/release/$b ~/.local/bin/; done
```

Instale como cópia, não como link: recompilar troca o arquivo e o serviço em
execução perderia o trabalhador.

O teclado virtual que cola o texto escreve em `/dev/uinput`. Confira a
permissão com `getfacl /dev/uinput | grep "^user:"`; sem ela:

```bash
echo 'KERNEL=="uinput", MODE="0660", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/80-uinput.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
```

Suba o serviço no login com `~/.config/autostart/talk-rs.desktop`:

```ini
[Desktop Entry]
Type=Application
Name=talk-rs
Exec=sh -c "exec $HOME/.local/bin/talk-rs serve >>$HOME/.cache/talk/talk-rs-serve.log 2>&1"
NoDisplay=true
```

E os atalhos, no COSMIC em Configurações → Teclado → Atalhos personalizados:

| tecla | comando |
|---|---|
| `Super+D` | `talk-rs toggle -l auto` |
| `Super+Alt+D` | `talk-rs toggle -l en` |

Fixar `-l pt` num modelo multilíngue faz fala em inglês sair traduzida para o
português; por isso o padrão detecta.

## Configuração

`~/.config/talk/config.toml`, relido a cada ditada:

| chave | padrão | o que faz |
|---|---|---|
| `ggml_model` | `ggml-large-v3-q5_0.bin` | o modelo em `~/.cache/talk/models` |
| `whisper_keep_alive` | `5m` | ocioso por esse tempo, o trabalhador termina e a VRAM volta a 0 (`0`, `30s`, `never`) |
| `language` | vazio | vazio detecta; o `-l` do atalho vence |
| `paste_key` | `ctrl+v` | `ctrl+shift+v` para terminais |
| `input_device` | vazio | o `node.name` do microfone no PipeWire; vazio é o padrão do sistema |
| `to` | `auto` | `clipboard` só copia; `stdout` só registra no log |

A janela (`talk-ui`, ou "Abrir talk" na bandeja) edita essas chaves sem apagar
os comentários do arquivo.

## Vocabulário

`~/.config/talk/vocab.txt`, uma linha por termo, relido a cada ditada:

```text
Pop!_OS                       só a dica: o Whisper passa a preferir a grafia
Wayland = Weylend, Uêilend    a dica, e essas grafias viram "Wayland"
```

Os termos viram o `initial_prompt` do Whisper, que guarda só os últimos ~224
tokens: o `vocab` do config vai na frente e os termos do arquivo por último.
Troca só grafias que nunca são palavras de verdade.

## Desenvolvimento

```bash
cargo test --release
CMAKE_CUDA_ARCHITECTURES=86 cargo clippy --release --all-targets
talk-ui --dump /tmp/talk-ui      # as telas em HTML, para conferir sem abrir a janela
talk-pill --png /tmp             # os dois estados da pílula em PNG
```

O plano e as medidas da reescrita estão em `docs/plano-rust.md`; o sistema
visual, em `DESIGN.md`.
