# Lista de mudanças

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
