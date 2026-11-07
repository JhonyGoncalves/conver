# Conver

Conversor universal para Windows (imagens, vídeo, áudio, PDF, downloads, OCR, remover fundo), em Rust com interface egui.
Repositório público: https://github.com/JhonyGoncalves/conver (licença MIT). O código, os comentários, a interface e
as mensagens de commit são em **português**.

## Comandos

```powershell
cargo run --release                                   # rodar (precisa das ferramentas de Conver\bin, veja abaixo)
cargo test --release -- --nocapture --test-threads 1  # todos os testes (sempre com 1 thread: usam porta TCP, registro, áudio)
cargo test --release -- --nocapture remove_background # um teste só
powershell -ExecutionPolicy Bypass -File build.ps1 -Repo JhonyGoncalves/conver   # gera dist\ (portátil, instalador, SHA256)
```

Antes dos testes, ponha `Conver\bin` no PATH na mesma sessão: `$env:PATH = "$PWD\Conver\bin;$env:PATH"`.

## Pasta `Conver\` (fora do git)

Tem tudo o que vai junto no download e é usada também pelo `cargo run`/testes (achada via `CARGO_MANIFEST_DIR`):
- `bin\ffmpeg.exe` (7.1.1, gyan full, GPLv3), `bin\yt-dlp.exe`, `bin\onnxruntime.dll` (1.22.0 oficial da Microsoft),
  `bin\whisper\` (whisper-cli.exe, whisper-server.exe + DLLs do whisper.cpp 1.9.2),
  `bin\piper\` (Piper 2023.11.14-2 com espeak-ng-data e o próprio onnxruntime 1.14.1), `bin\sd\` (sd-cli.exe +
  DLLs Vulkan do stable-diffusion.cpp master-1050-58bb3c1)
- `modelos\silueta.onnx` (remover fundo), `modelos\realesr-general-x4v3.onnx` (ampliar) e `modelos\rnnoise-sh.rnnn`
  (remover ruído, pelo `arnndn` do ffmpeg)
- `conver-cli.exe` (copiado pelo `build.ps1` de `target\release`)
- Modelos grandes (Whisper, UVR, vozes do Piper, DreamShaper 8 LCM, LaMa, SigLIP, CLAP, Qwen) não vão no download: são baixados
  para `%APPDATA%\Conver\modelos` no primeiro uso ou pela tela de boas-vindas / Configurações (`models.rs`). O llama.cpp
  (`bin\llm`) também não vai: o `build.ps1` monta o pacote sem ele, e `llm::ensure_runtime` baixa o zip oficial para
  `%APPDATA%\Conver\ferramentas\llm`; a cópia em `Conver\bin\llm` serve só ao desenvolvimento e aos testes.

`tools::find_tool` / `tools::find_tool_file` procuram em `exe_dir\bin`, `exe_dir` e `CARGO_MANIFEST_DIR\Conver\bin`.
Se existir `portatil.txt` ao lado do exe, os dados vão para `dados\` em vez de `%APPDATA%\Conver` (`tools::app_dir`).

## Arquitetura

- `src/main.rs`: janela, menu lateral (`NAV`), `Config` (serde, salva em `config.json`) e tela de Configurações.
- Telas: `pages.rs` (imagem, `ImgTool`, `IMG_TOOLS`), `media_page.rs` (vídeo/áudio), `extras*.rs` (demais ferramentas).
- Lógica: `imaging.rs` (`ImageOp`, `process_and_save`), `video.rs`/`media*.rs` (FFmpeg como processo separado),
  `download.rs` (yt-dlp), `pdf.rs` (lopdf), `ocr.rs` (Windows.Media.Ocr), `bgremove.rs` (ort), `recorder.rs`,
  `info.rs`, `qr.rs`, `rename.rs`, `watch.rs`, `history.rs`, `notify.rs`.
- `job.rs`: trabalhos em segundo plano com progresso e cancelamento. `widgets.rs`/`theme.rs`: componentes e paleta
  (linho #e8ebe4, carvão #494947, cinábrio #f13030; tema claro e escuro).
- `integration.rs`: menu do botão direito (registro do usuário), verificação de atualizações (API do GitHub),
  instância única (TCP 127.0.0.1:47815, protocolo `CONVER/1`; `PathBuf` vazio = só focar a janela).
- `ops.rs`: **núcleo de todas as ferramentas** como operações com argumentos JSON e JSON Schema (nomes e descrições
  em inglês, para agentes). Usado pela linha de comando, pelo MCP e pelas predefinições. Ferramenta nova = função
  `t_*` + entrada em `TOOLS` (e a tela, se tiver).
- `cli.rs`: modo `--cli` do Conver.exe — linha de comando e servidor MCP (`mcp`: JSON-RPC por stdio, uma thread por
  chamada, progresso e cancelamento). `src/bin/conver-cli.rs` é um executável de console mínimo que repassa os
  argumentos e o terminal para `Conver.exe --cli` (o Conver.exe é de janela e o Windows não esperaria por ele).
- `updater.rs`: atualização de dentro do app. Instalado (tem `unins000.exe`): baixa o instalador, confere com o
  `-SHA256.txt` da release e roda `/VERYSILENT ... /RELANCAR=1` (o `[Code]` do `conver.iss` reabre o app). Portátil:
  baixa o zip, extrai com o `tar.exe` do Windows e roda o Conver.exe novo com `--aplicar-atualizacao`, que espera o
  antigo fechar e copia por cima sem tocar em `dados\`. Os nomes dos arquivos da release (`-instalador.exe`,
  `-portatil.zip`, `-SHA256.txt`) fazem parte do contrato: não mude no `build.ps1`.
- `presets.rs`: predefinições (ferramenta + argumentos) em `predefinicoes.json`. Telas novas em `extras4.rs`.
- Fluxos por nodes: `flow.rs` (formato, catálogo de tipos de node, validação e execução; cada node de ferramenta
  roda em `fluxos/execucoes/<execução>/<node>` e tudo o que ele criar ali passa adiante), `flows_page.rs` (editor:
  quadro, ajustes do node, execução), `ai_api.rs` (Anthropic ou compatível com OpenAI; chave protegida por DPAPI
  em `ia.json`). Os fluxos ficam em `fluxos/<nome>.json` e o MCP usa os mesmos arquivos (`flow_*` em `ops.rs`);
  a tela recarrega quando o arquivo muda por fora. Ferramenta nova em `ops::TOOLS` vira node sozinha.
- IA: `ai.rs` (ambiente ONNX compartilhado e download de modelos com SHA-256), `upscale.rs` (Real-ESRGAN em blocos),
  `separate.rs` (UVR MDX-Net com STFT própria via realfft), `transcribe.rs` (whisper-cli em `bin\whisper`; também o
  tempo de cada palavra, `word_timestamps`).
- IA 1.5: `captions.rs` (legendas: frases `Cue` editáveis, `Style` com predefinições, .ass com `\pos` e, se animada,
  uma linha por palavra; `render_preview` desenha um quadro com o próprio ffmpeg; tela em `captions_page.rs`,
  estilo salvo em `legendas.json`; op `video_captions`, alias `video_animated_captions`),
  `dictation.rs` (ditado ao vivo: sobe o `whisper-server` numa porta livre e manda trechos do microfone por HTTP),
  `tts.rs` (Piper como processo, `VOICES` com revisão fixa do Hugging Face), `imagegen.rs` (`sd-cli` com
  `--sampling-method lcm`, Vulkan), `inpaint.rs` (LaMa em 512×512 recortado em volta da máscara). Telas em `extras5.rs`.
- `classify.rs`: classificação local sem exemplos (SigLIP 2 base 224, ONNX da onnx-community: imagem em q4 — o int8
  usa ConvInteger, que o ort não roda na CPU —, texto em int8 e `tokenizer.json`; BPE do Gemma implementado ali).
  Probabilidade = softmax de cosseno × 112,66 − 16,77 entre as opções. Node `classify` em `flow.rs`: as saídas são as
  opções + `uncertain` (`flow::node_outputs`, que o editor e a validação usam); op `classify` (alias `image_classify`).
  `Classifier` escolhe o que olhar (`look_at`): imagem (SigLIP), som (`audioclass.rs`, CLAP com mel Slaney e BPE do
  RoBERTa próprios; opções em português passam por um dicionário de sons) ou texto (`llm.rs`).
- `models.rs`: catálogo dos modelos em pacotes (`Pack`: o que faz, quem usa, tamanho, baixar, apagar), finalidades
  (`Purpose`) que sugerem pacotes e a fila de downloads (`Downloads`). `welcome.rs`: boas-vindas na primeira vez
  (`Config.welcome_done`; quem já tinha `config.json` não vê); cenas de captura `boas_vindas` e `boas_vindas_modelos`.
- Interface: em árabe, urdu e persa o layout é espelhado (veja "A Regra do Espelho" no DESIGN.md: `widgets::down`,
  `fwd`, `back`, `vt`, `check`, `mirror`; nunca `Layout::left_to_right`/`right_to_left` direto). `widgets::segmented`
  vira menu suspenso quando não cabe. Botões e itens cheios usam `Palette::action` (#d42a24), não `accent`.
- `llm.rs`: llama-server (`bin\llm` ou baixado, llama.cpp b12121 Vulkan) com Qwen 3.5 2B GGUF, numa porta livre só durante o
  trabalho (cai para CPU se a GPU falhar); `classify` usa JSON Schema + logprobs; `translate` vai em blocos de 20.
  `translate.rs`: traduzir .srt e dublar (Piper `--json-input`, uma fala por arquivo, atempo até 1,5×).
- Leva 1.7: `faces.rs` (YuNet 640×640, decodificação como o FaceDetectorYN; vídeo por pipe rawvideo do ffmpeg),
  `watermark.rs` (ffmpeg drawtext/overlay; PDF com lopdf, ExtGState e páginas giradas), `pdf_tables.rs` (OCR do
  Windows → linhas/células/colunas → rust_xlsxwriter), `media::enhance_voice`, `triggers.rs` (pasta vigiada em thread
  no app; horário com `schtasks`, tarefa `Conver\Fluxo - <nome>` chamando `Conver.exe --cli flow_run`). Telas em
  `extras6.rs`. Fluxos: `Item.tag` (opção do classificador) e `Item.origin` (arquivo de entrada) seguem pelos nodes;
  cada execução grava `execucao.json` (histórico, `retry_failed`); node `rename`.
- Leva 1.8: `docscan.rs` (Escanear documento: maior área clara por Otsu, cantos por x±y, homografia 8×8 e limpeza
  dividindo pelo fundo; tela em `extras6.rs`, op `document_scan`). Fluxos: node `condition` (`flow::condition_passes`),
  ops `files_rename` e `audio_record`; no painel (`flows_page.rs`) `arg_editor` usa seletor segmentado/slider e
  `special_editor` (cor, arquivo, pasta, microfone); `arg_hidden` esconde ajustes que não valem; `node_results`
  mostra a saída da última execução (`execucao.json`); `import_flow` confere os tipos de node. Teste
  `flow_nodes_in_portuguese` exige rótulo, valor e ajuda em português para todo node. Captura: `CONVER_FLUXO_NODE`.
- `flow::run_tool`: com `music` vazio, o primeiro áudio que chega vira a música; ferramenta com `files` e `text` roda só com o texto.
- Leva 1.9: `imagegen.rs` com `Engine` (Rápido, SDXL Lightning, Z-Image Turbo; Z-Image precisa de `--flash-attn on` em
  8 GB) e `edit` (`EditMode`: instrução com FLUX.2 klein `-r`, pintar com `--mask`, variar com `--strength`; imagem e
  máscara redimensionadas para múltiplos de 16); tela `editpage.rs`, op `image_edit` (o argumento é `how`, não `mode`).
  `bgremove.rs`: `Refine` (precisão, grow, feather, antialias), `Strokes` e `Dot` (traços em proporção da imagem); tela
  `bgpage.rs` (etapa pesada numa thread, ajustes e pincel na hora). `kokoro.rs`: Kokoro 82M no `ort` + fonemas do
  `espeak-ng.dll` do Piper por FFI (`LoadLibraryW`, IPA com "^" e a troca do misaki); o sherpa-onnx foi descartado (a 2.0
  não tem português e estraga UTF-8 no argv do Windows). `tts::AnyVoice` une Piper e Kokoro (Kokoro primeiro).
  `Job::done_folder` aceita arquivo: o botão vira "Mostrar arquivo".
- Recorte: `bgremove::cut` junta tudo. `Detect` escolhe IA ou cor do fundo (`flat_background`: borda 97% igual); a máscara por
  cor (`color_mask`) tem o formato da da IA, e por isso precisão e borda valem nos dois. `Interior`/`fill_inside` fecham o
  que está dentro do contorno, atravessando vãos de 0,35% do lado; `clean_edge` tira o halo. `Detect::Nada` = só sai o que foi
  marcado. `segment.rs`: SlimSAM (`Strokes.cut`, "Remover objeto": um clique, sai sem preencher; o decodificador leva
  50 ms e o codificador guarda a última imagem). A LaMa continua em "Apagar e preencher".
- Cortar: `media::plan_cuts` decide as partes (`Cut`: manter ou tirar; o que é tirado sai de tudo; sem `together`, um arquivo por trecho
  para manter, mesmo sobrepostos), `media::cut` grava. `trim_page.rs`: linha do tempo; captura com `CONVER_CORTES=m:0:10-0:40;t:0:20-0:30`.
- `cut_preset.rs`: arquivo JSON de cortes (`cuts`, `output`, `precise`; campo desconhecido é erro de propósito). `<vídeo>.cortes.json` ao
  lado é lido sozinho (só esse nome: um `.json` genérico pode ser o info.json do yt-dlp). Op: `cuts_file` (caminho ou `auto`).
- `smartcut.rs`: corte Inteligente (`CutMode::Inteligente`): pontas recodificadas e miolo copiado, em MPEG-TS, emendados e conferidos nas
  emendas (`verify`); se falhar, `media::cut` refaz no Preciso e diz no resultado. Só H.264 8 bits sem rotação em MP4/MOV/MKV. Armadilhas:
  `-discard nokey` erra os tempos em até 2 quadros (serve só para a tela; para cortar use `-skip_frame nokey`); `-ss` em vídeo com quadros B
  recua 3/23 s, daí `SEEK_SLACK`; `-to` com `-c copy` para pelo dts, daí `packets_between` + `-frames:v`.
- Interface 2.0 (`ui2.rs`, o padrão): `Config.interface` (2 = nova, 1 = clássica; `CONVER_INTERFACE=1/2` força), `follow_system`
  e `recent`. Casca nova: barra lateral (`rail`), `Page::Home` (arquivos esperam em `State.files` até a escolha da ferramenta;
  `add_paths` na Home não roteia), busca central (`palette_ui`, `egui::Modal`) e Fila (`queue_ui`, com `job::all`: cada `Job::start`
  se registra com a ferramenta da tela, `job::set_origin`). `widgets::fluent()` muda o estilo de cabeçalho, botões, `segmented`,
  lista de arquivos e `action_bar` (`action_bar_fluent`); cores em `ui2::palette` (destaque e tema lidos do registro); ícones da
  fonte Segoe Fluent Icons / MDL2 (`ui2::icon`, família `icons`). Capturas: `home`, `home_busca`, `home_fila`.
- `paste.rs`: Ctrl+V e botão Colar. O egui engole o Ctrl+V sem texto (nem `Event::Key` chega), por isso `main::handle_paste` olha a
  tecla com `GetKeyState`. Prints vão para Imagens\Conver\Prints e entram por `add_paths`, como arquivos soltos.
- `media_extra::adjust_video` (`VideoAdjust`): cor e desfoque do vídeo inteiro por filtros do ffmpeg, com os mesmos intervalos e `Filter` de `image_adjust`; tela em `extras6.rs`, op `video_adjust`.
- `pdf_extra.rs`: comprimir, senha (AES-128: o leitor de PDF do Windows não abre o AES-256 gerado pelo lopdf),
  PDF → imagens e OCR (os dois pelo `Windows.Data.Pdf`). `screenrec.rs`: gravação de tela (gdigrab).
- Testes: `src/tests*.rs` (`tests_v11.rs` = remover fundo e idiomas; `tests_v12.rs` = PDF, IA, tela, núcleo/MCP;
  `tests_v16.rs` = ferramentas da 1.5; `tests_v17.rs` = classificar; `tests_v18.rs` = leva 1.7; os dois usam imagens de exemplo em
  `CONVER_CLASSIFY_IMGS`). Transcrição, separação, ditado, voz, gerar imagem, apagar objeto e legendas
  animadas são `#[ignore]` (baixam modelos): rode com `--include-ignored`. O teste `integrations`
  falha se o Conver estiver aberto (a porta da instância única fica ocupada). Com placa AMD, o processo de teste
  termina com 0xC0000005 depois de `pdf_tools` (o driver `atidxx64.dll`, usado pela API de PDF do Windows, quebra ao
  ser descarregado): julgue pela linha `test result`. O app e a CLI evitam isso com `tools::exit_now`.

