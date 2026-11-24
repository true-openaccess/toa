"""Reading the chapters of the TOA standard: parts, structure, stamps, references, combined texts.

Used by build.py. Python 3.10 or later, standard library only.
"""
from __future__ import annotations

import hashlib
import re
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # toa/
SPEC = ROOT / "spec"
SOURCE = "en"  # the normative language
LANGS = ("en", "zh")
VERSION_RE = re.compile(r"\d+\.\d+")
STAMP_RE = re.compile(r"<!-- translated from (\S+) sha256:([0-9a-f]{12}) -->\n")
TYPES = ("ifc", "ckt", "tb", "evd", "doc", "lib")


def versions() -> list[str]:
    """The versions of the standard: folders such as 0.1/ that hold en/."""
    found = [p.name for p in SPEC.iterdir() if p.is_dir() and VERSION_RE.fullmatch(p.name) and (p / SOURCE).is_dir()]
    return sorted(found, key=lambda v: tuple(int(x) for x in v.split(".")))


def part_order(name: str) -> tuple:
    if name == "front.md":
        return (0, 0, "")
    if m := re.fullmatch(r"ch(\d\d)\.md", name):
        return (1, int(m.group(1)), "")
    if m := re.fullmatch(r"annex-([a-z])\.md", name):
        return (2, 0, m.group(1))
    raise ValueError(f"not a part of the standard: {name}")


def parts(version: str, lang: str) -> list[Path]:
    folder = SPEC / version / lang
    return sorted((p for p in folder.iterdir() if p.suffix == ".md"), key=lambda p: part_order(p.name))


def type_parts(lang: str) -> list[Path]:
    folder = SPEC / "types" / lang
    return [folder / f"{t}.md" for t in TYPES if (folder / f"{t}.md").exists()]


def read(path: Path) -> str:
    data = path.read_bytes()
    if b"\r" in data:
        raise ValueError(f"{rel(path)}: CR in the file; line ends must be LF")
    return data.decode("utf-8")


