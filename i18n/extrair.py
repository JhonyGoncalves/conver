"""Gera i18n/keys.txt com os textos em português do código (o que pode aparecer na tela).

Uso: python i18n/extrair.py
Os arquivos de tradução (i18n/<código>.txt) têm uma linha para cada linha de keys.txt. Ao adicionar textos novos
no código, rode este script: as traduções existentes são reaproveitadas e as linhas novas ficam em português
até alguém traduzir (o teste `i18n_files` avisa quais faltam).
"""
import os, re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "src")
LIT = re.compile(r'(?<![A-Za-z0-9_#])"((?:[^"\\]|\\.|\\\n)*)"', re.S)
TECH = re.compile(
    r"lib(x26|vpx|mp3|opus|vorbis)|scale=|fps=|palettegen|atempo|loudnorm|anullsrc|aresample|crop=|subtitles=|concat=|"
    r"amix|transpose|force_style|setpts|PROG\||DONE\||%\(|application/|Software\\\\|SystemFileAssociations|CONVER/|"
    r"^[A-Z][a-z]+[A-Z]|User Account|kernel32|user32|\.ttc|\.ttf|serde|MultiSelectModel"
)


def unescape(s):
    s = re.sub(r"\\\n\s*", "", s)  # continuação de linha do Rust
    out, i = [], 0
    while i < len(s):
        c = s[i]
        if c == "\\" and i + 1 < len(s):
            n = s[i + 1]
            out.append({"n": "\n", "t": "\t", '"': '"', "\\": "\\", "'": "'", "0": "\0"}.get(n, "\\" + n))
            i += 2
        else:
            out.append(c)
            i += 1
    return "".join(out)


# textos usados só por dentro (leitura da saída do ffmpeg, registro do Windows, PDF, nomes de marcas…)
INTERNAL = {
    "Conver", "Music", "Duration:", "Duration: ", "bitrate: ", "Metadata:", "  Metadata", "Stream #", "Video", "Audio",
    "Subtitle", "(default)", "attached pic", "Icon", "Player", "Accept", "Content-Length", "Parent", "Type", "Pages",
    "Kids", "Count", "Catalog", "Root", "ERROR: ", "sign in", "pipe:1", "out_time_us=", "(audio)", "Audio:", "Video:",
    "Firefox", "Edge", "Chrome", "Brave", "Opera", "Charcoal", "Cinnabar", "C:\\Windows", "/select,", "hflip,vflip",
    "bestaudio/best", "bv*+ba/b", "+faststart", "Flash", "ffmpeg:", "yt-dlp:", "Conver/{}", "Conver {}",
    "Program Manager", "Helvetica", "WinAnsiEncoding", "Font", "Type1", "FConverOcr", "Image", "Subtype", "DeviceRGB",
    "DeviceGray", "ICCBased", "StdCF", "Videos", "Filter", "Width", "Height", "Encoding", "Resources", "Contents",
    "pipe:0", "mcpServers", "ET\\nQ\\n", "UVR-MDX-NET-Inst_HQ_3.onnx", "Authorization", "Bearer {}", "image/jpeg",
    "tool:", "nothing arrived", "{base} {i}", "{}{port}  →  {}", "Documents",
}
# arquivos que não têm texto de tela: linha de comando/MCP (respondem em JSON e no terminal) e dados
SKIP_FILES = {"cli.rs", "presets.rs", "search.rs", "captura.rs"}
CODE = re.compile(
    r"\bfn |\blet |=>|&\[|\|c\||format!\(|\.push\(|\)\?|<<|/Type|endstream|xref|startxref|%%EOF|\bcm /Im0|starts_with|"
    r"to_string|\{page_id\}|%1|\.\./|^file '|:charenc|log-level|^pt\.\*|^res,|^WIFI:|^\{v:\.|^\{stem\}|"
    r"T[fjzm]|^q\n|^Q\n|BT\n|progress =|conver_tmp|ggml-|huggingface|^audio=|^title=|^/FConverOcr"
)
# em ops.rs só os nomes das ferramentas aparecem na tela (o resto é para agentes, em inglês, e mensagens da linha de comando)
TOOL_TITLE = re.compile(r'tool!\(\s*"[a-z_]+",\s*"((?:[^"\\]|\\.)*)"')