## Idiomas (i18n)

- O texto em português escrito no código **é a chave**. Todo texto que aparece na tela passa por `i18n::tr(...)`.
- `i18n/keys.txt` lista as chaves; cada `i18n/<código>.txt` tem linhas `número|tradução` alinhadas com ela,
  embutidas no exe via `include_str!`. 20 idiomas em `i18n::LANGS`.
- `cli.rs` e `presets.rs` ficam fora da extração; de `ops.rs` só entram os títulos das ferramentas (2º texto do `tool!`).
  Trechos entre `// i18n: fora` e `// i18n: dentro` também ficam fora (descrições para agentes, textos enviados à IA).
- Ao adicionar ou mudar textos: `python i18n/extrair.py` (reaproveita as traduções existentes), depois traduzir as
  linhas novas nos 19 arquivos e rodar o teste `i18n_files`, que aponta o que falta.
- `tr()` também traduz mensagens montadas (moldes com `{}`, prefixos ✔ ✖ ⚠, separadores `"   "`, `" • "`, `": "`),
  deixando nomes de arquivo e números como estão.
- Árabe, urdu e persa: `reshape_line` + `unicode-bidi` (ordem visual, parênteses espelhados). Fontes CJK vêm do
  Windows (`theme.rs`).

## Armadilhas conhecidas

