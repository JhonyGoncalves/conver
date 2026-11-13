# Lista de mudanças

## 2.1.2

- **Corte inteligente em mais vídeos:** em alguns vídeos o corte Inteligente falhava e o Conver refazia tudo no modo Preciso, bem mais devagar. Eram dois casos: vídeos com um bloco de informação (SEI) depois do quadro-chave, como os do Flow ("Error muxing a packet"), e vídeos cujo quadro-chave não é IDR, como os do Papo de Elite ("reference count overflow"). Os dois agora saem no modo Inteligente.
- **Legenda no idioma certo:** em vídeos com dublagem automática do YouTube, a legenda "original" podia vir em inglês. Agora vem a do idioma do vídeo.

## 2.1.1

- **Só a legenda, refeito:** baixa uma legenda só, a do idioma falado no vídeo, em vez de três. Em **Ver idiomas** aparecem as outras que o vídeo tiver (as do autor, a automática e as traduções automáticas do YouTube), com o nome de cada idioma na própria língua, e dá para escolher entre SRT, TXT (só o texto) ou VTT.
- **Legendas automáticas sem repetição:** a legenda automática do YouTube repetia cada linha na fala seguinte. Agora ela é montada a partir do tempo de cada palavra, em frases de até duas linhas e sem tempos sobrepostos.
- **Baixar junto com o vídeo:** a opção de legenda também baixa uma só, a do idioma original, já sem repetição.
- **Pedidos demais:** quando o YouTube pede uma pausa (erro 429, comum nas traduções automáticas), o Conver espera e tenta de novo sozinho.
- **MCP e linha de comando:** `download` aceita `subtitle_lang` (`original`, um código como `pt` ou um id da lista) e `subtitle_format` (`srt`, `txt`, `vtt`); a nova ferramenta `subtitles_list` mostra os idiomas de um vídeo.

## 2.1.0

- **Cortar silêncios:** tira as pausas de vídeos e áudios sozinho, deixando a fala corrida (jump cut). Dá para ajustar a pausa mínima, o limite de volume e uma folga antes e depois da fala.
- **Vertical automático:** transforma vídeos deitados em 9:16, 4:5 ou 1:1 seguindo o rosto de quem fala, em vez de cortar sempre o centro. A câmera fica parada enquanto o rosto está no meio e pula direto quando a cena muda.
- **Melhores momentos:** a IA ouve vídeos longos (podcasts, lives, aulas) e escolhe os trechos mais fortes para virar cortes, com um título para cada. Os momentos vão para o `<vídeo>.cortes.json`, que abre direto em **Cortar trecho**; dá para usar a IA local ou a das Configurações e cortar na hora.
- **Restaurar foto:** recupera rostos borrados ou gastos em fotos antigas (GFPGAN), com ampliação 2× opcional do resto da foto.
- **Colorir foto:** dá cor a fotos em preto e branco com IA (DDColor), na resolução original.
- **Ícones e favicon:** de uma imagem, um `.ico` com todos os tamanhos do Windows ou o pacote completo para um site (favicon, PNGs, ícone da Apple, `site.webmanifest` e as linhas para o `<head>`).
- **Fluxos:** nodes novos **Chamar webhook** (avisa n8n, Zapier, Make ou Discord, com os dados ou com os próprios arquivos) e **Enviar para a nuvem** (copia para a pasta do OneDrive, Google Drive ou Dropbox do PC).
- **Download:** novas opções **Só a legenda** e **Só a capa**, sem baixar o vídeo.
- **Ícones por ferramenta:** cada ferramenta tem o seu ícone do Windows na tela inicial, na busca e na Fila.
- **Som ao soltar arquivos:** um som curto confirma que os arquivos chegaram. Dá para desligar em Configurações → Avisos.
- **IA nas Configurações:** atalhos prontos para DeepSeek, OpenRouter, Groq, Gemini, Ollama e LM Studio; **Testar conexão** lista os modelos da conta para escolher com um clique; e um aviso quando o endereço parece ser a página do site, e não o da API (era o erro 405 com a DeepSeek).
- **MCP e linha de comando:** ferramentas novas `media_cut_silence`, `video_auto_vertical`, `video_highlights`, `image_restore`, `image_colorize` e `image_icons`; `download` aceita `kind` `subtitles` e `thumbnail`.

