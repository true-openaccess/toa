"""How far each language of TOA is: what is translated, missing, stale or unused.

English is the only core language; every other language is a translation that
follows it. A missing or stale translation is reported and fails nothing; only a
format error fails `check`.

    python scripts/i18n/status.py                  # one row per surface, one column per language
    python scripts/i18n/status.py --lang zh        # one language in detail
    python scripts/i18n/status.py files zh         # the files of a language, present or to be written
    python scripts/i18n/status.py check            # exit 1 on a format error, in any language
    python scripts/i18n/status.py --markdown       # the table in Markdown (also --json)

The surfaces are listed in i18n/surfaces.json, the languages in i18n/languages.json.
`--root DIR` reads another tree with the same layout, as the tests do.
Python 3.10 or later, standard library only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]  # toa/

STAMP_RE = re.compile(r"<!-- translated from (\S+) sha256:([0-9a-f]{12}) -->\n")
EN_GROUP_RE = re.compile(r"<en(?:\s+ctx=([A-Za-z0-9_-]+))?>(.*?)</en>", re.S)
T_CALL_RE = re.compile(r"""\bt\(\s*(['"])((?:\\.|(?!\1).)*?)\1""", re.S)
PLACEHOLDER_RE = re.compile(r"\{[A-Za-z0-9_]*\}")
TAG_RE = re.compile(r"<(/?)([A-Za-z][A-Za-z0-9]*)\b[^>]*?(/?)>")
LANGUAGE_FIELDS = {"code", "name", "short", "names", "html_lang", "matches", "status", "fonts", "messages", "reviewer", "ai_translated"}
STATUSES = {"published", "incubating"}
KINDS = {"document", "inline", "dictionary", "glossary"}
GLOSSARY_FIELDS = {"en", "term", "sense", "avoid", "kind", "note", "draft"}
GLOSSARY_KINDS = {"toa", "eda", "general"}


def ws(text: str) -> str:
    """A key: the English with its runs of white space as one space."""
    return " ".join(text.split())


@dataclass
class Result:
    """One surface in one language."""
    total: int = 0
    done: int = 0
    omitted: int = 0
    missing: list[str] = field(default_factory=list)
    stale: list[str] = field(default_factory=list)
    unused: list[str] = field(default_factory=list)
    errors: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)
    files: list[tuple[str, bool]] = field(default_factory=list)
    note: str = ""

    def cell(self) -> str:
        if self.note:
            text = self.note
        else:
            text = f"{self.done}/{self.total}"
        extra = []
        if self.stale:
            extra.append(f"{len(self.stale)} stale")
        if self.unused:
            extra.append(f"{len(self.unused)} unused")
        if self.errors:
            extra.append(f"{len(self.errors)} errors")
        return text + (", " + ", ".join(extra) if extra else "")

    def as_json(self) -> dict:
        return {"total": self.total, "done": self.done, "omitted": self.omitted, "missing": self.missing,
                "stale": self.stale, "unused": self.unused, "errors": self.errors, "warnings": self.warnings,
                "note": self.note or None}


class Tree:
    """The i18n files of one checkout."""

    def __init__(self, root: Path):
        self.root = root
        self.errors: list[str] = []
        self.registry = self._load("i18n/languages.json")
        self.surfaces = self._load("i18n/surfaces.json")
        self._check_registry()
        self._check_surfaces()

    # ---------------------------------------------------------------- loading

    def rel(self, path: Path) -> str:
        return path.relative_to(self.root).as_posix()

    def _load(self, rel: str):
        path = self.root / rel
        try:
            return read_json(path)
        except FileNotFoundError:
            self.errors.append(f"{rel}: missing")
        except ValueError as e:
            self.errors.append(f"{rel}: {e}")
        return {}

    @property
    def default(self) -> str:
        return self.registry.get("default", "en") if isinstance(self.registry, dict) else "en"

    def languages(self) -> list[dict]:
        langs = self.registry.get("languages", []) if isinstance(self.registry, dict) else []
        return [l for l in langs if isinstance(l, dict)]

    def translated_languages(self) -> list[str]:
        return [l["code"] for l in self.languages() if isinstance(l.get("code"), str) and l["code"] != self.default]

    def messages(self) -> dict:
        for l in self.languages():
            if l.get("code") == self.default:
                return l.get("messages", {}) or {}
        return {}

    def surface_list(self) -> list[dict]:
        s = self.surfaces.get("surfaces", []) if isinstance(self.surfaces, dict) else []
        return [x for x in s if isinstance(x, dict) and x.get("kind") in KINDS and isinstance(x.get("id"), str)]

    # ---------------------------------------------------------------- the two registries

    def _check_registry(self):
        r = self.registry
        where = "i18n/languages.json"
        if not isinstance(r, dict):
            return
        if r.get("format") != 1:
            self.errors.append(f"{where}: format must be 1")
        langs = r.get("languages")
        if not isinstance(langs, list) or not langs:
            self.errors.append(f"{where}: languages must be a non-empty list")
            return
        codes = []
        for i, l in enumerate(langs):
            if not isinstance(l, dict):
                self.errors.append(f"{where}: languages[{i}] is not an object")
                continue
            code = l.get("code")
            if not isinstance(code, str) or not re.fullmatch(r"[a-z]{2,3}(-[A-Za-z0-9]+)*", code):
                self.errors.append(f"{where}: languages[{i}]: bad code {code!r}")
                continue
            codes.append(code)
            for key in ("name", "short", "html_lang"):
                if not isinstance(l.get(key), str) or not l.get(key):
                    self.errors.append(f"{where}: {code}: {key} must be a non-empty string")
            if not isinstance(l.get("matches"), list) or not all(isinstance(m, str) for m in l.get("matches", [])):
                self.errors.append(f"{where}: {code}: matches must be a list of strings")
            if l.get("status") not in STATUSES:
                self.errors.append(f"{where}: {code}: status must be one of {sorted(STATUSES)}")
            for key in sorted(set(l) - LANGUAGE_FIELDS):
                self.errors.append(f"{where}: {code}: unknown field {key!r}")
            if code != r.get("default") and "messages" in l:
                self.errors.append(f"{where}: {code}: messages are translated in i18n/{code}/site/messages.json, not in the registry")
        dupes = [c for c, n in Counter(codes).items() if n > 1]
        if dupes:
            self.errors.append(f"{where}: codes listed twice: {dupes}")
        if r.get("default") not in codes:
            self.errors.append(f"{where}: default {r.get('default')!r} is not a listed language")
        msgs = self.messages()
        if not isinstance(msgs, dict) or not all(isinstance(v, str) for v in msgs.values()):
            self.errors.append(f"{where}: the default language's messages must be an object of strings")

    def _check_surfaces(self):
        s = self.surfaces
        where = "i18n/surfaces.json"
        if not isinstance(s, dict):
            return
        if s.get("format") != 1:
            self.errors.append(f"{where}: format must be 1")
        items = s.get("surfaces")
        if not isinstance(items, list):
            self.errors.append(f"{where}: surfaces must be a list")
            return
        ids = []
        for i, x in enumerate(items):
            name = x.get("id", f"surfaces[{i}]") if isinstance(x, dict) else f"surfaces[{i}]"
            if not isinstance(x, dict):
                self.errors.append(f"{where}: {name} is not an object")
                continue
            ids.append(name)
            kind = x.get("kind")
            if kind not in KINDS:
                self.errors.append(f"{where}: {name}: kind must be one of {sorted(KINDS)}")
                continue
            if not isinstance(x.get("translation"), str) or "{lang}" not in x["translation"]:
                self.errors.append(f"{where}: {name}: translation must be a path with {{lang}}")
            if kind != "glossary" and not x.get("source"):
                self.errors.append(f"{where}: {name}: source is missing")
            if kind == "document" and not isinstance(x.get("stamp"), str):
                self.errors.append(f"{where}: {name}: a document needs stamp, such as \"en/{{name}}\"")
            if kind == "inline":
                if x.get("markers", "en-groups") not in ("en-groups", "t-calls"):
                    self.errors.append(f"{where}: {name}: markers must be en-groups or t-calls")
                if not isinstance(x.get("base"), str):
                    self.errors.append(f"{where}: {name}: an inline surface needs base, the folder its {{path}} is relative to")
            if kind == "dictionary" and x.get("format") not in ("registry-messages", "json"):
                self.errors.append(f"{where}: {name}: format must be registry-messages or json")
        dupes = [c for c, n in Counter(ids).items() if n > 1]
        if dupes:
            self.errors.append(f"{where}: ids listed twice: {dupes}")

    # ---------------------------------------------------------------- the surfaces

    def sources(self, surface: dict) -> list[Path]:
        pats = surface.get("source", [])
        pats = [pats] if isinstance(pats, str) else pats
        exclude = set(surface.get("exclude", []))
        out = []
        for pat in pats:
            out += [p for p in sorted(self.root.glob(pat)) if p.is_file() and p.name not in exclude]
        return out

    def result(self, surface: dict, lang: str) -> Result:
        kind = surface["kind"]
        if kind == "document":
            return self._document(surface, lang)
        if kind == "inline":
            return self._inline(surface, lang)
        if kind == "dictionary":
            return self._dictionary(surface, lang)
        return self._glossary(surface, lang)

    def _document(self, s: dict, lang: str) -> Result:
        r = Result()
        sources = self.sources(s)
        r.total = len(sources)
        expected = set()
        for src in sources:
            tr = self.root / s["translation"].format(lang=lang, name=src.name)
            expected.add(tr)
            name = self.rel(tr)
            r.files.append((name, tr.exists()))
            if not tr.exists():
                r.missing.append(name)
                continue
            data = tr.read_bytes()
            if b"\r" in data:
                r.errors.append(f"{name}: CR in the file; line ends must be LF")
                continue
            try:
                text = data.decode("utf-8")
            except UnicodeDecodeError:
                r.errors.append(f"{name}: not UTF-8")
                continue
            stamp = s["stamp"].format(name=src.name)
            m = STAMP_RE.match(text)
            if not m:
                if text.startswith("<!-- translated from"):
                    r.errors.append(f"{name}: the first line looks like a stamp but is not one")
                else:
                    r.stale.append(f"{name}: not stamped")
                continue
            if m.group(1) != stamp:
                r.errors.append(f"{name}: stamped from {m.group(1)}, expected {stamp}")
                continue
            if m.group(2) != hashlib.sha256(src.read_bytes()).hexdigest()[:12]:
                r.stale.append(f"{name}: {self.rel(src)} changed since it was translated")
                continue
            r.done += 1
        # translations whose English is gone
        pattern = s["translation"].format(lang=lang, name="*")
        for tr in sorted(self.root.glob(pattern)):
            if tr.is_file() and tr not in expected and tr.name not in set(s.get("exclude", [])):
                r.unused.append(f"{self.rel(tr)}: no English source")
        return r

    def _groups(self, s: dict, src: Path) -> list[str]:
        text = src.read_text(encoding="utf-8")
        keys = []
        if s.get("markers", "en-groups") == "t-calls":
            for m in T_CALL_RE.finditer(text):
                keys.append(ws(re.sub(r"\\(.)", r"\1", m.group(2))))
        else:
            for m in EN_GROUP_RE.finditer(text):
                ctx, english = m.group(1), ws(m.group(2))
                keys.append(f"{ctx}|{english}" if ctx else english)
        return list(dict.fromkeys(keys))

    def _inline(self, s: dict, lang: str) -> Result:
        r = Result()
        base = self.root / s["base"]
        glossary = self._avoid(lang)
        if not self.sources(s):
            r.note = "no source yet"
        for src in self.sources(s):
            keys = self._groups(s, src)
            path = src.relative_to(base).as_posix()
            cat = self.root / s["translation"].format(lang=lang, path=path)
            name = self.rel(cat)
            r.files.append((name, cat.exists()))
            r.total += len(keys)
            catalog = self._catalog(cat, r)
            if catalog is None:
                r.missing += [f"{name}: {k}" for k in keys]
                continue
            script = src.suffix == ".js"
            for k in keys:
                v = catalog.get(k)
                if v is None:
                    r.missing.append(f"{name}: {k}")
                    continue
                r.done += 1
                if v == "":
                    r.omitted += 1
                    continue
                english = k.split("|", 1)[1] if "|" in k and re.match(r"[A-Za-z0-9_-]+\|", k) else k
                r.errors += [f"{name}: {k!r}: {p}" for p in text_problems(english, v, script)]
                r.warnings += [f"{name}: {k!r}: {w}" for w in avoid_problems(english, v, glossary)]
            r.unused += [f"{name}: {k}" for k in catalog if k not in set(keys)]
        return r

    def _catalog(self, path: Path, r: Result) -> dict | None:
        if not path.exists():
            return None
        try:
            cat = read_json(path)
        except ValueError as e:
            r.errors.append(f"{self.rel(path)}: {e}")
            return {}
        if not isinstance(cat, dict) or not all(isinstance(v, (str, type(None))) for v in cat.values()):
            r.errors.append(f"{self.rel(path)}: a catalog is an object of texts or null")
            return {}
        return cat

    def _dictionary(self, s: dict, lang: str) -> Result:
        r = Result()
        if s.get("format") == "registry-messages":
            english = {k: v for k, v in self.messages().items() if k not in set(s.get("keep", []))}
        else:
            src = self.root / s["source"]
            english = read_json(src) if src.exists() else {}
        cat_path = self.root / s["translation"].format(lang=lang)
        name = self.rel(cat_path)
        r.files.append((name, cat_path.exists()))
        r.total = len(english)
        catalog = self._catalog(cat_path, r)
        if catalog is None:
            r.missing = [f"{name}: {k}" for k in english]
            return r
        glossary = self._avoid(lang)
        for k, en in english.items():
            v = catalog.get(k)
            if v is None:
                r.missing.append(f"{name}: {k}")
                continue
            r.done += 1
            if v == "":
                r.omitted += 1
                continue
            r.errors += [f"{name}: {k}: {p}" for p in text_problems(en, v, False)]
            r.warnings += [f"{name}: {k}: {w}" for w in avoid_problems(en, v, glossary)]
        for k in catalog:
            if k in set(s.get("keep", [])):
                r.warnings.append(f"{name}: {k}: this message stays in English in every language; the translation is not used")
            elif k not in english:
                r.unused.append(f"{name}: {k}")
        return r

    def _glossary(self, s: dict, lang: str) -> Result:
        r = Result()
        path = self.root / s["translation"].format(lang=lang)
        name = self.rel(path)
        r.files.append((name, path.exists()))
        if not path.exists():
            r.note = "none"
            return r
        try:
            g = read_json(path)
        except ValueError as e:
            r.errors.append(f"{name}: {e}")
            return r
        r.errors += [f"{name}: {p}" for p in glossary_problems(g, lang)]
        terms = g.get("terms", []) if isinstance(g, dict) else []
        terms = [t for t in terms if isinstance(t, dict)]
        r.total = len(terms)
        r.done = sum(1 for t in terms if not t.get("draft"))
        drafts = r.total - r.done
        r.note = f"{r.total} terms" + (f" ({drafts} drafts)" if drafts else "")
        return r

    def _avoid(self, lang: str) -> list[tuple[str, list[str]]]:
        for s in self.surface_list():
            if s["kind"] == "glossary":
                path = self.root / s["translation"].format(lang=lang)
                try:
                    g = read_json(path)
                except (FileNotFoundError, ValueError):
                    return []
                return [(t["en"], t["avoid"]) for t in g.get("terms", [])
                        if isinstance(t, dict) and isinstance(t.get("en"), str) and isinstance(t.get("avoid"), list)]
        return []


# ---------------------------------------------------------------- checks of one text


def read_json(path: Path):
    data = path.read_bytes()
    if data.startswith(b"\xef\xbb\xbf"):
        raise ValueError("starts with a byte order mark")
    try:
        return json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as e:
        raise ValueError(f"not valid JSON: {e}") from None


def tags(text: str) -> Counter:
    return Counter((m.group(1) == "/", m.group(2).lower()) for m in TAG_RE.finditer(text) if not m.group(3))


def text_problems(english: str, translation: str, script: bool) -> list[str]:
    """Format errors of a translation against its English."""
    out = []
    extra = set(PLACEHOLDER_RE.findall(translation)) - set(PLACEHOLDER_RE.findall(english))
    if extra:
        out.append(f"placeholders the English does not have: {sorted(extra)}")
    if tags(english) != tags(translation):
        out.append("opens or closes other elements than the English")
    if script:
        for ch in ("'", '"', "\\"):
            if translation.count(ch) > english.count(ch):
                out.append(f"more {ch!r} than the English; use typographic quotes")
    return out


def avoid_problems(english: str, translation: str, glossary: list[tuple[str, list[str]]]) -> list[str]:
    out = []
    low = english.lower()
    for term, avoid in glossary:
        if re.search(r"\b" + re.escape(term.lower()) + r"\b", low):
            for a in avoid:
                if a and a in translation:
                    out.append(f"uses {a!r}, which the glossary avoids for {term!r}")
    return out


def glossary_problems(g, lang: str) -> list[str]:
    if not isinstance(g, dict):
        return ["a glossary is an object"]
    out = []
    if g.get("language") != lang:
        out.append(f"language must be {lang!r}")
    terms = g.get("terms")
    if not isinstance(terms, list):
        return out + ["terms must be a list"]
    seen = Counter()
    for i, t in enumerate(terms):
        if not isinstance(t, dict):
            out.append(f"terms[{i}] is not an object")
            continue
        label = t.get("en", f"terms[{i}]")
        for key in ("en", "term"):
            if not isinstance(t.get(key), str) or not t[key].strip():
                out.append(f"{label}: {key} must be a non-empty string")
        for key in ("sense", "note"):
            if key in t and not isinstance(t[key], str):
                out.append(f"{label}: {key} must be a string")
        if "avoid" in t and (not isinstance(t["avoid"], list) or not all(isinstance(a, str) for a in t["avoid"])):
            out.append(f"{label}: avoid must be a list of strings")
        if "kind" in t and t["kind"] not in GLOSSARY_KINDS:
            out.append(f"{label}: kind must be one of {sorted(GLOSSARY_KINDS)}")
        if "draft" in t and not isinstance(t["draft"], bool):
            out.append(f"{label}: draft must be true or false")
        for key in sorted(set(t) - GLOSSARY_FIELDS):
            out.append(f"{label}: unknown field {key!r}")
        seen[(t.get("en"), t.get("sense"))] += 1
    for (en, sense), n in seen.items():
        if n > 1:
            out.append(f"{en!r}{' (' + sense + ')' if sense else ''} is listed {n} times")
    return out


# ---------------------------------------------------------------- output


def collect(tree: Tree, langs: list[str]) -> dict[str, dict[str, Result]]:
    return {s["id"]: {lang: tree.result(s, lang) for lang in langs} for s in tree.surface_list()}


def table(tree: Tree, results, langs: list[str], markdown: bool) -> str:
    titles = {s["id"]: s["id"] for s in tree.surface_list()}
    rows = [["surface"] + langs] + [[titles[sid]] + [results[sid][l].cell() for l in langs] for sid in results]
    if markdown:
        lines = ["| " + " | ".join(rows[0]) + " |", "|" + "---|" * len(rows[0])]
        lines += ["| " + " | ".join(r) + " |" for r in rows[1:]]
        return "\n".join(lines)
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    return "\n".join("  ".join(c.ljust(w) for c, w in zip(r, widths)).rstrip() for r in rows)


def detail(tree: Tree, results, lang: str) -> str:
    out = []
    for s in tree.surface_list():
        r = results[s["id"]][lang]
        out.append(f"## {s['id']}: {r.cell()}")
        for label, items in (("errors", r.errors), ("stale", r.stale), ("missing", r.missing),
                             ("unused", r.unused), ("warnings", r.warnings)):
            if items:
                out.append(f"  {label}:")
                out += [f"    {x}" for x in items]
    return "\n".join(out)


def files(tree: Tree, lang: str) -> str:
    out = []
    for s in tree.surface_list():
        r = tree.result(s, lang)
        out.append(f"## {s['id']} ({s['kind']}): {s.get('title', '')}")
        if not r.files:
            out.append("  (no English source yet)")
        out += [f"  {'have ' if have else 'write'}  {name}" for name, have in r.files]
    return "\n".join(out)


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("command", nargs="?", default="report", choices=["report", "check", "files"])
    ap.add_argument("lang", nargs="?", help="the language, for files")
    ap.add_argument("--lang", dest="detail", help="one language in detail")
    ap.add_argument("--markdown", action="store_true")
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--root", type=Path, default=ROOT, help="the root of the tree (default: toa/)")
    args = ap.parse_args(argv)
    tree = Tree(args.root.resolve())

    if args.command == "files":
        if not args.lang:
            ap.error("files needs a language, such as: files zh")
        print(files(tree, args.lang))
        return 0

    langs = [args.detail] if args.detail else tree.translated_languages()
    results = collect(tree, langs)
    errors = list(tree.errors) + [e for per in results.values() for r in per.values() for e in r.errors]

    if args.json:
        print(json.dumps({"errors": tree.errors, "surfaces": {
            sid: {l: r.as_json() for l, r in per.items()} for sid, per in results.items()}},
            ensure_ascii=False, indent=2))
    elif args.detail:
        print(table(tree, results, langs, args.markdown))
        print()
        print(detail(tree, results, args.detail))
    else:
        print(table(tree, results, langs, args.markdown))

    if tree.errors and not args.json:
        print("\nerrors in the registries:", file=sys.stderr)
        for e in tree.errors:
            print(f"  {e}", file=sys.stderr)
    if args.command == "check":
        if errors:
            print(f"\n{len(errors)} format errors:", file=sys.stderr)
            for e in errors:
                print(f"  {e}", file=sys.stderr)
            return 1
        print("\nno format errors", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
