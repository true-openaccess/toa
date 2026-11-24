"""Build the project site in the languages of i18n/languages.json.

The pages in site/src/ are written in English, each text to translate in a group:

    <en>An object is named by its content</en>
    <en ctx=verb>seal</en>                      a context tells apart two senses of one English text

The translations are in i18n/<lang>/site/<page>.json, keyed by the English with its runs of white
space as one space, or "<ctx>|<English>" for a group with a context. A text is the translation;
null, or no key, is one not made yet and shows the English; "" is one left out on purpose. The format
is the one scripts/i18n/status.py reports on (i18n/README.md).

A page less than 80% translated is shown in English, with a notice; one between 80% and 100% is
shown translated, with the English where a text is missing, and a notice. Notices are in English.

Names in double braces are filled after the groups, so a translation keeps them as its English has
them:
- the shared messages of i18n/languages.json, translated in i18n/<lang>/site/messages.json; among them
  full_name, the project's full name, which is not translated and is written only there;
- the parts the build makes: html_lang, fonts, lang_menu, preview_bar, href_spec, home (the way to
  the language's pages: empty on them), and on the translation page the tables made from
  scripts/i18n/status.py and the glossaries, status_langs, status_<surface> (one row of the table,
  the id of the surface with every run of other characters than a-z as _, such as status_site_pages),
  glossary_head and glossary_rows.

The standard's pages are made by scripts/spec/build.py --site DIR from the template
site/src/spec-page.html, which fills the standard's parts; site/build.py --out DIR, run after it on the
same DIR, then translates the template's groups and fills the site's parts in each
<lang>/spec/<version>/index.html, leaving the standard's text, between the marks <!--spec-body--> and
<!--/spec-body-->, as it is. A download the page lists and DIR does not have, such as the PDF of a
build with --no-pdf, is left out. Without the standard's pages the site is built all the same, and its
links to the standard lead nowhere.

A language is built when languages.json says it is published; one that is incubating only with
--incubating, for a preview, and the menus of that build list it marked as a preview.

--preview puts the message "preview" at the top of every page, in the page's language: the site is a
preview and nothing has been released. Such a build is public, so it leaves out the line that an
incubating language is offered in no menu of the published site.

Output, in site/dist/ by default (ignored by git):
    <lang>/<page>         each page in each language
    <lang>/spec/<v>/      the standard's pages, written by scripts/spec/build.py and finished here
    assets/               site/src/assets/, shared by the languages
    llms.txt              site/src/llms.txt, the plain-text entry for agents, in English only
    index.html            leads to the language of the browser, or to English

Usage, from the root of the repository (the folder toa/), standard library only:
    python site/build.py                      build into site/dist/
    python site/build.py --check              check the sources and catalogs, write nothing
    python site/build.py --out DIR            build into DIR instead
    python site/build.py --incubating         also build the languages not published yet
    python site/build.py --spec-href URL      where the pages link the standard (default spec/0.1/)
    python site/build.py --preview            say on every page that the site is a preview
    python site/build.py --note TEXT          a line at the top of every page, such as "mockup"

The whole site, with the standard (pandoc and, for the PDFs, typst):
    python scripts/spec/build.py --site site/dist && python site/build.py
"""
import argparse
import html
import importlib.util
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent          # the folder toa/
SRC = ROOT / "site" / "src"
I18N = ROOT / "i18n"
PAGES = ["index.html", "tools.html", "agents.html", "translation.html"]
SPEC_PAGE = "spec-page.html"
# The parts of the standard's page that scripts/spec/build.py fills before this build sees it.
SPEC_FIELDS = {"lang", "title", "version", "revision", "other_lang", "downloads", "toc", "chapters", "body"}
SPEC_MARK = "<!--site:spec-page-->\n"
BODY_RE = re.compile(r"(<!--spec-body-->.*?<!--/spec-body-->)", re.S)
DOWNLOAD_RE = re.compile(r'<li><a href="([^"/]+)">\1</a></li>')
THRESHOLD = 0.8

EN_GROUP_RE = re.compile(r"<en(?:\s+ctx=([A-Za-z0-9_-]+))?>(.*?)</en>", re.S)
EN_TAG_RE = re.compile(r"</?en\b[^>]*>")
FIELD_RE = re.compile(r"\{\{([a-z_]+)\}\}")
MESSAGE_FIELD_RE = re.compile(r"\{([a-z_]+)\}")
CJK_RE = re.compile(r"[　-〿㐀-䶿一-鿿＀-￯]")
BUILT = {"html_lang", "fonts", "lang_menu", "preview_bar", "href_spec", "home",
         "status_langs", "glossary_head", "glossary_rows", "spec_versions", "spec_notice"}