## 2.0.1

- **Temas novos:** além de igual ao Windows, clara e escura, agora há Conver (as cores originais), Meia-noite, Floresta, Ametista, Areia e Alto contraste, em Configurações → Aparência.
- **Correções:** a seção Áudio aparecia em minúscula na tela inicial e no caminho do cabeçalho; na linha do tempo de Cortar trecho, o trecho para tirar ficava azul em alguns temas e agora é sempre vermelho.

## 2.0.0

- **Interface nova, no estilo do Windows 11:** o Conver agora começa pelo arquivo. Solte, cole (Ctrl+V) ou escolha arquivos na tela inicial e aparecem só as ferramentas que abrem aqueles tipos, com as usadas por último no topo; ao escolher uma, os arquivos vão junto.
- **Busca central (Ctrl+K):** acha qualquer ferramenta pelo nome ou por sinônimos ("tirar fundo", "aparar"), com as setas e o Enter.
- **Fila:** um painel com todas as tarefas em andamento, de qualquer ferramenta, com andamento, Cancelar, Abrir e Mostrar arquivo. Dá para começar uma tarefa, ir para outra ferramenta e começar outra.
- **Visual do Windows 11:** cor de destaque e tema do sistema, ícones do Windows, barra lateral compacta e cabeçalho com caminho (Início › Imagem). Botões, seletores, listas e a barra de ação de todas as ferramentas seguem o mesmo estilo.
- **Aparência:** o tema pode acompanhar o do Windows, ficar sempre claro ou sempre escuro. Em **Configurações → Interface**, a interface clássica continua disponível.

## 1.10.3

- **Fluxos, voz:** o node **Texto para voz** agora roda só com o texto. Antes ele falhava se nenhum arquivo chegasse.
- **Fluxos, música:** em **Música de fundo**, deixe a música em branco e ligue um áudio ao node, como uma narração gerada no próprio fluxo.

## 1.10.2

- **Cor e desfoque:** nova ferramenta na seção Vídeo. Deixe vídeos em preto e branco, sépia ou com as cores invertidas, desfoque a imagem inteira e ajuste brilho, contraste e saturação, com prévia em qualquer ponto do vídeo. O som continua o mesmo, regravado em AAC. Para mudar só um pedaço, corte o trecho antes.
- **MCP, linha de comando e fluxos:** nova ferramenta `video_adjust`.

## 1.10.1

- **Corte Inteligente:** novo modo em **Cortar trecho**, no ponto exato e bem mais rápido que o Preciso. Só o começo e o fim de cada trecho são regravados, e o resto do vídeo é copiado sem perda. O áudio é regravado em AAC, e só a primeira faixa de áudio é mantida. Funciona com H.264 de 8 bits em MP4, MOV e MKV; vídeos girados, como muitos de celular, e outros formatos usam o Preciso, e o resultado avisa. As emendas são conferidas no fim e, se algo falhar, o corte é refeito no Preciso.
- **Quadros-chave na linha do tempo:** aparecem como marcas, e no modo Rápido a borda arrastada gruda neles. A tela avisa quantos segundos o corte vai recuar.
- **MCP e linha de comando:** `media_trim` aceita `cut_mode` (`fast`, `precise` ou `smart`). No arquivo de cortes, use `"mode"`; `"precise"` continua aceito.

## 1.10.0

- **Vários cortes de uma vez:** em **Cortar trecho** e **Cortar áudio**, marque quantos trechos quiser para manter ou tirar, inclusive um dentro do outro. Um trecho para tirar dentro de um para manter abre um buraco nele; com um arquivo para cada trecho, um trecho pode ficar dentro de outro e os dois saem completos.
- **Linha do tempo:** mostra os trechos e o que vai sobrar, com o quadro do vídeo no ponto escolhido. Clique para mover o ponto, arraste as bordas do trecho ou use **Início aqui** e **Fim aqui**.
- **Arquivo de cortes:** um JSON pode dizer todos os cortes de um vídeo. Solte ele junto com o vídeo, use **Ler arquivo de cortes** ou deixe `<nome do vídeo>.cortes.json` ao lado do vídeo. **Salvar arquivo de cortes** gera um a partir da tela. O formato está no README.
- **MCP e linha de comando:** `media_trim` aceita `keep`, `remove`, `output` e `cuts_file` (um caminho ou `auto`). Os mesmos ajustes valem nos fluxos.

