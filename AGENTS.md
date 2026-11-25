# AGENTS.md

Instruções para um agente de IA (Claude Code, Codex, Cursor...) que vai
instalar, atualizar ou mexer no talk. Para pessoas, o README.

## Instalar ou atualizar

Um comando, do zero até o serviço rodando. Pode rodar de novo: só faz o que
falta e recompila.

| sistema | comando |
|---|---|
| Linux (apt) | `curl -fsSL https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.sh \| bash` |
| Windows | `irm https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.ps1 \| iex` |

De dentro de um checkout: `./scripts/install.sh` ou `.\scripts\install.ps1`.

O que o agente precisa saber antes:

- **Precisa de uma placa NVIDIA com o driver instalado.** Sem `nvidia-smi` o
  script para no primeiro passo. Isso o agente não resolve: avise a pessoa.
- **Vai pedir senha ou confirmação.** No Linux, `sudo` para o apt e para a
  regra do `/dev/uinput`. No Windows, o UAC do winget para o CUDA e o Visual
  Studio. Rode num terminal onde a pessoa possa responder, ou peça para ela
  rodar.
- **A primeira compilação leva ~10 minutos** (o whisper.cpp com CUDA). Use um
  timeout longo e não interrompa.
- **Baixa ~1,1 GB de modelo** do Hugging Face, uma vez.
- A arquitetura da placa vem do `nvidia-smi`. Para forçar: `CUDA_ARCH=86`
  (RTX 30xx), `89` (40xx), `120` (50xx).

## Verificar

```bash
talk-rs --version       # a versão e o commit do código instalado
talk-rs status          # "idle" = pronto; erro = o serviço não está rodando
talk-rs toggle          # começa a gravar; de novo, para e cola onde está o cursor
```

No Windows o executável fica em `%LOCALAPPDATA%\Programs\talk\talk-rs.exe`.
Logs: `~/.cache/talk/` no Linux, `%LOCALAPPDATA%\talk\` no Windows.

Depois de instalado, o próprio talk se atualiza (`talk-rs update`, ou pela
bandeja). Ele puxa do código registrado em `~/.cache/talk/install.toml`.

## Onde fica cada coisa

| | Linux | Windows |
|---|---|---|
| programas | `~/.local/bin/` | `%LOCALAPPDATA%\Programs\talk\` |
| config, vocabulário | `~/.config/talk/` (`config.toml`, `vocab.txt`) | `%APPDATA%\talk\` |
| modelos, histórico, logs | `~/.cache/talk/` | `%LOCALAPPDATA%\talk\` |

O `config.toml` é relido a cada ditada e tem um comentário em cada chave. A
janela (Ajustes) edita as chaves sem apagar os comentários; prefira ela.

## Publicar uma versão

1. Suba a `version` do `Cargo.toml`, faça o commit e crie a tag `vX.Y.Z`.
2. No Windows, `scripts\package-windows.ps1` gera em `target\dist` o
   `talk-setup-X.Y.Z.exe`, o `talk-app-X.Y.Z-cudaN.zip` (só os programas, para
   atualizar) e um `.sha256` de cada.
3. `gh release create vX.Y.Z` com esses quatro arquivos. É a release que os
   talks instalados enxergam; sem ela, ninguém atualiza.

## Atalhos

- **Windows:** o serviço já vem com Ctrl+Alt+D (detecta o idioma) e
  Ctrl+Alt+Shift+D (inglês).
- **Linux:** os atalhos são do ambiente gráfico. No COSMIC, crie pela janela do
  talk (bandeja → Abrir talk → Ajustes → novo atalho). Em outro ambiente,
  cadastre um atalho do sistema que rode `talk-rs toggle -l auto`.

Não edite o arquivo de atalhos do COSMIC na mão enquanto a janela está aberta.

## Desenvolver

- `cargo test --release` e `cargo clippy --release --all-targets`. No Linux,
  com `CMAKE_CUDA_ARCHITECTURES` da placa. `./scripts/install.sh` instala o
  que você compilou.
- `talk-ui --dump <pasta>` gera as telas da janela em HTML, e
  `talk-pill --png <pasta>` a pílula em PNG: dá para conferir sem abrir nada.
- No Windows, compile **sempre** pelo `scripts\build-windows.ps1`. Ele carrega
  o Visual Studio, as variáveis do CUDA e as flags de otimização que o cmake-rs
  apaga. Sem elas, o whisper.cpp sai ~2× mais lento.
- O código Windows fica em arquivos `*_win.rs`, trocados por
  `#[cfg_attr(windows, path = ...)]`. Mudanças no Windows não mexem no Linux.
