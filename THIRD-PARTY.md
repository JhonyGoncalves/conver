# Programas de terceiros incluídos no Conver

O código do Conver está sob a [licença MIT](LICENSE). Os downloads (versão portátil e instalador) também trazem programas e arquivos de terceiros, nas pastas `bin` e `modelos`, cada um com a sua própria licença:

## FFmpeg — GPL v3

- Arquivo: `bin\ffmpeg.exe`
- Versão: 7.1.1, compilação "full" de [gyan.dev](https://www.gyan.dev/ffmpeg/builds/)
- Licença: GNU General Public License v3 — texto completo em `licencas\ffmpeg-LICENSE.txt`
- Código-fonte: <https://github.com/FFmpeg/FFmpeg/commit/db69d06eee> (as bibliotecas externas e as opções de compilação estão em `licencas\ffmpeg-README.txt`)

O Conver não é derivado do FFmpeg: ele apenas executa o `ffmpeg.exe` como um programa separado. Você pode trocar esse arquivo por qualquer outra compilação do FFmpeg.

## yt-dlp — Unlicense (domínio público)

- Arquivo: `bin\yt-dlp.exe`
- Projeto: <https://github.com/yt-dlp/yt-dlp>
- Licença: [The Unlicense](https://github.com/yt-dlp/yt-dlp/blob/master/LICENSE)
- O executável incluído foi empacotado com PyInstaller a partir do pacote oficial do yt-dlp e de suas dependências (Python — PSF License; curl_cffi — MIT; yt-dlp-ejs — Unlicense; certifi — MPL 2.0; requests, urllib3, websockets, brotli, mutagen, pycryptodomex — licenças permissivas). O botão "Atualizar yt-dlp" do Conver troca esse arquivo pelo executável oficial publicado pelo próprio projeto.

## ONNX Runtime — MIT

- Arquivo: `bin\onnxruntime.dll`
- Versão: 1.22.0, compilação oficial da Microsoft (<https://github.com/microsoft/onnxruntime/releases/tag/v1.22.0>)
- Licença: MIT — texto em `licencas\onnxruntime-LICENSE.txt`; avisos dos componentes em `licencas\onnxruntime-ThirdPartyNotices.txt`
- Usado pelo removedor de fundo para rodar o modelo de IA no próprio computador.

## Modelo "silueta" (removedor de fundo)

- Arquivo: `modelos\silueta.onnx` (SHA-256 `75da6c8d2f8096ec743d071951be73b4a8bc7b3e51d9a6625d63644f90ffeedb`)
- Origem: projeto [rembg](https://github.com/danielgatis/rembg) (licença MIT), versão reduzida do [U²-Net](https://github.com/xuebinqin/U-2-Net) (licença Apache 2.0)

## Bibliotecas Rust

O Conver usa bibliotecas Rust de código aberto (egui/eframe, image, lopdf, ort, ar-reshaper, unicode-bidi, rfd, serde, ureq, windows-rs e outras), quase todas sob MIT e/ou Apache 2.0. A lista completa, com versões, está no arquivo `Cargo.lock` do repositório.
