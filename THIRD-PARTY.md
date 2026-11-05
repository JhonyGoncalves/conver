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

## whisper.cpp — MIT

- Arquivos: `bin\whisper\whisper-cli.exe`, `whisper.dll`, `ggml*.dll`
- Versão: 1.9.2, compilação oficial (<https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.2>)
- Licença: MIT — texto em `licencas\whisper.cpp-LICENSE.txt`
- Os modelos do Whisper (OpenAI, licença MIT) não vêm no download: são baixados de <https://huggingface.co/ggerganov/whisper.cpp> na primeira transcrição, conferindo o SHA-256.

## Modelo Real-ESRGAN "general x4v3" (ampliar com IA)

- Arquivo: `modelos\realesr-general-x4v3.onnx` (SHA-256 `3439a9e4cd26c4cdfff28f82f00411a7a4f06e7f72a1efdc664479f1e2bf0690`)
- Origem: `realesr-general-x4v3.pth` oficial do [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0) (Xintao Wang, licença BSD-3 — texto em `licencas\Real-ESRGAN-LICENSE.txt`), convertido para ONNX sem alterações nos pesos.

## Modelo UVR MDX-Net "Inst HQ 3" (separar voz e instrumental)

- Não vem no download: é baixado de <https://github.com/TRvlvr/model_repo/releases/tag/all_public_uvr_models> no primeiro uso, conferindo o SHA-256 (`317554b07fe1ea5279a77f2b1520a41ea4b93432560c4ffd08792c30fddf9adc`).
- Projeto: [Ultimate Vocal Remover](https://github.com/Anjok07/ultimatevocalremovergui) (licença MIT).

## whisper-server (ditado ao vivo)

- Arquivo: `bin\whisper\whisper-server.exe`, do mesmo pacote oficial do whisper.cpp 1.9.2 (MIT). Roda só no próprio computador (127.0.0.1).

## Modelo RNNoise "somnolent-hogwash" (remover ruído)

- Arquivo: `modelos\rnnoise-sh.rnnn` (SHA-256 `70bb6685eb0c2a1d18e2918dca3fbfbd39317010b1802eb1b6ea73a92f3fdec0`)
- Origem: [GregorR/rnnoise-models](https://github.com/GregorR/rnnoise-models) (commit `3eee541`). Segundo o autor, os modelos não são obra criativa e não estão sujeitos a direitos autorais. O filtro que usa o modelo (`arnndn`) faz parte do FFmpeg.

## Piper — MIT (texto para voz)

- Arquivos: `bin\piper\` (`piper.exe`, `piper_phonemize.dll`, `espeak-ng.dll`, `espeak-ng-data`, `onnxruntime.dll` 1.14.1, `libtashkeel_model.ort`)
- Versão: 2023.11.14-2, compilação oficial (<https://github.com/rhasspy/piper/releases/tag/2023.11.14-2>)
- Licenças: Piper e piper-phonemize — MIT (`licencas\piper-LICENSE.txt`, `licencas\piper-phonemize-LICENSE.txt`); eSpeak NG — GPL v3 (`licencas\espeak-ng-COPYING.txt`, código em <https://github.com/espeak-ng/espeak-ng>); ONNX Runtime 1.14.1 — MIT (`licencas\onnxruntime-1.14-LICENSE.txt`).
- O Conver só executa o `piper.exe` como programa separado.
- As vozes não vêm no download: são baixadas de <https://huggingface.co/rhasspy/piper-voices> (revisão `e7229f7`, licença MIT; cada voz traz o seu cartão com a origem dos dados) no primeiro uso, conferindo o SHA-256.

## stable-diffusion.cpp — MIT (gerar imagem)

- Arquivos: `bin\sd\` (`sd-cli.exe`, `stable-diffusion.dll`, `ggml*.dll` com Vulkan, `libwebp*.dll`)
- Versão: master-1050-58bb3c1, compilação oficial Vulkan (<https://github.com/leejet/stable-diffusion.cpp/releases/tag/master-1050-58bb3c1>)
- Licenças: MIT — `licencas\stable-diffusion.cpp-LICENSE.txt` e `licencas\ggml-LICENSE.txt`; libwebp — BSD.
- O modelo não vem no download: **DreamShaper 8 LCM** (Lykon, <https://huggingface.co/Lykon/dreamshaper-8-lcm>, baseado no Stable Diffusion 1.5) é baixado no primeiro uso, conferindo o SHA-256 (`a4f3e1526c5dc4fcbe342f5c410d83ae202c7a415fcefcbb92e0f93fcd0a87c3`). Licença **CreativeML OpenRAIL-M**: o uso é livre, inclusive comercial, mas proíbe usos nocivos listados na licença (<https://huggingface.co/spaces/CompVis/stable-diffusion-license>); quem gera as imagens é responsável por elas.

## Modelo LaMa (apagar objeto)

- Não vem no download: `lama_fp32.onnx` de <https://huggingface.co/Carve/LaMa-ONNX> (licença Apache 2.0, a partir do [LaMa](https://github.com/advimman/lama), Apache 2.0) é baixado no primeiro uso, conferindo o SHA-256 (`1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6`).

## Bibliotecas Rust

O Conver usa bibliotecas Rust de código aberto (egui/eframe, image, lopdf, ort, realfft, sha2, ar-reshaper, unicode-bidi, rfd, serde, ureq, windows-rs e outras), quase todas sob MIT e/ou Apache 2.0. A lista completa, com versões, está no arquivo `Cargo.lock` do repositório.