## 1.9.2

- **Colar da área de transferência:** tire um print e aperte Ctrl+V (ou o botão **Colar**) em uma ferramenta, sem salvar antes. Arquivos copiados no Explorer também entram. Os prints ficam em `Imagens\Conver\Prints`.
- **Mostrar arquivo corrigido:** com espaço no caminho, o Explorer abria em Documentos. Agora ele abre a pasta certa, com o arquivo selecionado.
- **Lista de arquivos:** em colunas estreitas, o título encurta em vez de ficar embaixo dos botões.

## 1.9.1

- **Remover fundo mais preciso em logos, ícones e desenhos:** quando o fundo é de uma cor só, o recorte vai pela cor. As partes escuras de dentro do objeto ficam, os vãos mínimos do contorno se fecham sozinhos e a borda sai limpa, sem halo. Em "Achar o objeto" dá para escolher entre automático, IA ou cor do fundo; em "Interior do objeto", manter ou tirar o que está dentro do contorno (como o miolo da letra O).
- **Remover objeto:** clique em uma parte da imagem, a IA reconhece o contorno e ela sai, deixando transparente, sem nada novo no lugar (SlimSAM, 113 MB, baixado no primeiro uso). Com "Só tirar o que eu marcar", o fundo fica e só o que você marcou sai. O pincel que apaga e preenche com a LaMa agora se chama "Apagar e preencher (IA)".
- **MCP e linha de comando:** `image_remove_background` aceita `detect` e `interior`.

## 1.9.0

### Novidades
- **Gerar imagem com modelos melhores:** além do modelo rápido (DreamShaper 8, 512 px), agora dá para escolher o **SDXL Lightning** (1024 px, bem mais detalhado; 6,9 GB) e o **Z-Image Turbo** (o melhor: entende pedidos longos, acerta mãos e rostos e escreve textos na imagem; 6,7 GB). Cada um é baixado só quando escolhido, e a tela já abre no melhor modelo que estiver no computador. Os modelos novos são mais lentos: numa placa de 8 GB, cerca de 2 minutos por imagem no SDXL e 4 a 7 minutos no Z-Image.
- **Modelos de IA** (boas-vindas e Configurações) mostram os três modelos de imagem para baixar ou apagar; em "Criar com IA" o sugerido passa a ser o Z-Image Turbo.
- **Editar imagem** (ferramenta nova, também pelo botão "Editar" nas imagens geradas), com IA no próprio PC:
  - **pedir uma mudança:** escreva "deixe de noite" ou "coloque um boneco de neve" e só isso muda (FLUX.2 klein, 2,8 GB);
  - **pintar e pedir:** pinte uma área e diga o que colocar ali; o resto fica igual;
  - **variar a imagem:** cria uma versão parecida, escolhendo o quanto muda.
- **Remover fundo com tela nova:** precisão em %, ajuste e suavização da borda, anticerrilhado, pincel para manter ou tirar partes do recorte, apagar objetos com a IA (com a opção de só apagar e manter o fundo), tudo com prévia ao vivo.
- **Vozes Kokoro:** dublagem e Texto para voz ganharam vozes bem mais naturais (92 MB) em português do Brasil, inglês, espanhol, francês, italiano e hindi. Elas aparecem primeiro nas listas; as vozes do Piper continuam disponíveis.
- **Mostrar arquivo:** depois de um trabalho, o botão abre o Explorer com o arquivo pronto já selecionado, em vez de só abrir a pasta.
- **MCP e linha de comando:** `image_generate` ganhou `model` (`auto`, `fast`, `sdxl` ou `z_image`); `image_edit` é novo; `image_remove_background` aceita `precision`, `grow`, `feather` e `antialias`; `text_to_speech` e `subtitle_translate` aceitam as vozes Kokoro.

## 1.8.0

