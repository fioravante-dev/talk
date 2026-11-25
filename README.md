# talk

Ditado por voz local, para Linux e Windows. Aperte o atalho, fale, aperte de
novo: o texto aparece onde está o cursor. Tudo roda na sua placa de vídeo, sem
nuvem, sem conta, sem assinatura.

![O talk transcrevendo um ditado em inglês num editor de texto](docs/demo/dog-diary.gif)

*15 s de fala, colados 0,9 s depois de soltar o atalho, numa RTX 3060 de
notebook. Com som: [dog-diary.mp4](docs/demo/dog-diary.mp4) ·
[note-to-self.mp4](docs/demo/note-to-self.mp4).*

## Como funciona

```
atalho → microfone → Silero VAD → Whisper large-v3 (whisper.cpp, CUDA) → vocabulário → cola no cursor
```

- O modelo só carrega na GPU quando você aperta o atalho, enquanto ainda está
  falando. Depois de 5 minutos parado ele sai, e a VRAM volta a zero. Em
  repouso o talk ocupa ~100 MB de RAM.
- Detecta português e inglês sozinho. Um segundo atalho fixa o inglês.
- Uma pílula no rodapé mostra que está gravando. A bandeja e a janela dão o
  histórico, o vocabulário (os nomes que o Whisper deve acertar) e os ajustes.
- Ele se atualiza: avisa quando sai uma versão e atualiza com um clique na bandeja.

Precisa de uma placa **NVIDIA RTX** (20xx a 50xx) com o driver instalado.

## Instalar

**Windows:** baixe o `talk-setup-<versão>.exe` em
[Releases](https://github.com/fioravante-dev/talk/releases) e rode. Ele
instala para o seu usuário, sem pedir administrador, e baixa o modelo (1 GB).

**Linux** (apt; testado no Pop!_OS 24.04 com COSMIC): compila para a sua
placa, ~10 minutos na primeira vez.

```bash
curl -fsSL https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.sh | bash
```

**Com um agente de IA** (Claude Code, Codex, Cursor...): peça *"instale o talk
seguindo o AGENTS.md de github.com/fioravante-dev/talk"*. O
[AGENTS.md](AGENTS.md) tem o que ele precisa saber.

Para compilar no Windows em vez de usar o instalador:
`irm https://raw.githubusercontent.com/fioravante-dev/talk/main/scripts/install.ps1 | iex`.

## Usar

| | Windows | Linux (COSMIC) |
|---|---|---|
| ditar, detectando o idioma | `Ctrl+Alt+D` | crie em Ajustes → novo atalho |
| ditar em inglês | `Ctrl+Alt+Shift+D` | idem, com o idioma inglês |

Aperte uma vez para gravar e outra para colar. Os atalhos, o modelo, o
microfone e o resto ficam em **Ajustes** (bandeja → Abrir talk). Em outro
ambiente gráfico no Linux, crie um atalho do sistema que rode
`talk-rs toggle -l auto`.

```bash
talk-rs status            # idle = pronto
talk-rs --version         # versão e commit
talk-rs update            # atualiza (o talk também avisa sozinho)
```

## Falar (o Claude lendo o resumo para você)

No Linux, o talk também fala: o Claude chama `talk-rs speak "resumo"` no fim de
cada resposta, e o resumo sai em voz alta, com uma voz natural (Qwen3-TTS na
placa, ~2,7 GB de VRAM, o primeiro som em 0,2 s). Português com a voz vivian,
inglês com a ryan.

- Numa conversa do **T3 Code**, a fala espera você abrir aquela conversa: o
  áudio é gerado na hora e toca quando a janela dela ganha foco (no COSMIC).
- **Clique no ícone** da bandeja: falando, cala; calado, fala de novo a última
  fala. As cinco últimas ficam em "Últimas falas", e todas na aba **falas** da
  janela. O texto fica para sempre; o áudio, por 7 dias.
- Começar uma ditada cala a fala, e nada toca enquanto você dita. Calada no
  meio, a fala ainda fica inteira no histórico.
- **Fala ligada**, no menu da bandeja ou em Ajustes, desliga tudo de uma vez:
  o `talk-rs speak` passa a não dizer nada.

```bash
TALK_SPEECH=1 ./scripts/install.sh      # instala a fala (~3 GB a mais)
talk-rs speak "Pronto, terminei."       # fala (o texto também pode vir do stdin)
talk-rs hush                            # cala
talk-rs speech off                      # desliga a fala (on liga)
talk-rs again                           # de novo a última
```

Para o Claude usar, ponha no `~/.claude/CLAUDE.md` algo como: *"No fim de cada
resposta, rode `talk-rs speak` com um resumo falado de 1 a 3 frases, sem
código nem listas."* Numa máquina sem o talk (o celular, um servidor), o
`scripts/talk-relay.sh` instalado como `talk-rs` repassa a fala ao PC pelo SSH.

## Desenvolver

Rust, em `crates/`: `talk` (o serviço), `talk-ui` (a janela), `talk-pill` (a
pílula) e `talk-core` (o que se testa sem GPU). Os comandos, o build do
Windows e como publicar uma versão estão no [AGENTS.md](AGENTS.md).

## Licença

[MIT](LICENSE)
