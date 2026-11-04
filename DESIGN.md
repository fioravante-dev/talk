---
name: talk
description: A face de um aparelho de áudio para o ditado local; a janela é a oficina, a pílula é a luz de gravação.
colors:
  ground: "#e1e2e0"
  plate: "#eeefed"
  well: "#d5d7d4"
  ink: "#2a2a28"
  ink-2: "#5b5d5a"
  ink-3: "#626461"
  line: "#c3c5c2"
  ink-inverse-2: "#cfd1ce"
  lamp-green: "#3f8f4a"
  signal-red: "#d8442e"
  transcribe-amber: "#c98a1b"
typography:
  mark:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "19px"
    fontWeight: 700
    letterSpacing: "-0.01em"
  body:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.45
  section-title:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    letterSpacing: "0.02em"
  control:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "13px"
    fontWeight: 500
  note:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  caption:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "12px"
    fontWeight: 400
  time:
    fontFamily: "Schibsted Grotesk, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    fontFeature: "tnum"
rounded:
  hair: "1px"
  sm: "4px"
  md: "6px"
  pill: "9999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "28px"
  gutter: "32px"
  column: "48px"
components:
  button:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "6px 14px"
  button-hover:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.plate}"
  button-quiet:
    backgroundColor: "transparent"
    textColor: "{colors.ink-2}"
    rounded: "{rounded.md}"
    padding: "4px 8px"
  button-quiet-hover:
    backgroundColor: "{colors.well}"
    textColor: "{colors.ink}"
  segmented-option:
    backgroundColor: "transparent"
    textColor: "{colors.ink}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "9px 12px"
  segmented-option-checked:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.plate}"
  segmented-option-small:
    padding: "4px 12px"
  input:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "8px 12px"
  select:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.ink}"
    rounded: "{rounded.md}"
    padding: "8px 32px 8px 12px"
  kbd:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.ink}"
    rounded: "{rounded.sm}"
    padding: "2px 7px"
  header-bar:
    backgroundColor: "{colors.plate}"
    height: "64px"
    padding: "0 32px"
  footer-bar:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.ink-2}"
    typography: "{typography.caption}"
    height: "40px"
    padding: "0 32px"
  recording-pill:
    backgroundColor: "{colors.plate}"
    textColor: "{colors.ink}"
    rounded: "{rounded.pill}"
    height: "28px"
    width: "244px"
---

# Design System: talk

## Overview

**Creative North Star: "O Aparelho Braun"**

A janela do talk é a face de um aparelho de áudio dos anos 60: uma chapa de alumínio claro, tinta grafite, filetes finos e uma luz indicadora. Cada ajuste é um rótulo com o valor travado na borda direita, e nada na tela existe que não seja controle, leitura ou consequência. A densidade é de painel de instrumento: muitas linhas curtas, separadas por filete, nenhuma caixa em volta de nada.

A cor é quase toda neutra. O que tem cor é sinal: verde para pronto, vermelho para gravando, âmbar para transcrevendo. Os controles são mecânicos na aparência (segmentados de contorno grafite com o escolhido preenchido, teclas com a borda de baixo dupla, uma escala de sintonia com agulha) e todo controle mostra a consequência ao vivo, em texto, logo abaixo. A pílula de gravação, desenhada em cairo fora do webview, é a mesma chapa e a mesma fonte, reduzida a uma luz, um texto e um medidor.

O sistema recusa o painel de ajustes padrão do Linux (cartões arredondados agrupados, o azul do sistema) e o dashboard escuro com neon. Toda a interface é em português, e os títulos de seção são em minúsculas.

**Key Characteristics:**
- Chapa neutra fria (o fundo saiu do quente para o neutro porque o quente lia como creme).
- Uma tinta grafite faz tudo: texto, contorno, preenchimento do ativo, foco.
- Cor só como sinal de estado, nunca como decoração ou marca.
- Hierarquia por filetes de 1px e espaço, sem cartões nem sombras na janela.
- Rótulo à esquerda, valor na borda direita, uma mensagem por linha.
- Consequência ao vivo sob cada controle.

## Colors

Alumínio neutro e grafite, com três luzes de estado que só acendem quando há estado para mostrar.

### Primary
- **Grafite** (ink): a tinta única. Texto principal, contorno de botões e segmentados, preenchimento do segmento escolhido e do botão em hover, a barra de duração do histórico, a barra do limite da dica, o anel de foco, a seleção de texto.

### Secondary
- **Luz Verde Braun** (lamp-green): a luz indicadora em pronto e os segmentos acesos do medidor de microfone (janela e pílula).
- **Vermelho de Gravação** (signal-red): gravando. A luz pulsando no cabeçalho e na pílula, a agulha da escala de sintonia, os segmentos quentes do medidor (os dois últimos de 16 na janela, de 12 na pílula).