### Novidades
- **Escanear documento:** foto de papel tirada com o celular vira documento reto e limpo: o Conver acha a folha, endireita a perspectiva e tira as sombras (preto e branco, tons de cinza ou cor). Salva em JPG/PNG ou junta todas as fotos num PDF. Também como node dos fluxos e no MCP (`document_scan`).
- **Fluxos de automação mais completos:**
  - **painel do node com os controles das telas:** seletores, sliders, seletor de cor, botão "Escolher…" para logo, música, legenda, vídeo e pasta, e a lista de microfones; ajustes que não valem para o que foi escolhido ficam escondidos;
  - **node "Condição":** separa os arquivos por orientação (vertical, horizontal, quadrada), duração, largura, altura, tamanho, páginas do PDF ou se tem som;
  - **resultado de cada node:** depois de rodar, clicar num node mostra o que ele produziu (miniaturas, arquivos que abrem com um clique, textos para copiar e a pasta);
  - **exportar e importar fluxos:** salve um fluxo num arquivo para compartilhar e abra fluxos de outras pessoas (sem os seus arquivos de entrada nem o horário);
  - **nodes novos:** Renomear em lote (com as mesmas regras da tela), Gravador de áudio (grava por um tempo) e Escanear documento; o QR Code ganhou a correção de erro.
- **MCP e linha de comando:** `document_scan`, `files_rename` e `audio_record`; `qr_code` aceita `error_correction`.

### Correções
- A barra de cima dos fluxos quebra linha quando não cabe (textos longos em outros idiomas).
- Apagar um fluxo também apaga as pastas das execuções dele.

## 1.7.1

### Correções
- **Textos longos em outros idiomas** (inglês, alemão…) não passam mais das bordas: nomes compridos no menu lateral são cortados com "…" (o nome inteiro aparece ao passar o mouse), rótulos das opções quebram linha dentro da coluna e os exemplos dos cartões das boas-vindas quebram linha, com o cartão crescendo junto.
- **Boas-vindas:** a frase sobre o download em segundo plano ficava por baixo do botão "Voltar"; agora tem linha própria.
- **Downloads de modelos:** o aviso de andamento no menu lateral cobria os últimos itens do menu; agora o espaço é reservado e o aviso fica acima de Histórico.
- **Fluxos de automação em português:** a dica de cada node (ao passar o mouse na lista "+ Adicionar node") aparecia em inglês; agora explica a ferramenta no idioma do Conver. Os ajustes dos nodes que apareciam com o nome técnico ("speed", "voice", "style"…) ganharam nome, e as opções das listas aparecem traduzidas: posições, formatos, idiomas e vozes pelo nome.
- **Renomear em lote e QR Code:** as dicas "procurar", "substituir", "por", "separador" e "https://… ou qualquer texto" agora são traduzidas.

## 1.7.0

### Novidades
- **Fluxos de automação:**
  - **rodar sozinho:** quando chega um arquivo novo numa pasta (com o Conver aberto, mesmo minimizado) ou num horário marcado, com os dias da semana (pelo Agendador de Tarefas do Windows, que roda o fluxo mesmo com o Conver fechado), e aviso do Windows ao terminar;
  - **histórico:** as últimas execuções de cada fluxo, com data, duração, resultados e erros, a pasta de cada uma e o botão **Repetir falhas**, que roda de novo só os arquivos que deram erro em algum node;
  - **renomear com padrão:** um node que copia cada arquivo com um nome novo, como `{opção}_{data}_{n}`, usando o que a IA local escolheu, a data, um contador, a pasta ou o nome original;
  - **o node "IA local: classificar" também ouve e lê:** além de imagens, classifica sons de áudios e vídeos (fala, música, silêncio, aplausos…, com o CLAP) e textos (arquivos .txt, .srt, PDF e textos de outros nodes, com uma IA de linguagem local).
