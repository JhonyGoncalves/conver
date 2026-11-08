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
- Também opcionais, baixados só se escolhidos em Gerar imagem ou em Modelos de IA (SHA-256 conferido):
  - **DreamShaper XL Lightning** (Lykon, <https://huggingface.co/Lykon/dreamshaper-xl-lightning>, baseado no SDXL),
    `DreamShaperXL_Lightning.safetensors` (`fdbe56354b8f876b736f24d3ad867ecd4140c019f57642ec8ddd878088d44f64`). Licença
    **CreativeML OpenRAIL++-M**: uso livre, inclusive comercial, com as mesmas restrições de uso nocivo.
  - **Z-Image Turbo** (Tongyi-MAI/Alibaba, <https://huggingface.co/Tongyi-MAI/Z-Image-Turbo>, licença **Apache 2.0**), na
    conversão GGUF de <https://huggingface.co/leejet/Z-Image-Turbo-GGUF>: `z_image_turbo-Q4_K.gguf`
    (`14b375ab4f226bc5378f68f37e899ef3c2242b8541e61e2bc1aff40976086fbd`); com o leitor de texto **Qwen3 4B Instruct 2507**
    (Qwen/Alibaba, Apache 2.0; GGUF de <https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF>,
    `3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597`) e o VAE do **FLUX.1 schnell** (Black Forest Labs,
    Apache 2.0; cópia de <https://huggingface.co/Comfy-Org/Z-Image-Turbo>,
    `afc8e28272cd15db3919bacdb6918ce9c1ed22e96cb12c4d5ed0fba823529e38`).
  - **FLUX.2 klein 4B** (Black Forest Labs, <https://huggingface.co/black-forest-labs/FLUX.2-klein-4B>, licença
    **Apache 2.0**), para editar imagens por instrução: GGUF de <https://huggingface.co/leejet/FLUX.2-klein-4B-GGUF>,
    `flux-2-klein-4b-Q4_0.gguf` (`d1023499ef3f2f82ff7c50e6778495195c1b6cc34835741778868428111f9ff4`), e VAE de
    <https://huggingface.co/Comfy-Org/flux2-klein-4B>, `flux2-vae.safetensors`
    (`868fe7b343cc8f3a19dbcfcafbc3d5f888802be3f89bd81b65b3621a066ce8f3`).

## Remover objeto com um clique

- Não vem no download: **SlimSAM** (Chen et al., <https://github.com/czg1225/SlimSAM>, licença **Apache 2.0**), na
  conversão ONNX de <https://huggingface.co/Xenova/slimsam-50-uniform>: `vision_encoder.onnx`
  (`4ffb40bdee86cbfb7ec96ecefc6f0cdeba3f1f160a8f091084cdb1cde3ac7fdd`) e `prompt_encoder_mask_decoder.onnx`
  (`f4514391764fbd56e08e119060d874ecd7d52994bfb1968af159e12d4943b5bb`), baixados no primeiro uso (SHA-256 conferido).

## Vozes Kokoro

- Não vêm no download: **Kokoro 82M v1.0** (hexgrad, <https://huggingface.co/hexgrad/Kokoro-82M>, licença **Apache 2.0**),
  na conversão ONNX de <https://huggingface.co/onnx-community/Kokoro-82M-v1.0-ONNX>: `model_quantized.onnx`
  (`fbae9257e1e05ffc727e951ef9b9c98418e6d79f1c9b6b13bd59f5c9028a1478`) e um arquivo por voz, baixados no primeiro uso
  (SHA-256 conferido). A pronúncia vem do `espeak-ng.dll` que já acompanha o Piper (GPLv3).

## Modelo LaMa (apagar objeto)

- Não vem no download: `lama_fp32.onnx` de <https://huggingface.co/Carve/LaMa-ONNX> (licença Apache 2.0, a partir do [LaMa](https://github.com/advimman/lama), Apache 2.0) é baixado no primeiro uso, conferindo o SHA-256 (`1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6`).

## Modelo SigLIP 2 (classificar imagens)

- Não vem no download: o SigLIP 2 base (patch16, 224) do Google ([google/siglip2-base-patch16-224](https://huggingface.co/google/siglip2-base-patch16-224), licença Apache 2.0), na conversão ONNX de <https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX> (revisão `ba1f3b0`), é baixado no primeiro uso, conferindo o SHA-256: `vision_model_q4.onnx` (`03e61be2…`), `text_model_int8.onnx` (`3a0603d3…`) e `tokenizer.json` (`cb9140fa…`).

## llama.cpp — MIT (IA local de linguagem: traduzir legendas, classificar textos)

- Arquivos: `bin\llm\` (`llama-server.exe`, `llama-server-impl.dll`, `llama.dll`, `llama-common.dll`, `mtmd.dll`, `ggml*.dll` com Vulkan, `libomp.dll`)
- Versão: b12121, compilação oficial Vulkan (<https://github.com/ggml-org/llama.cpp/releases/tag/b12121>), sem as outras ferramentas do pacote
- Licenças: MIT — `licencas\llama.cpp-LICENSE.txt`; LLVM OpenMP (`libomp.dll`) — Apache 2.0 com exceções LLVM, `licencas\llvm-openmp-LICENSE.txt`.
- O Conver só executa o `llama-server.exe` como programa separado, em 127.0.0.1, enquanto um trabalho precisa dele.

## Modelo Qwen 3.5 2B (IA local de linguagem)

- Não vem no download: `Qwen3.5-2B-Q4_K_M.gguf`, conversão GGUF de <https://huggingface.co/unsloth/Qwen3.5-2B-GGUF> (revisão `f6d5376`) do [Qwen/Qwen3.5-2B](https://huggingface.co/Qwen/Qwen3.5-2B) (licença Apache 2.0), é baixado no primeiro uso, conferindo o SHA-256 (`aaf42c8b…`).

## Modelo YuNet (desfocar rostos) — MIT

- Arquivo: `modelos\face_detection_yunet_2023mar.onnx` (SHA-256 `8f2383e4…`), do [OpenCV Zoo](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet) (<https://huggingface.co/opencv/face_detection_yunet>, revisão `3cc26e7`).
- Licença: MIT — `licencas\yunet-LICENSE.txt`.

## Modelo GFPGAN 1.4 (restaurar foto)

- Não vem no download: `GFPGANv1.4.onnx` de <https://huggingface.co/Neus/GFPGANv1.4> (revisão `944e127`), conversão ONNX do [GFPGAN](https://github.com/TencentARC/GFPGAN) da Tencent ARC (licença Apache 2.0), é baixado no primeiro uso, conferindo o SHA-256 (`cd7311b8…`).

## Modelo DDColor (colorir foto)

- Não vem no download: `ddcolor-onnx-float.zip` da exportação ONNX do [Qualcomm AI Hub](https://github.com/quic/ai-hub-models) (versão 0.66.0) do [DDColor](https://github.com/piddnad/DDColor) (licença Apache 2.0), é baixado no primeiro uso, conferindo o SHA-256 (`512e788c…`), e extraído em `modelos\ddcolor\`.

## Modelo CLAP (classificar sons)

- Não vem no download: o CLAP HTSAT "unfused" do LAION ([laion/clap-htsat-unfused](https://huggingface.co/laion/clap-htsat-unfused), licença Apache 2.0), na conversão ONNX de <https://huggingface.co/Xenova/clap-htsat-unfused> (revisão `c28f288`), é baixado no primeiro uso, conferindo o SHA-256: `audio_model.onnx` (`a1c2b43c…`), `text_model_quantized.onnx` (`1a3df8b1…`) e `tokenizer.json` (`dc239041…`).

## Bibliotecas Rust

O Conver usa bibliotecas Rust de código aberto (egui/eframe, image, lopdf, ort, realfft, rust_xlsxwriter, sha2, ar-reshaper, unicode-bidi, rfd, serde, ureq, windows-rs e outras), quase todas sob MIT e/ou Apache 2.0. A lista completa, com versões, está no arquivo `Cargo.lock` do repositório.