- **Não edite arquivos-fonte com PowerShell** (`Set-Content`/`Get-Content -Raw` estragam UTF-8). Use as ferramentas
  de edição ou Python com `encoding='utf-8'`. Em Python, use strings raw para caminhos com `\` (ex.: `Conver\bin`
  vira backspace).
- egui 0.31: `Margin` é i8, `CornerRadius` é u8, `rect_stroke` precisa de `StrokeKind`.
- `ort` usa `load-dynamic`: `bgremove::model()` precisa rodar **antes** de criar qualquer tensor, senão o Windows
  carrega o `onnxruntime.dll` antigo do System32 ("does not export OrtGetApiBase"). O link estático falha com o
  VS local (LNK1120).
- `widgets.rs`: o `egui::Label::new(text).truncate()` da lista de arquivos não deve receber `tr()`.
- O teste `recorder_devices` grava de verdade; sem microfone com som ele só verifica a detecção de silêncio.
- Não tire capturas de tela nem simule cliques com o usuário usando o PC (já houve cliques no navegador dele).
- Para revisar o visual sem cliques, use o modo de captura da compilação de depuração (`src/captura.rs`):
  `CONVER_CAPTURA=<pasta>` + `CONVER_CENAS=img_resize:claro:pt,v_compress:escuro:ar` (+ `CONVER_ARQUIVOS`,
  `CONVER_BUSCA`) e rode `target\debug\conver.exe`. Ele abre cada tela, salva a captura da própria janela e fecha,
  sem salvar a configuração e sem passar pela instância única.

## Release

1. Subir `version` no `Cargo.toml` (o `build.ps1` e o instalador leem dali) e atualizar `CHANGELOG.md` e `README.md`.
   Numeração: atualização pequena (uma configuração, um bug corrigido, um recurso modesto, sem muitas ferramentas
   novas nem mudanças de interface) sobe só o último número (1.2.0 → 1.2.1 → 1.2.2); levas maiores sobem o do
   meio (1.2 → 1.3).
2. Rodar os testes e o `build.ps1` (Inno Setup 6 em `installer\conver.iss`); conferir o conteúdo do zip.
3. Commit (mensagem por arquivo, `git commit -F`) e push em `main`.
4. `gh release create vX.Y.Z dist\Conver-X.Y.Z-instalador.exe dist\Conver-X.Y.Z-portatil.zip dist\Conver-X.Y.Z-SHA256.txt --title "Conver X.Y.Z" --notes-file <notas>`
   (`gh` fica em `C:\Program Files\GitHub CLI\gh.exe`).
5. Componentes de terceiros novos vão para `THIRD-PARTY.md` e `licenses\` (copiados para `Conver\licencas`).