- **Traduzir e dublar:** traduz legendas (ou a transcrição de vídeos e áudios) para 12 idiomas com uma IA de linguagem que roda no PC (Qwen 3.5 2B, pelo llama.cpp) e, se você quiser, dubla o vídeo com as vozes do Piper: cada fala entra no tempo da legenda, acelerada quando não cabe, com o som original baixinho por baixo.
- **Desfocar rostos:** acha os rostos de fotos e vídeos (detector YuNet, que vai junto no download) e os esconde com desfoque, pixelado ou tarja preta.
- **Marca d'água em lote:** logo ou texto em imagens, vídeos e PDFs, em 9 posições ou na diagonal, com tamanho, opacidade e margem, com prévia.
- **Melhorar voz:** som de podcast num clique: tira o ruído (IA), corta graves que embolam, dá presença, suaviza os "s", comprime e deixa em −16 LUFS (podcast) ou −14 LUFS (Reels e YouTube).
- **Tabelas do PDF para Excel:** tira as tabelas de PDFs, inclusive escaneados (pelo OCR do Windows), para .xlsx (uma aba por página, números de verdade) ou .csv.
- **Boas-vindas:** na primeira vez que o Conver abre, ele pergunta para que você vai usá-lo (vídeos para redes, podcast, documentos, automação, criar com IA…) e sugere os modelos de IA que valem a pena baixar, com o tamanho de cada um. Nada é baixado sem você escolher; dá para pular, e o download segue em segundo plano.
- **Configurações → Modelos de IA:** o que cada modelo faz, quais ferramentas usam, o que já está no computador e quanto ocupa, com botões para baixar antes ou apagar para liberar espaço.
- **Interface renovada:**
  - a opção escolhida nos seletores agora fica em vermelho suave, e o vermelho cheio fica só para a ação (botão principal, menu e progresso), então telas com muitas opções ficam mais calmas;
  - o vermelho dos botões ficou um tom mais fundo, para o texto branco ficar mais legível;
  - em árabe, urdu e persa o layout inteiro é espelhado: menu à direita, rótulos à direita dos controles e botão de ação à esquerda;
  - seletores com opções demais para a largura (ou com textos longos em outros idiomas) viram um menu suspenso;
  - o menu lateral rola até a ferramenta aberta;
  - "Fim" e "Duração" ganharam linha própria em Cortar e em Vídeo para GIF, e as pastas de destino do gravador, da gravação de tela e de Configurações aparecem numa caixa, como nas outras telas;
  - o japonês usa a fonte japonesa do Windows (antes os kanji e os kana saíam desalinhados), e os nomes dos idiomas aparecem certos no menu de idiomas.
- **MCP e linha de comando:** `subtitle_translate`, `image_hide_faces`, `watermark`, `audio_enhance_voice`, `pdf_tables` e `flow_history`; `flow_run` ganhou `retry_failed` e `notify`; `flow_save` aceita o gatilho (`trigger`) do fluxo; `image_classify` virou `classify` (o nome antigo continua valendo) e aceita `look_at`.

### Mudanças
- O llama.cpp (a IA de linguagem) não vai mais no download do Conver: é baixado do GitHub oficial, conferido pela assinatura, só por quem usa a tradução ou a classificação de textos.

## 1.6.0

### Novidades
- **IA local: classificar** (novo node dos fluxos de automação): uma IA que roda no próprio PC, sem internet, olha cada imagem (ou um quadro de cada vídeo) e escolhe entre as opções que você escrever, como "documento", "selfie" ou "comida". O node tem uma saída para cada opção e uma saída "incerto" para o que ficar abaixo da confiança mínima. Assim, cada arquivo segue para a ferramenta certa (por exemplo: documentos para o OCR, fotos de pessoas para remover o fundo). Usa o SigLIP 2, do Google; o modelo (412 MB) é baixado no primeiro uso. As opções podem ser escritas em português, inglês e mais de 100 idiomas. Cada imagem leva cerca de 0,3 s no processador.
- **MCP e linha de comando:** `image_classify` devolve, para cada arquivo, a opção escolhida, a confiança e a probabilidade de cada opção. Com `sort_into_folders`, copia cada arquivo para uma pasta com o nome da opção.

### Correções
- **Gerar imagem:** duas gerações no mesmo minuto não substituem mais as imagens da primeira (as novas ganham "-2", "-3"…).
- **Fluxos:** a saída "sim" dos nodes de filtro e de IA aparece traduzida nos outros idiomas.
- **Downloads de modelos de IA e atualizações** com antivírus que inspecionam conexões seguras (Kaspersky, Avast, ESET…): falhavam com "invalid peer certificate: UnknownIssuer". Agora o Conver usa os certificados do Windows, como o navegador.

## 1.5.2

### Correções
- **Transcrever / legendar:** o .txt, o .srt e o .vtt não trazem mais as marcações de som do Whisper, como "(upbeat music)" ou "[Música]", nem o travessão de troca de quem fala. Assim, a legenda gravada a partir do .srt não mostra mais esses textos.