### Tertiary
- **Âmbar de Transcrição** (transcribe-amber): transcrevendo. A luz do cabeçalho e a luz da pílula, fixa, sem pulso. É uma adaptação do build: o contrato original previa só verde e vermelho, e o âmbar entrou para separar "ouvindo" de "processando" sem gastar o vermelho.

### Neutral
- **Alumínio** (ground): o fundo do corpo da janela. Neutro de propósito; o tom quente #e6e4df do primeiro desenho lia como creme e foi trocado.
- **Chapa** (plate): um tom acima do fundo. Faixa do cabeçalho, rodapé, fundo de campos, selects, teclas e da pílula; também a tinta invertida sobre grafite.
- **Poço** (well): o rebaixo. Hover de opções segmentadas e de botões discretos, fundo de `code`, trilho das barras de duração e de limite.
- **Segunda Tinta** (ink-2): títulos de seção, notas de consequência, abas inativas, texto do estado, rodapé, horário no histórico.
- **Terceira Tinta** (ink-3): placeholders, contagens, metadados da ditada, luz em serviço parado, opção desabilitada. No build ela ficou quase igual à segunda tinta (ligeiramente mais clara), de modo que a diferença é nominal; manter ambas legíveis sobre o alumínio vale mais do que o degrau.
- **Filete** (line): as linhas de 1px entre seções, sob o cabeçalho, sobre o rodapé, borda de campos e teclas, segmentos apagados do medidor, contorno da pílula. Entre linhas de uma lista (ditadas, termos) o filete entra a 60% de opacidade.
- **Segunda Tinta Invertida** (ink-inverse-2): o texto pequeno de apoio dentro de um segmento escolhido ou de um chip em hover, sobre grafite.

### Named Rules
**The Lights Are Signals Rule.** Verde, vermelho e âmbar existem só para estado: pronto, gravando, transcrevendo. Nenhum deles vira cor de botão, link, destaque ou erro. Erros são grafite em peso 600.

**The Red Is Recording Rule.** O vermelho fica reservado à gravação e ao que mede a gravação (luz, agulha, segmentos quentes do medidor). Se aparece vermelho na tela, algo está gravando ou é o ponteiro de um mostrador.

**The One Ink Rule.** Ativo, escolhido, focado e pressionado se expressam em grafite: preenchido ou contornado. Não existe cor de acento para interação.

## Typography

**Display Font:** Schibsted Grotesk (com sans-serif)
**Body Font:** Schibsted Grotesk (com sans-serif)
**Label/Mono Font:** ui-monospace só para `code` (comandos como `talk-rs toggle`)

**Character:** Uma grotesca só, auto-hospedada em woff2 (licença OFL, pesos 400 a 700), com a franqueza de uma serigrafia de painel. A mesma fonte é instalada pela pílula em `~/.local/share/fonts/talk` para que o cairo a ache pelo fontconfig; janela e pílula falam com a mesma voz.

### Hierarchy
- **Marca** (700, 19px, -0.01em): só a palavra "talk", em minúsculas, no cabeçalho.
- **Corpo** (400, 14px, 1.45): rótulos de linha, abas, texto das ditadas (até 75ch), campos.
- **Controle** (500, 13px): botões, opções segmentadas, o valor de uma linha (500 na borda direita).
- **Título de seção** (500, 12px, 0.02em, segunda tinta, em minúsculas): "atalhos", "colar com", "fica carregado depois da última ditada". Escrito em minúsculas no texto, sem `text-transform`.
- **Nota** (400, 13px, segunda tinta, até 60ch): a consequência ao vivo sob o controle.
- **Legenda** (400, 12px): rodapé, metadados, contagens, rótulos da escala, texto de apoio dentro de segmentos.
- **Pílula** (400, 12.5px, grafite): "gravando 0:07", "transcrevendo".

### Named Rules
**The Tabular Only On Time Rule.** Algarismos tabulares só em `<time>` (horário no histórico). Nesta fonte o tnum alarga também a vírgula e o ponto, e no texto corrido isso vira "Sim , sim ,". Nunca aplicar tnum no corpo ou em contêineres de texto.

**The Lowercase Label Rule.** Títulos de seção, abas e a marca são em minúsculas, escritas assim na fonte do texto. Botões e frases seguem a caixa normal do português ("Adicionar", "Vale a partir da próxima ditada.").

## Layout

