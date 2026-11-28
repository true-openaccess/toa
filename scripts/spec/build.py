#!/usr/bin/env python3
"""Check and build the published forms of the TOA standard. See README.md.

    python scripts/spec/build.py               # check, then build every version into scripts/spec/dist/
    python scripts/spec/build.py --check       # the checks only
    python scripts/spec/build.py --stamp       # after bringing a translation in line with the English
    python scripts/spec/build.py --combine     # write the combined texts again
    python scripts/spec/build.py --site DIR    # also write the web pages into the built site DIR
    python scripts/spec/build.py --no-pdf      # skip the PDFs
"""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import os
import re
import shutil
import subprocess
import sys
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import spectools as st  # noqa: E402

HERE = Path(__file__).resolve().parent
DIST = HERE / "dist"
SITE_TEMPLATE = st.ROOT / "site" / "src" / "spec-page.html"
TITLES = {"en": "TOA {v}", "zh": "TOA {v}（中文版）"}
NOTICE = {  # notices are in English in every language, as on the rnl site
    "stale": "This part of the translation may differ from the English, which prevails.",
}


# ---------------------------------------------------------------- checks


def translation_pairs(version: str | None):
    """(English part, translated part, the English path as stamped) for every translated part."""
    if version:
        for en in st.parts(version, st.SOURCE):
            for lang in st.LANGS[1:]:
                yield en, st.SPEC / version / lang / en.name, f"en/{en.name}"
    else:
        for lang in st.LANGS[1:]:
            for en in st.type_parts(st.SOURCE):
                yield en, st.SPEC / "types" / lang / en.name, f"en/{en.name}"


def check(versions: list[str]) -> list[str]:
    problems = []
    for v in versions + [None]:
        for en, tr, en_rel in translation_pairs(v):
            if not tr.exists():
                problems.append(f"{st.rel(tr)}: missing")
                continue
            try:
                problems += st.compare(st.read(en), st.read(tr), st.rel(tr))
                problems += st.stamp_problems(en, tr, en_rel)
            except ValueError as exc:
                problems.append(str(exc))
        if v is None:
            continue
        for lang in st.LANGS:
            names = [p.name for p in st.parts(v, lang)]
            if names != [p.name for p in st.parts(v, st.SOURCE)]:
                problems.append(f"{v}/{lang}: parts {names} differ from English")
            texts = [st.read(p) for p in st.parts(v, lang)]
            keys = st.section_keys(texts)
            for p, text in zip(st.parts(v, lang), texts):
                for ref in st.cited(text):
                    if ref not in keys:
                        problems.append(f"{st.rel(p)}: cites {ref}, which does not exist")
            want = st.combined(st.parts(v, lang))
            path = st.combined_path(v, lang)
            if not path.exists() or path.read_bytes().decode() != want:
                problems.append(f"{st.rel(path)}: not what the chapters give; run build.py --combine")
    return problems


def stamp(versions: list[str]):
    for v in versions + [None]:
        for en, tr, en_rel in translation_pairs(v):
            if tr.exists() and not st.compare(st.read(en), st.read(tr), st.rel(tr)):
                st.write_stamp(en, tr, en_rel)
            elif tr.exists():
                print(f"not stamped, structure differs: {st.rel(tr)}", file=sys.stderr)


def combine(versions: list[str]):
    for v in versions:
        for lang in st.LANGS:
            st.combined_path(v, lang).write_bytes(st.combined(st.parts(v, lang)).encode())


# ---------------------------------------------------------------- revision label


def git(*args) -> str:
    return subprocess.run(["git", *args], cwd=st.ROOT, capture_output=True, text=True, check=False).stdout.strip()