## 1.5.1

### Melhorias
- **Legendas** (antes "Legendas animadas"):
  - **modo manual:** "Revisar o texto antes" transcreve o vídeo e mostra as frases para corrigir o texto e os tempos, apagar ou criar frases, e só depois grava no vídeo. Também abre uma legenda .srt/.vtt pronta e salva o texto corrigido em .srt;
  - **animação opcional:** legendas paradas, como as comuns, ou animadas palavra por palavra, com três efeitos (palavra colorida, caixa e "aparecendo") e o pulinho opcional;
  - **seis estilos prontos** (Reels, Caixa colorida, Uma palavra por vez, Aparecendo, Clássica, Faixa escura) e **estilos salvos** com o nome que você quiser;
  - **ajuste fino:** fonte, tamanho, negrito, itálico, cor do texto, do destaque e do contorno, espessura do contorno, sombra, faixa atrás do texto (cor e opacidade) e posição livre (clique ou arraste na prévia);
  - **prévia de verdade:** um quadro do próprio vídeo com a legenda desenhada pelo mesmo ffmpeg que grava, e o estilo escolhido fica salvo para a próxima vez.
- MCP e linha de comando: a ferramenta agora se chama `video_captions` (o nome antigo continua funcionando), com `preset`, `animated`, `effect`, cores em `#RRGGBB`, `font`, `font_size`, `outline`, `shadow`, `background_color`, `x`/`y` e `subtitle_file`. Um .srt com o mesmo nome do vídeo nos arquivos é usado no lugar da transcrição, para gravar uma legenda corrigida.

## 1.5.0

### Novidades (IA no próprio PC, sem internet depois de baixar o modelo)
- **Texto para voz:** transforma textos e legendas (.txt, .srt) em narração com voz natural, em MP3 ou WAV, com 4 velocidades. Usa o Piper; 10 vozes em português (3), inglês, espanhol, francês, alemão e italiano, cada uma baixada (uns 63 MB) na primeira vez que for usada. Um texto de 5 s de fala fica pronto em uns 2 s.
- **Gerar imagem:** descreva a imagem e ela é criada pela placa de vídeo (Vulkan, funciona em placas AMD, NVIDIA e Intel), em formato quadrado, retrato ou paisagem, 1, 2 ou 4 de uma vez. Com uma IA configurada, o pedido é traduzido para o inglês antes, que o modelo entende melhor. Usa o stable-diffusion.cpp e o modelo DreamShaper 8 LCM (2,1 GB, baixado no primeiro uso); cada imagem leva uns 10 a 20 s numa placa comum.
- **Apagar objeto:** pinte por cima de uma pessoa, um objeto, um texto ou uma marca d'água e a IA (LaMa) preenche o fundo. Dá para continuar apagando, comparar com o original e voltar atrás.
- **Ditado ao vivo:** fale e o texto aparece na tela enquanto você fala, com o Whisper. Numa pausa a frase é confirmada; o texto pode ser editado e copiado.
- **Legendas animadas (Reels/TikTok):** legendas palavra por palavra em que a palavra falada fica destacada, com um leve pulo. Três estilos (palavra colorida, caixa, uma palavra por vez), posição, cor, palavras por vez e maiúsculas, com prévia na tela.
- **Remover ruído:** tira chiado, ventilador e barulho de fundo de gravações de voz, em áudios e vídeos. Três forças: leve (só chiado) e duas com IA (RNNoise).
- **Organizar páginas do PDF:** miniaturas de todas as páginas para arrastar, girar e apagar, e salvar um PDF novo.
- As 7 ferramentas também estão na linha de comando, no MCP e nos fluxos de automação: `text_to_speech`, `image_generate`, `image_erase`, `video_animated_captions`, `audio_denoise` e `pdf_organize` (o ditado ao vivo é só na tela).

## 1.4.2

### Desempenho
- **Gravar a tela** e **Gravador de áudio** abrem na hora. A lista de microfones, que o Windows leva de 0,5 a 3 s para montar, agora é feita em segundo plano assim que o Conver abre, e as duas telas usam a mesma lista.
- **A faixa de áudio e o medidor andam lisos.** O Windows entregava o som do microfone em blocos de meio segundo; agora ele vem em blocos de 20 ms. O maior intervalo medido caiu de 525 ms para menos de 100 ms. A gravação de tela com microfone também usa blocos menores.
- **Fluxos de automação:** a configuração da IA não é mais lida do disco e decodificada a cada quadro enquanto um node de IA está escolhido.

