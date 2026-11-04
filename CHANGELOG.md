# Lista de mudanças

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