def rel(path: Path) -> str:
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def source_hash(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()[:12]


def strip_stamp(text: str) -> str:
    return STAMP_RE.sub("", text, count=1) if text.startswith("<!--") else text


# ---------------------------------------------------------------- structure


@dataclass
class Section:
    key: str                     # "3.2.1", "annex-a", "title"
    level: int
    items: int = 0               # list items
    rows: int = 0                # table rows, separators not counted
    blocks: list = field(default_factory=list)  # info strings of code blocks
    code: Counter = field(default_factory=Counter)  # inline code without white space


HEADING_RE = re.compile(r"(#{1,4}) (.*)")
ITEM_RE = re.compile(r"\s*(?:[-*]|\d+\.) ")
SEP_RE = re.compile(r"\s*\|[-:| ]+\|\s*$")
CODE_RE = re.compile(r"(`+)(.+?)\1")


def heading_key(level: int, title: str) -> str:
    if level == 1:
        return "title"
    if m := re.match(r"(\d+(?:\.\d+)*) ", title):
        return m.group(1)
    if m := re.match(r"(?:Annex|附录) ([A-Z])", title):
        return "annex-" + m.group(1).lower()
    return "?"  # an unnumbered heading: only its level is compared


def structure(text: str) -> list[Section]:
    sections = [Section("start", 0)]
    fence = None
    for line in strip_stamp(text).split("\n"):
        stripped = line.strip()
        if fence:
            if stripped == fence:
                fence = None
            continue
        if stripped.startswith("```"):
            fence = stripped[: len(stripped) - len(stripped.lstrip("`"))]
            sections[-1].blocks.append(stripped[len(fence):])
            continue
        if m := HEADING_RE.fullmatch(line):
            sections.append(Section(heading_key(len(m.group(1)), m.group(2)), len(m.group(1))))
            continue
        sec = sections[-1]
        if ITEM_RE.match(line):
            sec.items += 1
        if stripped.startswith("|") and not SEP_RE.match(line):
            sec.rows += 1
        for m in CODE_RE.finditer(line):
            span = m.group(2).strip()
            if not re.search(r"\s", span):
                sec.code[span] += 1
    return sections


def compare(en_text: str, zh_text: str, name: str) -> list[str]:
    """Differences of structure between a part and its translation."""
    a, b = structure(en_text), structure(zh_text)
    problems = []
    ka, kb = [(s.key, s.level) for s in a], [(s.key, s.level) for s in b]
    if ka != kb:
        return [f"{name}: headings differ: {ka} / {kb}"]
    for x, y in zip(a, b):
        where = f"{name} {x.key}"
        if x.items != y.items:
            problems.append(f"{where}: {x.items} list items in English, {y.items} in the translation")
        if x.rows != y.rows:
            problems.append(f"{where}: {x.rows} table rows in English, {y.rows} in the translation")
        if x.blocks != y.blocks:
            problems.append(f"{where}: code blocks {x.blocks} in English, {y.blocks} in the translation")
        if x.code != y.code:
            only_en = sorted((x.code - y.code).elements())
            only_zh = sorted((y.code - x.code).elements())
            problems.append(f"{where}: inline code differs; only in English {only_en}, only in the translation {only_zh}")
    return problems


# ---------------------------------------------------------------- stamps


def stamp_problems(en_path: Path, zh_path: Path, en_rel: str) -> list[str]:
    text = read(zh_path)
    m = STAMP_RE.match(text)
    if not m:
        return [f"{rel(zh_path)}: no stamp; run build.py --stamp after translating"]
    if m.group(1) != en_rel:
        return [f"{rel(zh_path)}: stamped from {m.group(1)}, expected {en_rel}"]
    if m.group(2) != source_hash(en_path):
        return [f"{rel(zh_path)}: {en_rel} changed since it was translated; bring the translation in line, then --stamp"]
    return []


def write_stamp(en_path: Path, zh_path: Path, en_rel: str):
    text = strip_stamp(read(zh_path))
    zh_path.write_bytes(f"<!-- translated from {en_rel} sha256:{source_hash(en_path)} -->\n{text}".encode())


# ---------------------------------------------------------------- references

REF_PATTERNS = [
    re.compile(r"(?<![\w.`/-])(\d+\.\d+(?:\.\d+)?)(?![\w`/-])(?!\.\d)"),
    re.compile(r"(?:chapters?|Chapter|第) ?(\d+)(?: (?:to|and) (\d+))?"),
    re.compile(r"(?:第 )(\d+)(?:、(\d+))? ?章"),
    re.compile(r"(?:Annex|附录) ([A-Z])"),
]


def cited(text: str) -> list[str]:
    """Section numbers cited in the text, outside code and headings."""
    out = []
    fence = False
    for line in strip_stamp(text).split("\n"):
        if line.strip().startswith("```"):
            fence = not fence
            continue
        if fence or line.startswith("#"):
            continue
        plain = CODE_RE.sub(" ", line)
        plain = re.sub(r"\]\([^)]*\)", "]", plain)  # link targets
        for m in REF_PATTERNS[0].finditer(plain):
            out.append(m.group(1))
        for m in re.finditer(r"(?:chapters?|第) ?(\d+)(?: (?:to|and) (\d+))?(?: ?章)?", plain):
            out.extend(g for g in m.groups() if g)
        for m in REF_PATTERNS[3].finditer(plain):
            out.append("annex-" + m.group(1).lower())
    return out


def section_keys(texts: list[str]) -> set[str]:
    keys = set()
    for text in texts:
        for s in structure(text):
            keys.add(s.key)
    return keys


# ---------------------------------------------------------------- combined texts


def combined(paths: list[Path]) -> str:
    return "\n".join(strip_stamp(read(p)).rstrip("\n") + "\n" for p in paths)


def combined_path(version: str, lang: str) -> Path:
    return SPEC / (f"toa-{version}.md" if lang == SOURCE else f"toa-{version}.{lang}.md")