## 1.4.1

### Melhorias
- **Gravar a tela:**
  - nova opção **Um monitor**, para gravar só o monitor escolhido na lista, que mostra o principal e a resolução de cada um;
  - **Pausar / Continuar** durante a gravação: os trechos são juntados num MP4 só, sem recomprimir, e o cronômetro conta só o tempo gravado;
  - **qualidade** (Alta, Média ou Leve) e **resolução máxima** (Original, 1080p ou 720p);
  - as mesmas opções no MCP e na linha de comando: `monitor`, `quality` e `resolution` no `screen_record`, e os monitores no `list_devices`.

## 1.4.0

### Novidades
- **Fluxos de automação por nodes** (nova seção Automação no menu): monte uma sequência de ferramentas ligando nodes num quadro. O resultado de um node passa para o próximo, e todas as ferramentas do Conver viram nodes. Há também nodes de entrada, de texto, de filtro (por tipo, extensão ou tamanho) e de "Salvar em pasta". O quadro permite arrastar, aproximar e afastar, e salva sozinho; arrastar arquivos para a janela os coloca na entrada. Dois fluxos de exemplo vêm prontos.
- **IA dentro dos fluxos:** o node **IA: instrução** pede algo à IA (descrever, resumir, traduzir, escrever legendas…) e o node **IA: avaliar** confere cada arquivo e o manda para a saída "sim" ou "não". Funciona com a Anthropic (Claude) e com APIs compatíveis com a da OpenAI, inclusive Ollama e LM Studio no próprio PC. A configuração fica em Configurações → Inteligência artificial, e a chave é guardada protegida pelo Windows.
- **MCP e linha de comando:** `flow_list`, `flow_get`, `flow_node_types`, `flow_save`, `flow_delete` e `flow_run`. Agentes de IA podem criar fluxos novos, editar os existentes e executá-los, e as mudanças aparecem na tela na hora.

## 1.3.1

### Correções
- Linha de comando e MCP: depois de **PDF → imagens** ou **PDF pesquisável (OCR)**, o `conver-cli` terminava com erro (0xC0000005) mesmo com o trabalho feito, em computadores com placa de vídeo AMD. A API de PDF do Windows usa a placa de vídeo, e o driver quebrava ao ser descarregado no fim do programa. Agora o Conver encerra sem passar por essa etapa, na linha de comando e ao fechar a janela.

## 1.3.0

### Novidades
- **Gravar a tela:** prévia ao vivo do que vai ser gravado (tela inteira, a janela escolhida, mesmo coberta por outras, ou a área), com o tamanho real em pixels e a marca REC durante a gravação.
- **Gravador de áudio:** faixa de áudio ao vivo e medidor de nível em dB com marca de pico. Os trechos que estouram ficam em vermelho, e uma frase avisa quando o som está baixo demais. O botão **Testar microfone** mostra o nível sem gravar nada, e a faixa da última gravação continua na tela depois de parar.

### Melhorias
- Visual de todas as telas: textos de apoio (dicas, subtítulos, detalhes dos arquivos) mais legíveis no tema claro e no escuro; campos de texto com a mesma altura dos botões; caixas de seleção maiores; a barra de progresso não mostra mais um ponto vermelho quando está parada; a dica da busca não fica mais cortada.

## 1.2.3

### Novidades
- **Busca de ferramentas** no topo do menu lateral (Ctrl+F): procura pelo nome, pela seção e por palavras que alguém usaria ("cortar", "aparar", "tirar fundo", "karaokê", "youtube", "senha"…), em português e inglês e no idioma escolhido, sem diferenciar acentos e aceitando um erro de digitação. Enter abre a primeira ferramenta encontrada e Esc limpa a busca.

## 1.2.2

### Correções
- MCP: o aviso de progresso de 100% era repetido várias vezes durante a etapa final de tarefas longas (por exemplo, quando o download junta áudio e vídeo). Agora ele chega uma vez só e o progresso nunca volta para trás.

