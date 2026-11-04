"""Converte arquivos de tradução escritos com a numeração antiga (600 linhas, antes das seções do menu).

Arquivos que começam com a linha "#numeracao-600" são convertidos para a numeração atual de keys.txt:
  antigas 1–236 → iguais; 237–257 → +1; 258 → 260; 259–600 → +3; e "S1|", "S2|", "S3|" são as seções
  IMAGEM (237), DOWNLOAD (259) e FERRAMENTAS (261).
Uso: python i18n/renumerar.py
"""
import os

DIR = os.path.dirname(os.path.abspath(__file__))
SECTIONS = {"S1": 237, "S2": 259, "S3": 261}


def new_number(old):
    if old <= 236:
        return old
    if old <= 257:
        return old + 1
    if old == 258:
        return 260
    return old + 3


for f in sorted(os.listdir(DIR)):
    if not f.endswith(".txt") or f == "keys.txt":
        continue
    p = os.path.join(DIR, f)
    lines = open(p, encoding="utf-8-sig").read().split("\n")
    if not lines or lines[0].strip() != "#numeracao-600":
        continue
    out = []
    for line in lines[1:]:
        n, sep, text = line.partition("|")
        if not sep:
            continue
        num = SECTIONS.get(n) or (new_number(int(n)) if n.isdigit() else None)
        if num:
            out.append((num, text))
    out.sort()
    open(p, "w", encoding="utf-8", newline="\n").write("".join(f"{n}|{t}\n" for n, t in out))
    print(f"{f}: {len(out)} linhas convertidas")