# títulos das seções do menu, em maiúsculas (passariam por nomes de formato)
SECTIONS = {"IMAGEM", "VÍDEO", "ÁUDIO", "DOWNLOAD", "FERRAMENTAS", "AUTOMAÇÃO", "ENTRADA E SAÍDA"}


def is_ui(s):
    if s in SECTIONS:
        return True
    if len(s) < 2 or "://" in s or ".exe" in s or "::" in s or s.startswith("-") or s.startswith("/"):
        return False
    if s in INTERNAL or CODE.search(s) or re.fullmatch(r"[A-Z_]{4,}", s):
        return False
    # moldes de nomes de arquivo e argumentos ("{stem}_frames", "audio={device}", "{kbps}k")
    if "{" in s and " " not in s.strip() and not re.search(r"[À-ú]", s):
        return False
    if TECH.search(s):
        return False
    if not re.search(r"[A-Za-zÀ-ú]{2,}", s):
        return False
    if re.fullmatch(r"[a-z0-9_.\-]+", s):  # chaves, extensões, códigos ("pt", "png", "mp4")
        return False
    if re.fullmatch(r"[A-Z0-9 .()+/-]{2,12}", s):  # nomes de formato ("MP4", "H.264")
        return False
    return True


def keys_from_source():
    keys, seen = [], set()
    for f in sorted(os.listdir(SRC)):
        if not f.endswith(".rs") or f.startswith("tests") or f == "i18n.rs" or f in SKIP_FILES:
            continue
        text = open(os.path.join(SRC, f), encoding="utf-8-sig").read()
        if f == "ops.rs":
            for m in TOOL_TITLE.finditer(text):
                s = unescape(m.group(1))
                if s not in seen:
                    seen.add(s)
                    keys.append(s)
            continue
        # trechos marcados como sem texto de tela (descrições para agentes, textos enviados à IA, dados de exemplo)
        text = re.sub(r"(?s)//\s*i18n: fora.*?//\s*i18n: dentro", "", text)
        # sem comentários de linha (// …), que podem ter aspas
        text = re.sub(r"(?m)^\s*//.*$", "", text)
        for m in LIT.finditer(text):
            s = unescape(m.group(1))
            if is_ui(s) and s not in seen:
                seen.add(s)
                keys.append(s)
    return keys


def esc(s):
    return s.replace("\\", "\\\\").replace("\n", "\\n").replace("\t", "\\t") if False else s.replace("\n", "\\n").replace("\t", "\\t")


def read_numbered(fp):
    """Linhas "N|tradução" -> {N: tradução}."""
    out = {}
    for line in open(fp, encoding="utf-8-sig").read().split("\n"):
        n, sep, text = line.partition("|")
        if sep and n.isdigit():
            out[int(n)] = text
    return out


def main():
    path = os.path.join(ROOT, "i18n", "keys.txt")
    old_keys = open(path, encoding="utf-8").read().split("\n")[:-1] if os.path.exists(path) else []
    keys = [esc(k) for k in keys_from_source()]
    open(path, "w", encoding="utf-8", newline="\n").write("".join(k + "\n" for k in keys))
    # reaproveita as traduções que já existem (cada arquivo tem linhas "N|tradução", N = linha em keys.txt)
    for f in os.listdir(os.path.join(ROOT, "i18n")):
        if not f.endswith(".txt") or f == "keys.txt":
            continue
        fp = os.path.join(ROOT, "i18n", f)
        old = {old_keys[n - 1]: t for n, t in read_numbered(fp).items() if 0 < n <= len(old_keys)}
        lines = [f"{i + 1}|{old[k]}\n" for i, k in enumerate(keys) if k in old]
        open(fp, "w", encoding="utf-8", newline="\n").write("".join(lines))
    print(f"{len(keys)} textos em i18n/keys.txt")


if __name__ == "__main__":
    main()