## 1.2.1

### Novidades
- **Atualizar sem sair do app**: quando sai uma versão nova, o botão **Atualizar agora** (no aviso do topo ou em Configurações → Atualizações) baixa a atualização, confere o SHA-256 publicado na release, fecha o Conver, instala e abre de novo sozinho. Não é preciso baixar nada pelo navegador.
  - Instalado: o instalador novo roda em modo silencioso, na mesma pasta e com as mesmas opções.
  - Portátil: os arquivos da pasta são trocados e a pasta `dados` (configurações e histórico) é mantida.
  - Dá para cancelar durante o download; se a assinatura não conferir, nada é alterado.

## 1.2.0

### Novidades
- **Automação com agentes de IA (MCP)**: o `conver-cli mcp` é um servidor MCP com mais de 40 ferramentas (todas as do Conver), para Claude, Cursor, VS Code e outros agentes. Informa o progresso, aceita cancelamento e devolve os caminhos dos arquivos gerados. A configuração pronta para copiar fica em Configurações → Automação.
- **Linha de comando**: `conver-cli <ferramenta> arquivos… --opção valor` para scripts e arquivos .bat (curingas como `*.jpg`, saída em JSON com `--json`, `conver-cli help <ferramenta>`).
- **Predefinições**: salve configurações favoritas e use com um clique, pela linha de comando ou pelo MCP. Já vêm prontas: Instagram (1080 px), Fotos para a web, WhatsApp (vídeo até 16 MB), Discord (vídeo até 10 MB), Podcast (MP3 mono) e Legendas automáticas.
- **Ampliar com IA** (2× ou 4×, Real-ESRGAN), sem internet.
- **Transcrever / legendar** com o Whisper (whisper.cpp), sem internet: texto (.txt) e legendas (.srt/.vtt), detecção do idioma e tradução para o inglês. Três modelos: rápido (82 MB), equilibrado (264 MB) e preciso (574 MB), baixados na primeira vez.
- **Separar voz e instrumental** com IA (UVR MDX-Net): karaokê ou só a voz, em WAV, FLAC ou MP3.
- **Gravar a tela**: tela inteira, uma janela ou uma área, em MP4, com microfone opcional.
- **PDF**: comprimir (recomprime as imagens de dentro), PDF → imagens (PNG/JPG, 96 a 300 dpi), proteger com senha (AES-128, com permissões de impressão e cópia), tirar a senha e **PDF pesquisável** (OCR do Windows numa camada de texto invisível).

### Melhorias
- Modelos de IA baixados sob demanda, sempre conferindo o SHA-256.
- Os textos novos estão traduzidos para os 20 idiomas.

## 1.1.0

### Novidades
- **Remover fundo** (Imagem → Remover fundo): recorta pessoas, produtos e objetos com um modelo de IA que roda no próprio computador, sem internet e sem enviar a foto para lugar nenhum (cerca de 1 segundo por imagem).
  - Fundo transparente (salva em PNG; WEBP continua WEBP) ou de uma cor à sua escolha (mantém o formato original).
  - Borda **definida** ou **suave** (melhor para cabelo e contornos finos).
  - Prévia com fundo quadriculado para ver a transparência e processamento em lote, como nas outras ferramentas.
- **20 idiomas** (Configurações → Idioma): português, inglês, chinês, espanhol, árabe, francês, russo, indonésio, urdu, alemão, japonês, vietnamita, turco, hauçá, coreano, italiano, persa, suaíli, polonês e ucraniano.
  - Na primeira abertura o Conver usa o idioma do Windows.
  - Árabe, urdu e persa são escritos da direita para a esquerda, com as letras unidas.
  - Fontes do Windows para chinês, japonês e coreano carregadas automaticamente.

### Melhorias
- O modelo do removedor de fundo e o ONNX Runtime já vêm no instalador e na versão portátil.
- Mensagens de resultado e de erro também são traduzidas (nomes de arquivos e números ficam como estão).

### Correções
- O teste do gravador não falha mais em computadores sem microfone com som.

## 1.0.0

Primeira versão pública: imagens, vídeos, áudio, PDF, downloads (yt-dlp), OCR, informações do arquivo, gravador de áudio, menu do botão direito, verificação de atualizações, versão portátil e instalador.