def label(version: str) -> dict:
    """The revision label of a version, from git: revision n, later changes, or not released."""
    paths = [f"spec/{version}", "spec/types"]
    tags = [t for t in git("tag", "--list", f"spec-{version}-r*", "--sort=-v:refname").split("\n") if t]
    last = git("log", "-1", "--format=%h %cs", "--", *paths)
    dirty = bool(git("status", "--porcelain", "--", *paths))
    if tags:
        tag = tags[0]
        n = tag.rsplit("-r", 1)[1]
        later = git("log", "--format=%h", f"{tag}..HEAD", "--", *paths)
        text = {"en": f"revision {n}", "zh": f"第 {n} 次修订"}
        if later:
            text = {"en": text["en"] + " + later changes", "zh": text["zh"] + "之后的改动"}
    else:
        text = {"en": "not released", "zh": "尚未发布"}
    commit, _, date = last.partition(" ")
    if dirty:
        text = {k: t + (" (uncommitted changes)" if k == "en" else "（有未提交的改动）") for k, t in text.items()}
    return {"text": text, "commit": commit, "date": date}


# ---------------------------------------------------------------- building


def need(tool: str):
    if not shutil.which(tool):
        sys.exit(f"build.py: {tool} is not on PATH (see README.md)")


def pandoc(args: list[str], data: str) -> str:
    res = subprocess.run(["pandoc", "--fail-if-warnings", *args], input=data.encode(), capture_output=True)
    if res.returncode or res.stderr.strip():
        sys.exit(f"pandoc failed: {res.stderr.decode(errors='replace')}")
    return res.stdout.decode()


def full_text(version: str, lang: str) -> str:
    """The text of a version with its type specifications appended, as the web page and PDF show it."""
    text = st.combined(st.parts(version, lang))
    types = st.type_parts(lang)
    if types:
        heading = "Type specifications" if lang == "en" else "类型规格"
        text += f"\n## {heading} {{#types .unnumbered}}\n\n" + "\n".join(
            demote(st.strip_stamp(st.read(p)), p.stem) for p in types)
    return text


def demote(text: str, prefix: str) -> str:
    """A type specification as a section of the page: headings one level down, ids under types-<prefix>."""
    out, fence, n = [], False, 0
    for line in text.split("\n"):
        if line.startswith("```"):
            fence = not fence
        elif line.startswith("#") and not fence:
            hashes, _, title = line.partition(" ")
            level = len(hashes) + 1
            if len(hashes) == 1:
                ident = f"types-{prefix}"
            else:  # by position, so that the ids of the languages agree
                n += 1
                ident = f"types-{prefix}-{n}"
            line = f"{'#' * level} {title} {{#{ident} .type-spec}}"
        out.append(line)
    return "\n".join(out)


FONTS = {  # wanted families, in order; only those typst finds are passed
    "en": {"serif": ["Libertinus Serif", "Noto Serif", "Cambria", "Georgia"],
           "sans": ["Noto Sans", "Segoe UI", "Arial"],
           "mono": ["Cascadia Mono", "Consolas", "DejaVu Sans Mono"]},
    "zh": {"serif": ["Libertinus Serif", "Noto Serif CJK SC", "Source Han Serif SC", "SimSun", "Songti SC"],
           "sans": ["Noto Sans CJK SC", "Source Han Sans SC", "Microsoft YaHei", "PingFang SC"],
           "mono": ["Cascadia Mono", "Consolas", "DejaVu Sans Mono", "Noto Sans CJK SC", "Microsoft YaHei"]},
}


def typst_fonts(lang: str) -> dict:
    listed = subprocess.run(["typst", "fonts"], capture_output=True, text=True, encoding="utf-8").stdout
    have = {line.strip().lower() for line in listed.splitlines()}
    return {k: [f for f in v if f.lower() in have] for k, v in FONTS[lang].items()}


