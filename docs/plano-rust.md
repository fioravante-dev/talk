# talk em Rust — plano

> **2026-11-04:** a versão em Python saiu do repositório e do computador; o
> workspace saiu de `talk-rs/` para a raiz. Leitura em voz alta, palavra-chave,
> `-f` e os modos de LLM saíram junto e, se voltarem, voltam em Rust. O que segue
> é o registro do plano como foi decidido.

Decidido em 2026-11-03, numa sessão de perguntas e respostas.

## Metas

O que conta é **(a) rapidez percebida** e **(b) consumo em repouso**. Instalar e
abrir rápido ficam em segundo plano. Medidas de partida, no talk em Python,
numa RTX 3060 Laptop com 6 GB:

| medida | hoje (Python) | meta |
|---|---|---|
| RAM do serviço em repouso | 3,5 GB | < 300 MB |
| VRAM em repouso | 1,2 GB (não liberava) | 0 |
| soltar o atalho → texto colado (ditada curta) | ~0,6 s de STT + 250 ms do ydotool + processos | < 1 s com large-v3 |

O modelo já roda em C++/CUDA; o ganho do Rust está em RAM, em liberar de
verdade e na colagem, não na transcrição.

## Decisões

1. **Reescrever do zero em Rust**, com o Handy (cjpais/Handy) só como
   referência, não como base. O código é nosso, porque o pipeline vai para o
   projeto da empresa.
2. **Escopo da v1**: atalho → grava → Whisper → cola; idioma por atalho;
   vocabulário; histórico; bandeja; atalhos no COSMIC. Ficam no Python até
   serem portados: leitura em voz alta, palavra-chave, modos de LLM, `-f`.
   **O Qwen sai.**
3. **Modelo**: Whisper **large-v3 completo** como padrão (FLEURS pt 3,88 contra
   4,17 do turbo); o turbo continua disponível no config. O Voxtral não é viável
   por enquanto: no modo tempo real é pior em pt (5,03 a 480 ms) e não cabe em
   6 GB sem quantização agressiva.
4. **Convivência**: workspace Cargo em `talk-rs/` neste repositório. Lê o mesmo
   `~/.config/talk/config.toml` e escreve o mesmo `history.jsonl`. O Python sai
   quando o Rust cobrir tudo.
5. **Motor**: whisper.cpp via `whisper-rs`, **condicionado à fase 0**. Se
   perder feio para o faster-whisper, troca para CTranslate2 (`ct2rs`).
6. **Interface**: Tauri só como moldura, no padrão de outro
   projeto Rust do autor (axum + askama + htmx + SSE): o serviço Rust renderiza as páginas com
   `askama` + `htmx`, com templates e CSS compilados no binário, sem npm e sem
   framework JS. O estado ao vivo (gravando, cronômetro, nível do mic, ditada
   nova) chega por SSE, **empurrado no instante do evento**, sem polling. As
   páginas trafegam por um protocolo interno do Tauri ou por um socket local,
   nunca por uma porta TCP em localhost, que qualquer site no navegador
   conseguiria alcançar. Desenhada com o Impeccable. A janela é
   criada ao abrir e destruída ao fechar; em repouso só ficam o serviço e a
   bandeja.
7. **Processos**: o serviço principal (bandeja, atalhos, colagem) nunca toca na
   CUDA. A transcrição roda num **trabalhador filho que termina** após 5 min
   ocioso (configurável). Ele começa a carregar no momento em que o atalho é
   apertado, em paralelo com a gravação.
8. **Colagem**: teclado virtual em `/dev/uinput`, criado **uma vez** ao iniciar
   o serviço; área de transferência escrita direto (`wl-clipboard-rs`). Sem
   ydotool, sem os 250 ms, sem abrir processos.
9. **Vocabulário**: `~/.config/talk/vocab.txt` é a fonte única. Cada linha é
   `Termo` ou `Termo = erro1, erro2`. Os termos viram dica para o Whisper
   (pessoais por último, porque o limite de ~224 tokens corta o começo); os erros são
   substituídos depois da transcrição. Substituir só o que nunca é palavra real
   ("PopiOS"); "Cloud" → "Claude" fica só como dica. A lista hoje no `vocab` do
   config migra para lá.
10. **Telas**: Histórico (com busca, copiar de novo, tempo de STT), Vocabulário
    (termos, erros, barra de tokens, sugestões tiradas do histórico), Ajustes
    (atalhos e idioma, modelo, tempo carregado, tecla de colar, microfone). Mais
    uma **pílula flutuante** via layer-shell durante a gravação, que não rouba o
    foco: nível do microfone e cronômetro.
11. **Design**: o Impeccable propõe as direções, que você escolhe **comparando
    duas de cada vez, às cegas**, como nas outras escolhas perceptivas do
    projeto.

## Fases

**Estado em 2026-11-03:** fase 0 concluída, com o whisper.cpp aprovado e o
large-v3 q5 como padrão (às cegas, 2 × 0 contra o turbo q8). Fase 1 no ar:
`talk-rs serve` pelo autostart, Super+D e Super+Alt+D chamando `talk-rs toggle`.
Medido: 101 MB de RAM em repouso, VRAM 0 até o primeiro atalho, ~0,5 s de STT
numa frase curta.

Lições da fase 1:
- O whisper.cpp só roda o VAD dentro do `whisper_full`; o `full_with_state`,
  que o whisper-rs usa, pula. O corte é feito à mão (`WhisperVadContext` +
  `clean::keep_speech`). Sem isso, silêncio vira "Inscreva-se no canal…".
- O Whisper guarda só o FIM do prompt (~224 tokens): os termos pessoais vão
  por último.

**0. Prova do motor** (1–2 dias)
- Gravar 10–15 ditadas reais com jargão ("Claude Code", "Pop!_OS", nomes de projetos).
- Protótipo `crates/talk-bench`: os mesmos WAVs no whisper.cpp (CUDA) e no
  faster-whisper atual, com large-v3 e turbo. Medir latência, pico de VRAM e
  tempo de carga a frio (processo novo → primeiro texto).
- Critério: whisper.cpp igual ou mais rápido segue; perdendo feio, troca para
  CTranslate2.

**1. Núcleo sem janela**
- Serviço: socket Unix, `talk toggle [-l pt|en]` para os atalhos, gravação via
  PipeWire, trabalhador filho com carga antecipada, teclado virtual, área de
  transferência, `history.jsonl`, `vocab.txt`.
- Bandeja (StatusNotifierItem) com o ícone que pulsa.
- Trocar os atalhos do COSMIC para o binário novo; o Python continua instalado
  para voltar.
- Medir contra as metas acima.

**2. Interface**
- Impeccable: direções → comparação em pares → telas.
- Tauri: Histórico, Vocabulário, Ajustes; pílula flutuante.

**3. Portar o resto**, um recurso por vez: leitura em voz alta (Piper/Kokoro em
ONNX via `ort`), palavra-chave (ONNX), `-f`, modos de LLM. Depois disso, remover
o Python.

## Pré-requisitos do sistema

```bash
sudo apt install nvidia-cuda-toolkit clang libclang-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  libgtk-layer-shell-dev librsvg2-dev
```

`nvcc` e `clang` são para a fase 0 (whisper.cpp com CUDA); o resto é do Tauri,
na fase 2.