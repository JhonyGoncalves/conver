# Lista de mudanças

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