def build(version: str, lang: str, lab: dict, pdf: bool) -> dict[str, Path]:
    out = DIST / version
    out.mkdir(parents=True, exist_ok=True)
    stem = f"toa-{version}" if lang == "en" else f"toa-{version}.{lang}"
    files = {}
    md = out / f"{stem}.md"
    md.write_bytes(st.combined(st.parts(version, lang)).encode())
    files["md"] = md
    text = full_text(version, lang)
    common = ["--from", "markdown-auto_identifiers+pipe_tables", "--lua-filter", str(HERE / "toa.lua"),
              "--metadata", f"html-lang={'zh-CN' if lang == 'zh' else 'en'}", "--metadata", f"toalang={lang}"]
    body = pandoc(common + ["--to", "html5", "--wrap=none"], text)
    page = render_page(version, lang, lab, body, files)
    (out / f"{stem}.html").write_bytes(page.encode())
    files["html"] = out / f"{stem}.html"
    if pdf:
        fonts = [a for kind, fams in typst_fonts(lang).items() for f in fams for a in ("--metadata", f"{kind}={f}")]
        typ = pandoc(common + ["--to", "typst", "--standalone", "--syntax-highlighting=none",
                               "--template", str(HERE / "print.typ"), *fonts,
                               "--metadata", f"title={TITLES[lang].format(v=version)}",
                               "--metadata", f"typst-lang={lang}",
                               "--metadata", f"contents-title={'Contents' if lang == 'en' else '目录'}",
                               "--metadata", f"revision={lab['text'][lang]} · {lab['date']} · {lab['commit']}"], text)
        work = DIST / "work"
        work.mkdir(parents=True, exist_ok=True)
        src = work / f"{stem}.typ"
        src.write_bytes(typ.encode())
        res = subprocess.run(["typst", "compile", str(src), str(out / f"{stem}.pdf")], capture_output=True)
        if res.returncode or res.stderr.strip():
            sys.exit(f"typst failed: {res.stderr.decode(errors='replace')}")
        files["pdf"] = out / f"{stem}.pdf"
    zpath = out / f"{stem}-chapters.zip"
    write_zip(zpath, version, lang)
    files["zip"] = zpath
    return files


def write_zip(path: Path, version: str, lang: str):
    entries = [(f"{version}/{lang}/{p.name}", p.read_bytes()) for p in st.parts(version, lang)]
    entries += [(f"types/{lang}/{p.name}", p.read_bytes()) for p in st.type_parts(lang)]
    entries += [(f"types/{t}.schema.json", (st.SPEC / "types" / f"{t}.schema.json").read_bytes())
                for t in st.TYPES if (st.SPEC / "types" / f"{t}.schema.json").exists()]
    sums = "".join(f"{hashlib.sha256(d).hexdigest()}  {n}\n" for n, d in entries)
    entries.append(("SHA256SUMS", sums.encode()))
    stamp = (1980, 1, 1, 0, 0, 0)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in entries:
            info = zipfile.ZipInfo(name, stamp)
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)


HEADING_RE = re.compile(r'<h([23]) id="([^"]+)"[^>]*>(.*?)</h\1>', re.S)


def contents(body: str) -> tuple[str, str]:
    """The contents of the page, chapters with their sections nested, and the chapters alone."""
    heads = []
    for m in HEADING_RE.finditer(body):
        inner = re.sub(r'<a class="anchor".*?</a>', "", m.group(3), flags=re.S)
        num = re.search(r'<span class="sec">(.*?)</span>', inner)
        title = re.sub(r"<[^>]+>", "", re.sub(r'<span class="sec">.*?</span>', "", inner)).strip()
        heads.append((int(m.group(1)), m.group(2), num.group(1) if num else "", title))
    out, open_sub = [], False
    for level, ident, num, title in heads:
        n = f'<span class="n">{num}</span>' if num else ""
        if level == 2:
            if open_sub:
                out.append("</ol></li>")
                open_sub = False
            elif out:
                out.append("</li>")
            out.append(f'<li><a href="#{ident}">{n}{title}</a>')
        else:
            if not open_sub:
                out.append("<ol>")
                open_sub = True
            out.append(f'<li><a href="#{ident}">{n}{title}</a></li>')
    if out:
        out.append("</ol></li>" if open_sub else "</li>")
    chapters = "\n".join(f'<li><a href="#{i}">{(n + " ") if n else ""}{t}</a></li>' for lv, i, n, t in heads if lv == 2)
    return "\n".join(out), chapters