GLOBE = ('<svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><circle cx="8" cy="8" r="6.5" '
         'fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M1.5 8h13M8 1.5c2 2 2.8 4.2 2.8 6.5S10 '
         '12.5 8 14.5C6 12.5 5.2 10.3 5.2 8S6 3.5 8 1.5z" fill="none" stroke="currentColor" stroke-width="1.3"/></svg>')


class BuildError(Exception):
    pass


def ws(text):
    return " ".join(text.split())


def ticks(text):
    """Plain text with `code` in backticks, as HTML."""
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", html.escape(text))


def read_json(path):
    return json.loads(path.read_bytes().decode("utf-8"))


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.replace("\r\n", "\n").encode("utf-8"))


class Site:
    def __init__(self, incubating=False, spec_href="spec/0.1/", note=None, preview=False):
        reg = read_json(I18N / "languages.json")
        self.default = reg.get("default", "en")
        self.all = reg["languages"]
        self.english = next(l for l in self.all if l["code"] == self.default)["messages"]
        self.languages = [l for l in self.all
                          if l.get("status") == "published" or (incubating and l.get("status") == "incubating")]
        self.incubating = incubating
        self.spec_href = spec_href
        self.note = note
        self.preview = preview
        self.problems = []

    # ---- sources and catalogs ----

    def groups(self, page, text, fields=()):
        """The keys of the page's groups, in order; records malformed markup. fields: others allowed."""
        keys = []
        for m in EN_GROUP_RE.finditer(text):
            ctx, inner = m.group(1), m.group(2)
            if EN_TAG_RE.search(inner):
                self.problems.append(f"{page}: a group inside a group: {ws(inner)[:60]}")
            if not ws(inner):
                self.problems.append(f"{page}: an empty group")
            if CJK_RE.search(inner):
                self.problems.append(f"{page}: the English has Chinese characters: {ws(inner)[:60]}")
            keys.append(f"{ctx}|{ws(inner)}" if ctx else ws(inner))
        stray = EN_TAG_RE.findall(EN_GROUP_RE.sub("", text))
        if stray:
            self.problems.append(f"{page}: {len(stray)} en tag(s) outside a whole group")
        # The same text may stand in several places: it has one key and one translation.
        for name in FIELD_RE.findall(text):
            if name not in BUILT and name not in self.english and name not in fields                     and not name.startswith("status_"):
                self.problems.append(f"{page}: {{{{{name}}}}} is neither a message nor a part the build makes")
        return keys

    def catalog(self, code, page):
        if code == self.default:
            return {}
        path = I18N / code / "site" / f"{page}.json"
        if not path.exists():
            return {}
        data = read_json(path)
        if not isinstance(data, dict):
            self.problems.append(f"{path.relative_to(ROOT)}: not a JSON object")
            return {}
        return data

    def messages(self, lang):
        out = dict(self.english)
        if lang["code"] != self.default:
            path = I18N / lang["code"] / "site" / "messages.json"
            if path.exists():
                for k, v in read_json(path).items():
                    if isinstance(v, str) and v and k in out:
                        out[k] = v
        # A message may name another, such as {full_name}; the full name itself is never translated.
        out["full_name"] = self.english["full_name"]
        return {k: MESSAGE_FIELD_RE.sub(lambda m: out.get(m.group(1), m.group(0)), v) for k, v in out.items()}

    # ---- the parts the build makes ----

    def fonts(self, lang):
        f = lang.get("fonts") or {}
        parts = []
        if f.get("google"):
            parts.append('<link rel="preconnect" href="https://fonts.googleapis.com">\n'
                         '<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n'
                         f'<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family={f["google"]}&amp;display=swap">')
        decl = "".join(f"--{k}:{f[k]};".replace("--body", "--sans") for k in ("body", "mono") if f.get(k))
        if decl:
            parts.append(f"<style>:root{{{decl}}}</style>")
        return "\n".join(parts)

    def lang_menu(self, lang, msg, href, languages=None):
        """The language menu; href(other) is the address of this page in the other language."""
        links = []
        for other in languages or self.languages:
            preview = other.get("status") != "published"
            code = other["code"].upper() + (" · preview" if preview else "")
            current = ' aria-current="true"' if other is lang else ""
            links.append(f'<a href="{href(other)}" lang="{other["html_lang"]}"{current}>'
                         f'{html.escape(other["name"])} <span class="code">{code}</span></a>')
        return (f'<details class="lang">\n        <summary class="btn small" aria-label="{msg["language_menu"]}">\n'
                f'          {GLOBE}\n          {html.escape(lang["short"])}\n        </summary>\n'
                f'        <div class="lang-list">\n          ' + "\n          ".join(links) + "\n        </div>\n      </details>")

    def preview_bar(self, lang, msg, notice):
        lines = []
        if self.preview:
            lines.append(html.escape(msg["preview"]))
        if self.note:
            lines.append(html.escape(self.note))
        if lang.get("status") != "published" and not self.preview:
            lines.append(f"Preview of {html.escape(lang['names'].get('en', lang['name']))}, a language not yet published: "
                         "the published site offers it in no menu.")
        if notice:
            lines.append(notice)
        if not lines:
            return ""
        return '<div class="preview-bar"><div class="wrap">' + " ".join(lines) + "</div></div>"

    # ---- pages ----

    def render(self, page, text, lang, extra=None, fields=()):
        """The page in the language; extra: built parts of this page besides the common ones."""
        keys = self.groups(page, text, fields)
        catalog = self.catalog(lang["code"], page)
        msg = self.messages(lang)
        distinct = list(dict.fromkeys(keys))
        done = [k for k in distinct if isinstance(catalog.get(k), str)]
        share = len(done) / len(distinct) if distinct else 1.0
        use = catalog if share >= THRESHOLD else {}
        notice = ""
        if lang["code"] != self.default and share < 1:
            name = lang["names"].get("en", lang["name"])
            which = "not_translated" if share < THRESHOLD else "partly_translated"
            notice = html.escape(self.english[which].replace("{language}", name))

        def group(m):
            ctx, inner = m.group(1), m.group(2)
            key = f"{ctx}|{ws(inner)}" if ctx else ws(inner)
            t = use.get(key)
            return t if isinstance(t, str) else inner

        out = EN_GROUP_RE.sub(group, text)
        built = {
            "html_lang": lang["html_lang"],
            "fonts": self.fonts(lang),
            "lang_menu": self.lang_menu(lang, msg, lambda other: f"../{other['code']}/{page}"),
            "preview_bar": self.preview_bar(lang, msg, notice),
            "href_spec": html.escape(self.spec_href),
            "home": "",
        }
        if page == "translation.html":
            built.update(self.progress(msg))
            for name in built:
                if name.startswith("status_") and "{{" + name + "}}" not in text:
                    self.problems.append(f"{page}: no row for the surface of {{{{{name}}}}}")
        built.update(extra or {})

        def field(m):
            name = m.group(1)
            if name in built:
                return built[name]
            if name in msg:
                return html.escape(msg[name])
            self.problems.append(f"{lang['code']}/{page}: {{{{{name}}}}} left after translation")
            return m.group(0)

        out = FIELD_RE.sub(field, out)
        if lang["code"] == self.default:
            bare = re.sub(r'<div class="lang-list">.*?</div>', "", out, flags=re.S)
            # Text marked as in another language, such as the terms of a glossary, may be Chinese.
            bare = re.sub(r'<(\w+)\b[^>]*\slang="(?!en")[^"]*"[^>]*>.*?</\1>', "", bare, flags=re.S)
            if CJK_RE.search(bare):
                self.problems.append(f"{lang['code']}/{page}: Chinese characters in an English page")
        return out, len(done), len(distinct)

    # ---- the translation page ----

    def i18n_tree(self):
        """The report of scripts/i18n/status.py, read in this process."""
        path = ROOT / "scripts" / "i18n" / "status.py"
        spec = importlib.util.spec_from_file_location("toa_i18n_status", path)
        mod = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = mod
        spec.loader.exec_module(mod)
        return mod, mod.Tree(ROOT)

    def progress(self, msg):
        """The parts of the translation page: how far each language is, and the glossaries."""
        mod, tree = self.i18n_tree()
        langs = [l for l in self.all if l["code"] != self.default]
        results = mod.collect(tree, [l["code"] for l in langs])
        out = {}
        head = []
        for l in langs:
            published = l.get("status") == "published"
            head.append(f'<th scope="col" lang="{l["html_lang"]}">{html.escape(l["name"])} '
                        f'<span class="code">{l["code"].upper()}</span> '
                        f'<span class="pill {"ok" if published else "plain"}">'
                        f'{"published" if published else "incubating"}</span></th>')
        out["status_langs"] = "".join(head)
        for sid, per in results.items():
            cells = []
            for l in langs:
                r = per[l["code"]]
                share = r.done / r.total if r.total else 1.0
                extra = []
                if r.stale:
                    extra.append(f"{len(r.stale)} stale")
                if r.omitted:
                    extra.append(f"{r.omitted} left out")
                detail = f"{r.done}/{r.total}" + ("; " + ", ".join(extra) if extra else "")
                cls = "ok" if share >= 1 and not r.stale else "part" if share >= THRESHOLD else "low"
                cells.append(f'<td class="prog {cls}"><span class="bar"><span style="width:{share:.1%}"></span></span>'
                             f'<b>{share:.0%}</b> <span class="faint">{html.escape(detail)}</span></td>')
            out["status_" + re.sub(r"[^a-z]+", "_", sid).strip("_")] = "".join(cells)
        # The glossaries: the English terms of TOA, with each language's rendering.
        glossaries = {}
        for l in langs:
            path = I18N / l["code"] / "glossary.json"
            if path.exists():
                glossaries[l["code"]] = read_json(path).get("terms", [])
        with_glossary = [l for l in langs if l["code"] in glossaries]
        out["glossary_head"] = "".join(f'<th scope="col" lang="{l["html_lang"]}">{html.escape(l["name"])}</th>'
                                       for l in with_glossary)
        rows = {}
        for code, terms in glossaries.items():
            for t in terms:
                if t.get("kind") == "toa":
                    rows.setdefault((t["en"], t.get("sense", "")), {})[code] = t
        lines = []
        for (en, sense), per in sorted(rows.items(), key=lambda kv: (kv[0][0].lower(), kv[0][1])):
            first = f"<b>{html.escape(en)}</b>" + (f'<div class="faint">{ticks(sense)}</div>' if sense else "")
            cells = []
            for l in with_glossary:
                t = per.get(l["code"])
                if not t:
                    cells.append("<td></td>")
                    continue
                draft = ' <span class="pill draft">draft</span>' if t.get("draft") else ""
                note = f'<div class="faint" lang="en">{ticks(t["note"])}</div>' if t.get("note") else ""
                cells.append(f'<td lang="{l["html_lang"]}">{html.escape(t["term"])}{draft}{note}</td>')
            lines.append(f'<tr><td lang="en">{first}</td>{"".join(cells)}</tr>')
        out["glossary_rows"] = "\n".join(lines)
        return out

    # ---- the standard's pages ----

    def spec_pages(self, out):
        """The standard's pages scripts/spec/build.py wrote into out: {code: [(version, path)]}."""
        found = {}
        codes = {l["code"] for l in self.all}
        for path in sorted(out.glob("*/spec/*/index.html")):
            code, version = path.parts[-4], path.parts[-2]
            if code in codes:
                found.setdefault(code, []).append((version, path))
        return found

    def download(self, folder, msg, m):
        """One download of the standard's page, labelled; nothing for a file not there."""
        name = m.group(1)
        f = folder / name
        if not f.exists():
            return ""
        kind = ("sums_label" if name == "SHA256SUMS" else "zip_label" if name.endswith("-chapters.zip")
                else "pdf_label" if name.endswith(".pdf") else "md_label" if name.endswith(".md") else None)
        size = f.stat().st_size
        size = f"{size / 1e6:.1f} MB" if size >= 1e6 else f"{max(1, round(size / 1e3))} kB"
        label = html.escape(msg[kind]) if kind else html.escape(name)
        return (f'<li><a class="btn small" href="{html.escape(name)}" download>{label} '
                f'<span class="size">{html.escape(name)} · {size}</span></a></li>')

    def render_spec(self, text, lang, version, folder, found):
        """One page of the standard, finished: the site's parts outside the standard's text."""
        if SPEC_MARK not in text.split("<html", 1)[0]:
            return None                                   # not from the site's template, or done already
        text = text.replace(SPEC_MARK, "", 1)
        parts = BODY_RE.split(text)
        if len(parts) != 3:
            self.problems.append(f"{lang['code']}/spec/{version}: the marks of the standard's text are missing")
            return None
        code = lang["code"]
        msg = self.messages(lang)
        outside = DOWNLOAD_RE.sub(lambda m: self.download(folder, msg, m), parts[0] + "{{spec_body}}" + parts[2])
        # The contents list ends each title with the "#" of the heading's link; drop it.
        outside = re.sub(r"#</a>(\s*</?(?:li|ol)>)", r"</a>\1", outside)
        # A language whose pages are not built leads to the default language's.
        home = "../../" if any(l is lang for l in self.languages) else f"../../../{self.default}/"
        versions = sorted((v for v, _ in found.get(code, [])),
                          key=lambda v: [int(x) if x.isdigit() else x for x in v.split(".")])
        menu = [l for l in self.languages if any(v == version for v, _ in found.get(l["code"], []))]
        if not any(l is lang for l in menu):
            menu.append(lang)
        notice = ""
        if code != self.default:
            notice = ('<div class="notice info"><svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">'
                      '<circle cx="8" cy="8" r="6.5" fill="none" stroke="currentColor" stroke-width="1.5"/>'
                      '<path d="M8 7v4.2M8 4.6v.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>'
                      f'<p>{html.escape(self.english["spec_translated"])} '
                      f'<a href="../../../{self.default}/spec/{version}/" lang="en">'
                      f'{html.escape(self.english["spec_english"])}</a></p></div>')
        extra = {
            "home": home,
            "lang_menu": self.lang_menu(lang, msg, lambda o: f"../../../{o['code']}/spec/{version}/", menu),
            "spec_versions": ", ".join(f'<a href="../{v}/">{html.escape(v)}</a>' for v in versions),
            "spec_notice": notice,
            "spec_body": "{{spec_body}}",
        }
        page, done, total = self.render(SPEC_PAGE, outside, lang, extra, ("spec_body",))
        return page.replace("{{spec_body}}", parts[1], 1), done, total

    def redirect(self):
        table = {m: l["code"] for l in self.languages for m in l.get("matches", [l["code"]])}
        return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>TOA</title>