- O `unsafe` do projeto: `crates/talk-pill/src/main_win.rs` e os
  `#[unsafe(no_mangle)]` das funções JNI em `android/rust/src/lib.rs`.

## Fala (`talk-rs speak`)

Só no Linux. Opcional: `TALK_SPEECH=1 ./scripts/install.sh` cria o ambiente
Python em `~/.cache/talk/speech/venv` (`faster-qwen3-tts` e `transformers`
com versões fixas: o 5.24 quebra o modelo) e baixa o Qwen3-TTS 0.6B.

- O serviço põe a fala numa fila e volta na hora; um trabalhador em Python
  (`crates/talk/src/speech_worker.py`, embutido no binário e escrito em
  `~/.cache/talk/speech/` ao subir) sintetiza frase por frase e o `pw-play`
  toca. Ele sai depois de `speech_keep_alive` (30 min) e devolve a VRAM.
- Foco (`speech_when = "focus"`): o `talk-rs speak` lê `T3CODE_THREAD_ID`, acha
  o título da conversa no banco do T3 (`~/.t3/userdata/statev2.sqlite`, só
  leitura) e a fala espera a janela do T3 em foco mostrar aquela conversa
  (`crates/talk/src/focus.rs`, pelo `zcosmic_toplevel_info_v1`). Fora do T3 ou
  fora do COSMIC, toca na hora.
- Calar: `stop()` (a ditada começando, o clique no ícone falando) só corta o
  som e sobe a geração; a fala de agora e a fila terminam de ser geradas em
  silêncio e vão para o histórico. Enquanto `set_dictating(true)`, nada toca e
  o que espera o foco continua esperando. `hush()` também esvazia as que
  esperam. `speech_enabled = false` (bandeja, Ajustes, `talk-rs speech off`)
  faz o `speak` não gerar nada.
- Histórico: `~/.cache/talk/speech/spoken.jsonl` (texto, para sempre) e
  `clips/<id>.wav` (áudio, `speech_audio_keep = "7d"`, até
  `speech_audio_max_mb = 200`). Sem o áudio, `talk-rs again <id>` gera de novo.
- O que dá para testar sem GPU está em `talk-core`: `speech` (markdown → prosa
  → frases, idioma, o fio com o trabalhador) e `spoken` (histórico, WAV, limpeza).
- Log do trabalhador: `~/.cache/talk/speech/speech.log`.

## Celular (Android)

O app em `android/` é um teclado só de voz: grava, transcreve no PC pelo
Tailscale (ou no próprio celular, com o Parakeet v3 via sherpa-onnx ou o
large-v3-turbo, baixados pelo app) e digita no campo.

- No PC: `listen = "<ip do tailscale>:7878"` no `config.toml` e reiniciar o
  serviço. Ele passa a atender `GET /health`, `GET /peers` (os talks da
  tailnet, que o app usa para achar os PCs), `GET /vocab`, `POST /warm` e
  `POST /transcribe?language=pt` (corpo: WAV). Ver `crates/talk/src/remote.rs`.
- Sem sessão gráfica (máquina na tela de login), o serviço sobe assim mesmo e
  só atende pela rede: sem bandeja e sem colar.
- Compilar: `cd android && ./gradlew assembleRelease`, com `JAVA_HOME` num JDK
  17 e `ANDROID_HOME` no SDK com o NDK. O Gradle chama `android/rust/build.sh`,
  que compila o motor (Rust + whisper.cpp) com `cargo ndk`.
- O `android/rust` fica fora do workspace: o `cargo test` do PC não compila o
  whisper.cpp para Android. A transcrição é a mesma do PC:
  `crates/talk/src/stt.rs`, incluído por `#[path]`.
- O projeto todo está em português: comentários, mensagens e commits.