Grade de três faixas fixas: cabeçalho de 64px, corpo rolável, rodapé de 40px. O corpo tem margem de 28px no topo e 32px nas laterais e embaixo. Ajustes e vocabulário usam duas colunas iguais com 48px de vão (32px abaixo de 900px; uma coluna só abaixo de 640px, que fica abaixo do mínimo da janela de 720px), limitadas a 1040px. O histórico é uma coluna de 1040px com grade por ditada: horário (52px), régua de duração (120px), texto, botão.

Cada seção começa com um filete de 1px no topo e respira 14px acima e 22px abaixo. Dentro dela, linhas de 7px de respiro vertical com rótulo à esquerda e valor à direita (`space-between`, alinhadas pela linha de base). O ritmo de espaçamento é 4, 8, 12, 16, 28, 32, 48.

No cabeçalho, marca, abas (28px entre elas) e estado ficam numa linha; a aba ativa é sublinhada por 2px de grafite colado ao filete inferior. O rodapé lista o que o aparelho consome: serviço, VRAM, última transcrição, com o número em grafite 500.

### Named Rules
**The Value On The Right Edge Rule.** Uma mensagem por linha: rótulo à esquerda, valor travado na borda direita. Teclas de atalho, contagem de tokens e "sobram N" obedecem a isso.

**The One Scale Rule.** No histórico todas as durações usam a mesma régua (a barra cheia corresponde ao valor indicado no cabeçalho da lista), para que durações se comparem de relance.

## Elevation & Depth

A janela é plana. A profundidade vem de três tons de chapa (poço abaixo, alumínio no meio, chapa acima) e de filetes de 1px; não há sombra em nenhum elemento da janela. Os únicos relevos são mecânicos: a borda inferior de 2px das teclas e o anel interno de 1px em grafite a 35% na luz indicadora.

A pílula é a exceção, porque flutua sobre outros apps: uma sombra curta e macia, em três camadas de grafite a 5%, 7% e 9%, deslocada 1,5px a 2,5px para baixo, como um aparelho pousado sobre a tela.

### Shadow Vocabulary
- **Pílula pousada** (cairo: três contornos arredondados expandidos 3, 2 e 1px, grafite a .05/.07/.09, deslocados +2,5px em y; aproximação CSS `0 2px 3px rgb(42 42 40 / .14)`): só na pílula de gravação.
- **Luz embutida** (`box-shadow: inset 0 0 0 1px rgb(42 42 40 / .35)`): a borda da luz indicadora no cabeçalho.

### Named Rules
**The Flat Plate Rule.** Na janela, nada flutua. Separação é filete ou troca de tom de chapa, nunca sombra ou cartão.

## Shapes

Cantos pouco arredondados (6px) em tudo que é controle: botões, segmentados, campos, select. Teclas, `code`, rótulos da escala e anéis de foco usam 4px (3px no foco global). Barras e segmentos de medida usam 1px, quase retos. As luzes são círculos de 10px. A pílula é o único formato totalmente arredondado (raio igual à metade da altura, 14px), com contorno de 1px em filete.

Bordas são sempre de 1px: grafite nos controles que se acionam (botão, segmentado), filete nos que recebem texto (campo, select, tecla). O foco do campo troca o filete por grafite e soma um anel de 1px em grafite.

## Components

### Buttons
Contorno grafite que enche ao toque, como uma tecla de aparelho.
- **Shape:** cantos levemente arredondados (6px), contorno de 1px em grafite.
- **Padrão:** fundo transparente, texto grafite 500 13px, 6px por 14px.
- **Hover / Focus:** enche de grafite com texto em chapa (transição de 120ms em fundo e cor); foco é contorno de 2px em grafite afastado 2px; ao pressionar desce 1px.
- **Discreto:** sem contorno, segunda tinta, 4px por 8px; no hover ganha o poço e a tinta principal. Usado em "remover".
- **Recopiar no histórico:** sempre visível mas sem contorno e em segunda tinta; o contorno aparece quando a ditada está sob o cursor; enche de grafite no hover e fica cheio com "copiado" depois do uso.

### Segmented Control
O seletor de aparelho: uma régua de opções com contorno grafite, a escolhida preenchida.
- **Style:** contorno de 1px em grafite, 6px de canto, divisórias de 1px em grafite entre opções; cada opção 500 13px, 9px por 12px, com uma linha de apoio de 12px em segunda tinta quando há.
- **State:** escolhida é grafite cheio com texto em chapa e apoio em segunda tinta invertida; hover é poço; foco é contorno interno de 2px; desabilitada fica em terceira tinta com cursor proibido.
- **Pequeno:** 4px por 12px, 12px, largura do conteúdo. Usado no idioma de cada atalho.
- São rádios nativos com o input invisível sobre a opção.

