# scripts/spec

Checks the TOA standard in [`spec/`](../../spec/) and builds its published forms. Adapted from `scripts/spec/` of [rich-netlist/rnl](https://github.com/rich-netlist/rnl) (MIT).

```bash
python scripts/spec/build.py               # check, then build every version into scripts/spec/dist/
python scripts/spec/build.py --check       # the checks only
python scripts/spec/build.py --stamp       # after bringing a translation in line with the English
python scripts/spec/build.py --combine     # write the combined texts again after changing a chapter
python scripts/spec/build.py --site DIR    # also write the web pages into the built site DIR
python scripts/spec/build.py --no-pdf      # skip the PDFs
python scripts/spec/vectors.py --git       # check the test vectors, cross-checking hashes with git
```

Needs Python 3.10 or later (standard library only), git, [pandoc](https://pandoc.org/) 3.x and [Typst](https://typst.app/) 0.13 or later on PATH. `--check`, `--stamp` and `--combine` need only Python and git.

## Input

| Path in `spec/` | Content |
| --- | --- |
| `0.1/en/`, `0.1/zh/` | `front.md`, the chapters `ch01.md` … and the annexes `annex-a.md` …: the source, the only text that is edited. Both languages have the same parts |
| `toa-0.1.md`, `toa-0.1.zh.md` | the combined texts, assembled from the chapters and committed |
| `types/en/<prefix>.md`, `types/zh/<prefix>.md`, `types/<prefix>.schema.json` | the type specifications: text and definition of the fields |
| `vectors/` | the test vectors, written and checked by `vectors.py` |

Every folder named `major.minor` that holds `en/` is a version, and every version is built.

## Output

For each version and language, in `dist/<version>/`, all from the chapters of one commit and with the same revision label:

| File | Form |
| --- | --- |
| `toa-0.1.md` | the combined Markdown of the chapters |
| `toa-0.1.html` | one web page: the chapters and then the type specifications, section anchors such as `#3.2`, a link on every section reference |
| `toa-0.1.pdf` | the same as a PDF: title page, contents, the same links |
| `toa-0.1-chapters.zip` | the chapters, the type specifications and their field definitions as files, with a `SHA256SUMS`; reproducible |
| `SHA256SUMS` | hashes of all of the above |

Chinese files have `.zh` before the extension.

With `--site DIR`, the page of each language goes to `DIR/en/spec/<version>/index.html` and `DIR/zh/spec/<version>/index.html` with its downloads, and `DIR/<lang>/spec/index.html` leads to the newest version, keeping the section of the address.

**The shell of the page** comes from the website: `site/src/spec-page.html`, which the site line provides. `build.py` fills these placeholders in it:

| Placeholder | Value |
| --- | --- |
| `{{body}}` | the text, as HTML: headings `<h2 id="3">`, `<h3 id="3.2">`, `<h4 id="3.2.1">` holding `<span class="sec">3.2</span>`, the title and `<a class="anchor">`; links to sections have the class `ref` |
| `{{toc}}` | the contents: `<li>` for each chapter, with an `<ol>` of its sections |
| `{{chapters}}` | the chapters alone, as `<li>` |
| `{{lang}}`, `{{title}}`, `{{version}}`, `{{revision}}`, `{{other_lang}}` | language code, title, version, revision label, code of the other language |
| `{{downloads}}` | the downloads of the page, as `<li>` |

Until that template exists, `page.html` here stands in for it.

**The revision label** is taken from git: "revision n" at the tag `spec-<version>-r<n>`, "revision n + later changes" after it, "not released" before the first tag, with the date and short hash of the last commit that changed `spec/<version>/` or `spec/types/`, and a note when they have uncommitted changes.

## Checks

Run before anything is built; any failure stops the build:

1. **Structure:** each Chinese part has the headings, list items, table rows and code blocks of its English part, section by section, and the same inline code (spans without spaces). The type specifications likewise.
2. **Stamps:** each Chinese part starts with `<!-- translated from en/ch03.md sha256:… -->`, the first 12 hex digits of the SHA-256 of the English part it was translated from. When the English changes, the check fails until the Chinese is brought in line and `--stamp` is run.
3. **References:** every section cited in the text (`3.2`, `(3.2.1)`, `chapter 7`, `第 7 章`, `Annex A`, `附录 A`) exists.
4. **Combined texts:** `toa-0.1.md` and `toa-0.1.zh.md` are what the chapters give.

pandoc runs with `--fail-if-warnings`, and any output of Typst fails the build. Only the fonts Typst finds are passed to it, so the same build runs on Windows, macOS and CI.

## Writing for the build

- Cite sections by number: `(3.2)`, `3.2.1`, `item 3 of 3.1`, `chapter 7`, `Annex A`; in Chinese `（3.2）`, `3.1 第 3 条`, `第 7 章`, `附录 A`.
- Write version numbers as code (`` `0.1` ``, `` `2.26` ``): a bare `0.1` would be read as a section.
- Do not write other numbers of the form `n.n` in plain text: a numbered step is "the first sub-step of step 3", not "step 3.1".
- Keep inline code the same in both languages; placeholders in code are written in English (`<object name>`).

## Files

| File | Content |
| --- | --- |
| `build.py` | the checks and the build |
| `spectools.py` | parts, structure, stamps, references, combined texts |
| `toa.lua` | the pandoc filter: anchors and links of section references |
| `page.html` | the plain web page, used until the site's template exists |
| `print.typ` | the PDF template (adapted from rnl) |
| `vectors.py` | writes and checks `spec/vectors/` |

`dist/` is ignored by git; `dist/work/` keeps the Typst sources.
