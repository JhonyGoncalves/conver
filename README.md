<p align="center">
  <img src="assets/logo.png" width="96" alt="Logo do Conver">
</p>

<h1 align="center">Conver</h1>

<p align="center">
  <b>Conversor universal para Windows</b> — imagens, vídeos, áudios, PDFs e downloads num só programa, rápido e sem anúncios.
</p>

<p align="center">
  <a href="https://github.com/JhonyGoncalves/conver/releases/latest"><b>⬇ Baixar a versão mais recente</b></a>
  &nbsp;•&nbsp;
  <a href="#apoie-o-projeto">💚 Apoiar com Pix</a>
</p>

![Tela do Conver lendo o texto de uma imagem](docs/ocr.png)

## Download

Na página de [releases](https://github.com/JhonyGoncalves/conver/releases/latest) há duas opções:

| Arquivo | Para quem |
|---|---|
| `Conver-x.y.z-instalador.exe` | Instala o Conver no seu usuário (não pede administrador), cria atalhos e pode adicionar **"Abrir com Conver"** ao menu do botão direito. |
| `Conver-x.y.z-portatil.zip` | Não instala nada: extraia em qualquer pasta (ou pendrive) e abra o `Conver.exe`. As configurações ficam na própria pasta. |

Requisitos: Windows 10 ou 11, 64 bits. O FFmpeg, o yt-dlp e o modelo do removedor de fundo já vêm junto — não é preciso instalar mais nada.

> **Aviso do Windows SmartScreen:** como o Conver é novo e não tem assinatura digital paga, o Windows pode mostrar "O Windows protegeu o computador". Clique em **Mais informações → Executar assim mesmo**. Os arquivos de cada versão têm a soma SHA-256 publicada na release, para você conferir.

## O que o Conver faz

**Imagem**
- **Remover fundo** com IA, no próprio computador e sem internet: fundo transparente (PNG) ou de uma cor, borda definida ou suave
- Redimensionar (porcentagem ou pixels), comprimir, diminuir qualidade e converter entre PNG, JPG, WEBP, BMP, GIF, TIFF e ICO
- Girar, espelhar e recortar (1:1, 4:3, 16:9, 9:16…)
- Ajustes de cor: brilho, contraste, saturação, preto e branco, sépia
- Remover metadados (EXIF, localização GPS) sem perder qualidade
- Juntar imagens num PDF
- **Texto de imagem (OCR)** com o reconhecimento do próprio Windows, sem internet — inclusive colando um print (Win+Shift+S)
- Prévia do resultado antes de salvar

**Vídeo**
- Converter (MP4, MKV, AVI, MOV, WEBM, FLV, WMV, GIF, MP3) e comprimir (por nível ou pelo tamanho final em MB)
- Cortar trecho, juntar vídeos, mudar velocidade (0,25× a 4×)
- Girar e recortar para 9:16 (Reels/TikTok), 1:1, 4:5 e 16:9
- Vídeo → GIF de um trecho, queimar legenda (.srt/.ass/.vtt), música de fundo
- Remover o áudio ou extrair frames

**Áudio**
- Gravador de áudio (microfone) em MP3, WAV ou M4A
- Converter (MP3, WAV, FLAC, OGG, M4A, OPUS), cortar, juntar, normalizar volume e comprimir

**PDF**
- Juntar PDFs e dividir (páginas como `1-3, 7` ou uma página por arquivo)

**Downloads**
- YouTube, YouTube Music, TikTok, Instagram, X/Twitter, Facebook, Dailymotion, SoundCloud e centenas de outros sites (via [yt-dlp](https://github.com/yt-dlp/yt-dlp))
- MP4 ou MP3 com escolha de qualidade, playlists, vários links de uma vez, legendas e capa
- Login do navegador para sites que pedem conta
- Botão para atualizar o yt-dlp quando algum site mudar

**Ferramentas**
- Informações do arquivo (resolução, duração, codecs, câmera, GPS…)
- Pasta vigiada: converte automaticamente tudo que cair numa pasta
- Renomear em lote (com prévia e desfazer)
- Gerador de QR Code
- Histórico de conversões e downloads

**No dia a dia**
- Arraste arquivos para a janela (ou para o ícone do Conver) e eles vão para a ferramenta certa
- "Abrir com Conver" no menu do botão direito
- **20 idiomas** (Configurações → Idioma): português, inglês, chinês, espanhol, árabe, francês, russo, indonésio, urdu, alemão, japonês, vietnamita, turco, hauçá, coreano, italiano, persa, suaíli, polonês e ucraniano
- Tema claro e escuro, aviso com som e notificação ao terminar tarefas longas
- Avisa quando sai uma versão nova

![Configurações do Conver](docs/configuracoes.png)

## Privacidade

Tudo é feito no seu computador. O Conver só acessa a internet quando você usa os downloads, quando atualiza o yt-dlp e, se a opção estiver ligada, para ver se há uma versão nova (uma consulta à API do GitHub). Não há telemetria.

Use os downloads apenas para conteúdos que você tem direito de baixar.

## Compilar a partir do código

O Conver é escrito em [Rust](https://www.rust-lang.org/) com a interface [egui](https://github.com/emilk/egui).

```powershell
# 1. ferramentas
winget install Rustlang.Rustup Gyan.FFmpeg JRSoftware.InnoSetup
# 2. coloque em Conver\bin o yt-dlp.exe (https://github.com/yt-dlp/yt-dlp/releases) e o onnxruntime.dll
#    (pasta lib do onnxruntime-win-x64-1.22.0.zip, https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0)
#    e em Conver\modelos o silueta.onnx (https://github.com/danielgatis/rembg/releases/download/v0.0.0/silueta.onnx)
# 3. compile e gere a versão portátil e o instalador em dist\
powershell -ExecutionPolicy Bypass -File build.ps1
```

Para só rodar durante o desenvolvimento: `cargo run --release` (com `ffmpeg.exe` e `yt-dlp.exe` no PATH). Os testes ficam em `src/tests*.rs`: `cargo test --release -- --test-threads 1`.

### Estrutura

| Arquivo | O que tem |
|---|---|
| `src/main.rs` | Janela, menu lateral, configurações |
| `src/pages.rs`, `media_page.rs`, `extras*.rs` | As telas de cada ferramenta |
| `src/imaging.rs` | Imagens (redimensionar, comprimir, metadados, PDF…) |
| `src/video.rs`, `media.rs`, `media_extra.rs` | Vídeo e áudio com o FFmpeg |
| `src/download.rs` | Downloads com o yt-dlp |
| `src/pdf.rs`, `ocr.rs`, `info.rs`, `recorder.rs` | PDF, OCR, informações e gravador |
| `src/bgremove.rs` | Removedor de fundo (modelo ONNX) |
| `src/i18n.rs`, `i18n/*.txt` | Idiomas: o texto em português do código é a chave; cada idioma tem linhas `número\|tradução` alinhadas com `i18n/keys.txt` |
| `src/integration.rs` | Menu do botão direito, atualizações, instância única |
| `installer/conver.iss` | Script do instalador (Inno Setup) |

## Apoie o projeto

O Conver é gratuito, sem anúncios e sem telemetria. Se ele te ajudou e você quiser contribuir com o desenvolvimento, aceito doações de qualquer valor por **Pix**:

<p align="center">
  <img src="docs/qrcode-pix.png" width="220" alt="QR Code Pix para doação">
</p>

Também ajuda muito dar uma ⭐ no repositório, relatar problemas e sugerir ferramentas nas [issues](https://github.com/JhonyGoncalves/conver/issues).

## Licença

O código do Conver está sob a [licença MIT](LICENSE). Os downloads incluem o FFmpeg (GPL v3), o yt-dlp (Unlicense), o ONNX Runtime (MIT) e o modelo silueta (MIT/Apache 2.0) como componentes separados — veja [THIRD-PARTY.md](THIRD-PARTY.md).