<meta http-equiv="refresh" content="0; url={self.default}/index.html">
<script>
(function () {{
  var table = {json.dumps(table, sort_keys=True)};
  var langs = navigator.languages || [navigator.language || ""];
  for (var i = 0; i < langs.length; i++) {{
    var tag = langs[i], code = table[tag] || table[tag.split("-")[0]];
    if (code) {{ location.replace(code + "/index.html" + location.hash); return; }}
  }}
  location.replace("{self.default}/index.html" + location.hash);
}})();
</script>
</head>
<body><p><a href="{self.default}/index.html">TOA</a></p></body>
</html>
"""

    def build(self, out, check=False):
        report = []
        pages = {}
        for page in PAGES:
            text = (SRC / page).read_bytes().decode("utf-8")
            for lang in self.languages:
                html_text, done, total = self.render(page, text, lang)
                pages[(lang["code"], page)] = html_text
                report.append(f"{lang['code']}/{page}: {done}/{total} texts translated" if lang["code"] != self.default
                              else f"{lang['code']}/{page}: {total} texts")
        # The template of the standard's pages: checked, and counted as a page.
        template = (SRC / SPEC_PAGE).read_bytes().decode("utf-8")
        for lang in self.languages:
            spec = {k: "" for k in SPEC_FIELDS | {"spec_versions", "spec_notice"}}
            _, done, total = self.render(SPEC_PAGE, template, lang, spec, SPEC_FIELDS)
            report.append(f"{lang['code']}/{SPEC_PAGE}: {done}/{total} texts translated" if lang["code"] != self.default
                          else f"{lang['code']}/{SPEC_PAGE}: {total} texts")
        if self.problems:
            raise BuildError("\n".join(dict.fromkeys(self.problems)))
        if check:
            return report
        for (code, page), text in pages.items():
            write(out / code / page, text)
        found = self.spec_pages(out)
        by_code = {l["code"]: l for l in self.all}
        for code, items in found.items():
            for version, path in items:
                done = self.render_spec(path.read_bytes().decode("utf-8"), by_code[code], version, path.parent, found)
                if done is not None:
                    write(path, done[0])
                    report.append(f"{code}/spec/{version}/: finished")
        if self.problems:
            raise BuildError("\n".join(dict.fromkeys(self.problems)))
        write(out / "llms.txt", (SRC / "llms.txt").read_bytes().decode("utf-8"))
        assets = out / "assets"
        assets.mkdir(parents=True, exist_ok=True)
        for f in sorted((SRC / "assets").iterdir()):
            if f.is_file():
                write(assets / f.name, f.read_bytes().decode("utf-8"))
        write(out / "index.html", self.redirect())
        return report


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--out", type=Path, default=ROOT / "site" / "dist")
    p.add_argument("--check", action="store_true")
    p.add_argument("--incubating", action="store_true")
    p.add_argument("--spec-href", default="spec/0.1/")
    p.add_argument("--note")
    p.add_argument("--preview", action="store_true")
    a = p.parse_args(argv)
    site = Site(incubating=a.incubating, spec_href=a.spec_href, note=a.note, preview=a.preview)
    try:
        report = site.build(a.out.resolve(), check=a.check)
    except BuildError as e:
        print(e, file=sys.stderr)
        return 1
    print("\n".join(report))
    if not a.check:
        print(f"written to {a.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