### Chips
- **Style:** o botão padrão com contorno em filete em vez de grafite; o número de ocorrências ("12×") vai em terceira tinta.
- **State:** hover enche de grafite como o botão; o número passa para segunda tinta invertida. Usados nas sugestões do histórico no vocabulário.

### Inputs / Fields
- **Style:** fundo chapa, filete de 1px, 6px de canto, 8px por 12px, corpo 400 14px; placeholder em terceira tinta.
- **Focus:** a borda vira grafite e soma um anel de 1px em grafite, sem contorno extra.
- **Hover:** a borda passa para segunda tinta.
- **Select:** mesmo corpo do campo, em 500, com seta em linha desenhada em SVG à direita.
- **Busca:** o botão de limpar é um X em linha de 1,5px em grafite.
- **Erro:** texto grafite 600 abaixo do formulário, sem cor.

### Navigation
- **Style:** abas em minúsculas no cabeçalho, corpo 14px em segunda tinta, 28px entre elas.
- **Hover:** tinta principal.
- **Ativa:** tinta principal em 500 e sublinhado de 2px em grafite que se sobrepõe ao filete do cabeçalho.

### Keys (kbd)
Teclas físicas: fundo chapa, filete de 1px com a borda de baixo em 2px, 4px de canto, 500 12px, 2px por 7px, 4px entre teclas. Ficam na borda direita da linha do atalho.

### Status Lamp
A luz indicadora: círculo de 10px com anel embutido, seguido do estado em 13px segunda tinta, na borda direita do cabeçalho. Verde em pronto ("pronto · modelo na GPU"), vermelho pulsando a 1s em gravando (com cronômetro), âmbar fixo em transcrevendo, terceira tinta em serviço parado.

### Tuning Scale
A escala de sintonia de "fica carregado": um controle funcional, não decoração. Traços maiores em grafite de 30px em cada parada (a cada 25%) e traços menores a 45% a cada 6,25%; uma agulha vermelha de 2px por 40px desliza para a parada escolhida (350ms, `cubic-bezier(.2, .8, .2, 1)`). Os rótulos ficam sob seus traços, em 12px segunda tinta; o escolhido fica em grafite 700. São rádios comuns desenhados como o mostrador, e a nota abaixo diz a consequência ("A GPU volta a 0 MB cinco minutos depois da última ditada").

### Level Meter
Medidor de segmentos: 16 retângulos de até 14px por 8px com 3px de vão, cantos de 1px. Apagados em filete, acesos em verde, os dois últimos em vermelho. Escala logarítmica do RMS. A pílula usa o mesmo mapeamento com 12 segmentos de 5px por 8px.

### Measure Bars
Duração da fala no histórico e limite da dica no vocabulário: trilho em poço (6px e 8px de altura), preenchimento em grafite, cantos de 1px. Duração usa uma escala única para todas as ditadas.

### Recording Pill
A pílula de gravação, GTK nativo e cairo numa camada layer-shell no rodapé da tela, a 48px da borda, sem teclado e com clique atravessando. Chapa com filete, raio total, sombra pousada. Gravando: 244px, luz vermelha pulsando, "gravando 0:07" e o medidor de 12 segmentos à direita. Transcrevendo: encolhe para 156px, luz âmbar fixa, "transcrevendo", sem medidor.

## Do's and Don'ts

### Do:
- **Do** separar seções com um filete de 1px no topo e espaço, sem cartão em volta.
- **Do** pôr o valor na borda direita da linha, com o rótulo à esquerda.
- **Do** escrever uma nota de consequência em segunda tinta sob cada controle que muda algo.
- **Do** expressar ativo, escolhido e hover em grafite: contorno, ou preenchido com texto em chapa.
- **Do** usar a mesma Schibsted Grotesk na janela e em qualquer superfície nativa, instalando-a localmente quando o desenho for fora do webview.
- **Do** escrever títulos de seção, abas e a marca em minúsculas, e toda a interface em português.
- **Do** desenhar controles de escala (traços, agulha) como rádios nativos, para que o mostrador seja o controle.

### Don't:
- **Don't** usar verde, vermelho ou âmbar fora dos estados pronto, gravando e transcrevendo; erro é grafite 600, não vermelho.
- **Don't** pôr sombra em elemento da janela; a sombra existe só na pílula que flutua sobre outros apps.
- **Don't** aplicar algarismos tabulares fora de `<time>`.
- **Don't** voltar o fundo para o alumínio quente (#e6e4df); ele lê como creme.
- **Don't** agrupar ajustes em cartões arredondados nem usar o azul do sistema.
- **Don't** fazer dashboard escuro com acentos neon.