def render_page(version: str, lang: str, lab: dict, body: str, files: dict) -> str:
    """The web page: the site's template when there is one, else a plain page of our own."""
    template = SITE_TEMPLATE if SITE_TEMPLATE.exists() else HERE / "page.html"
    page = template.read_bytes().decode()
    toc, chapters = contents(body)
    other = "zh" if lang == "en" else "en"
    stem = f"toa-{version}" if lang == "en" else f"toa-{version}.{lang}"
    downloads = [f"{stem}.md", f"{stem}-chapters.zip", "SHA256SUMS"]
    downloads.insert(1, f"{stem}.pdf")
    values = {
        "lang": "zh-CN" if lang == "zh" else "en",
        "title": html.escape(TITLES[lang].format(v=version)),
        "version": version,
        "revision": html.escape(f"{lab['text'][lang]} · {lab['date']} · {lab['commit']}"),
        "other_lang": other,
        "downloads": "\n".join(f'<li><a href="{d}">{d}</a></li>' for d in downloads),
        "toc": toc,
        "chapters": chapters,
        "body": body.strip(),
    }
    for key, value in values.items():
        page = page.replace("{{" + key + "}}", value)
    return page


def sums(paths: list[Path], target: Path):
    target.write_bytes("".join(f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n" for p in sorted(paths)).encode())


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="only check")
    ap.add_argument("--stamp", action="store_true", help="stamp translations whose structure is in line")
    ap.add_argument("--combine", action="store_true", help="write the combined texts again")
    ap.add_argument("--version", help="only this version")
    ap.add_argument("--site", type=Path, help="also write the web pages into this built site")
    ap.add_argument("--no-pdf", action="store_true", help="skip the PDFs")
    args = ap.parse_args()
    vs = [args.version] if args.version else st.versions()
    if args.stamp:
        stamp(vs)
    if args.combine:
        combine(vs)
    problems = check(vs)
    for p in problems:
        print("error:", p, file=sys.stderr)
    if problems:
        return 1
    print(f"checks: ok ({', '.join(vs)})")
    if args.check or args.stamp or args.combine:
        return 0
    need("pandoc")
    if not args.no_pdf:
        need("typst")
    for v in vs:
        lab = label(v)
        built = []
        for lang in st.LANGS:
            files = build(v, lang, lab, not args.no_pdf)
            built += files.values()
        # The downloads. The page is not one: with the site's template it is left for the site build to
        # finish (site/build.py), so its bytes here are not the bytes the site serves.
        downloads = [p for p in built if p.suffix != ".html"]
        sums(downloads, DIST / v / "SHA256SUMS")
        print(f"built {v}: {', '.join(p.name for p in built)}")
        if args.site:
            for lang in st.LANGS:
                target = args.site / lang / "spec" / v
                target.mkdir(parents=True, exist_ok=True)
                stem = f"toa-{v}" if lang == "en" else f"toa-{v}.{lang}"
                shutil.copyfile(DIST / v / f"{stem}.html", target / "index.html")  # the only page
                for p in downloads:
                    shutil.copyfile(p, target / p.name)
                shutil.copyfile(DIST / v / "SHA256SUMS", target / "SHA256SUMS")
            print(f"site: {args.site / 'en' / 'spec' / v}, {args.site / 'zh' / 'spec' / v}")
    if args.site and vs:
        newest = vs[-1]
        for lang in st.LANGS:
            index = args.site / lang / "spec" / "index.html"
            index.write_bytes(redirect(f"{newest}/").encode())
    return 0


def redirect(to: str) -> str:
    return ('<!doctype html><meta charset="utf-8"><title>TOA</title>'
            f'<script>location.replace("{to}" + location.hash)</script><a href="{to}">{to}</a>\n')


if __name__ == "__main__":
    sys.exit(main())
