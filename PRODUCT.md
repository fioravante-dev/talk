# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

A janela é HTML/CSS num webview (wry, o motor do Tauri) no Linux. Ela parece um
app de desktop, mas a linguagem de design é web: não segue libcosmic nem GTK.

## Users

Por enquanto, uma pessoa só: o Pedro, dev, no notebook com Pop!_OS 24.04
(COSMIC, Wayland), RTX 3060 de 6 GB. Ele dita o dia inteiro, quase sempre em
português e às vezes em inglês, para agentes de código (Claude Code, T3 Code),
chats e mensagens de trabalho, cheio de jargão de dev e de nomes
próprios de projetos internos. Se o pipeline for aprovado, ele vai para um
projeto da empresa, que terá a interface de lá.

## Product Purpose

Ditado por voz 100% local: aperta o atalho, fala, aperta de novo, e o texto
aparece onde o cursor estiver. O sucesso é o texto certo, rápido, sem a pessoa
pensar na ferramenta. A janela não é o produto: ela existe para conferir o que
foi ditado e recopiar, ensinar palavras ao modelo e ajustar o que muda pouco.

## Positioning

Nada sai da máquina (sem nuvem, sem conta, sem assinatura) e, em repouso, ele
não ocupa nada: a GPU volta a zero e o serviço fica em ~100 MB. O vocabulário é
dele, editável, com trocas de grafias erradas.

## Operating Context

- O ditado acontece com o foco em OUTRO app; o talk nunca pode roubar o foco.
  Por isso a pílula de gravação usa layer-shell.
- Atalhos do COSMIC: Super+D detecta o idioma, Super+Alt+D fixa o inglês.
- A janela abre no histórico: as frases vêm primeiro, depois o vocabulário;
  ajustes são sempre secundários e ficam na última aba.
- A janela é aberta raramente: abre, olha ou ajusta, fecha. Ela é criada ao
  abrir e destruída ao fechar.
- A bandeja (StatusNotifierItem) mostra o estado o tempo todo: em repouso,
  gravando ou transcrevendo.

## Capabilities and Constraints

- Telas: Histórico (busca, recopiar, tempo de fala e de transcrição),
  Vocabulário (termos, grafias erradas que viram o termo, quanto do limite da
  dica já foi usado, sugestões tiradas do histórico), Ajustes (atalhos e o
  idioma de cada um, modelo large-v3 q5 / turbo q8, tempo que o modelo fica
  carregado, tecla de colar ctrl+v / ctrl+shift+v, microfone).
- Pílula flutuante durante a gravação: gravando, nível do microfone, cronômetro;
  depois, transcrevendo.
- Stack: páginas renderizadas pelo serviço Rust (askama + htmx), estado ao vivo
  por SSE empurrado no instante do evento, sem npm e sem framework JS, no padrão
  de outro projeto Rust dele (axum + askama + htmx + SSE). Nunca uma porta
  TCP em localhost.
- Os arquivos de sempre (os da versão em Python, que saiu): `~/.config/talk/config.toml`,
  `~/.config/talk/vocab.txt`, `~/.cache/talk/history.jsonl`.
- A dica do Whisper guarda só os últimos ~224 tokens: termos pessoais por
  último.
- Idioma da interface: português.

## Brand Commitments

O nome é **talk**, em minúsculas. Identidade própria, sem ligação com a marca
da empresa.

## Evidence on Hand

- O histórico real: `~/.cache/talk/history.jsonl` (500+ ditadas).
- O vocabulário real: `~/.config/talk/vocab.txt`.
- Medidas da fase 0 e 1 em `docs/plano-rust.md`.
- Não existem usuários, depoimentos ou números de mercado; não inventar.

## Product Principles

1. Some do caminho: o ditado é o produto, a janela é a oficina.
2. Mostrar o estado sem pedir atenção: gravando e transcrevendo se leem de
   relance e nunca roubam o foco.
3. A prova é o texto: o histórico mostra o que saiu e quanto demorou, para o
   usuário julgar se está rápido e certo.
4. Mudanças aditivas e reversíveis: nada liga sozinho, tudo volta atrás.
