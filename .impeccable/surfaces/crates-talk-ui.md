---
version: 1
slug: "crates-talk-ui"
primary_target: "crates/talk-ui"
related_targets: ["crates/talk-pill"]
---

# Janela do talk (Ajustes, Histórico, Vocabulário) e pílula de gravação

Modo: Operate. A janela abre no histórico: as frases vêm primeiro, depois o vocabulário; ajustes são sempre secundários e ficam na última aba (decisão do usuário em 2026-11-04, revertendo a resposta inicial de que ajustar era o motivo mais comum). A janela abre raramente e fecha logo. A pílula aparece só durante a gravação e nunca rouba o foco.

Restrições: português; páginas renderizadas pelo serviço/processo da UI em Rust (askama + htmx), estado ao vivo empurrado no instante do evento; nenhuma porta TCP; nada de npm. O usuário rejeita "cara de app web genérico".

## Direction contract

THESIS: A janela do talk é a face de um aparelho Braun dos anos 60: cada ajuste é um rótulo em minúsculas com o valor travado na borda direita, e nada que não seja controle. Recusa o painel padrão de ajustes do Linux (cartões arredondados agrupados, azul do sistema) e o dashboard escuro com neon.

OWN-WORLD: Fundo alumínio claro neutro (#e1e2e0; o quente #e6e4df lia como creme) com faixa de cabeçalho um tom acima (#eeefed), tinta grafite (#2a2a28), segunda tinta #5f5d58, filetes #c3c5c2 de 1px. Uma luz indicadora verde Braun (#3f8f4a) para pronto e vermelho (#d8442e) reservado a gravando e à agulha. Tipo: Schibsted Grotesk, auto-hospedada; títulos de seção em minúsculas pequenas; números tabulares. Controles: segmentados com contorno grafite e o ativo preenchido, teclas como kbd de borda inferior dupla.

STORY: Abre, vê de relance que está pronto e se o modelo está na GPU, ajusta um valor e vê a consequência na hora, fecha. No histórico, compara durações numa régua única e recopia; no vocabulário, ensina uma palavra e vê quanto do limite da dica sobra.

FIRST VIEWPORT: Cabeçalho de 64px: "talk" à esquerda, abas histórico / vocabulário / ajustes em linha (a janela abre no histórico) com sublinhado no ativo, luz indicadora e estado à direita. Corpo em duas colunas de seções separadas por filete: à esquerda atalhos e colar com; à direita modelo, fica carregado (escala de sintonia com traços e agulha vermelha, a consequência em texto abaixo) e microfone com medidor de segmentos. Rodapé de 40px com RAM do serviço, VRAM e o tempo da última transcrição.

FORM: Aparelho de áudio Braun (Rams), candidato 5 da lista ordenada. Elevações: uma mensagem por linha com valor na borda direita (sinalização de aeroporto); todo controle mostra a consequência ao vivo (espécime de fonte variável); durações numa escala única no histórico (fólio botânico). seed:6e46003dcce41902

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
