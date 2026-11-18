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
talk-rs status          # "idle" = pronto; erro = o serviço não está rodando
talk-rs toggle          # começa a gravar; de novo, para e cola onde está o cursor
```

No Windows o executável fica em `%LOCALAPPDATA%\Programs\talk\talk-rs.exe`.
Logs: `~/.cache/talk/` no Linux, `%LOCALAPPDATA%\talk\` no Windows.

## Atalhos

- **Windows:** o serviço já vem com Ctrl+Alt+D (detecta o idioma) e
  Ctrl+Alt+Shift+D (inglês).
- **Linux:** os atalhos são do ambiente gráfico. No COSMIC, crie pela janela do
  talk (bandeja → Abrir talk → Ajustes → novo atalho). Em outro ambiente,
  cadastre um atalho do sistema que rode `talk-rs toggle -l auto`.

Não edite o arquivo de atalhos do COSMIC na mão enquanto a janela está aberta.

## Desenvolver

- `cargo test --release` e `cargo clippy --release --all-targets`. No Linux,
  com `CMAKE_CUDA_ARCHITECTURES` da placa.
- No Windows, compile **sempre** pelo `scripts\build-windows.ps1`. Ele carrega
  o Visual Studio, as variáveis do CUDA e as flags de otimização que o cmake-rs
  apaga. Sem elas, o whisper.cpp sai ~2× mais lento.
- O código Windows fica em arquivos `*_win.rs`, trocados por
  `#[cfg_attr(windows, path = ...)]`. Mudanças no Windows não mexem no Linux.
- O único `unsafe` do projeto é `crates/talk-pill/src/main_win.rs`.
- O projeto todo está em português: comentários, mensagens e commits.
