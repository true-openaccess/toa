# i18n

The start for anyone who translates TOA, or maintains one of its languages: what there is to translate, how far each language is, and where a translation goes.

English is the only core language of TOA. The English text of the standard is normative, and its English terms are the only normative ones. Every other language is a translation that follows the English; wherever a translation has no text, the English is shown. A missing or stale translation is reported, and never fails a check or a build. The approach is that of [rnl](https://github.com/rich-netlist/rnl/tree/main/i18n), simplified.

| File | Content |
| --- | --- |
| [`languages.json`](languages.json) | The one list of languages: code, native name, HTML language, how browsers' languages match it, fonts, `published` or `incubating`, and the English of the site's shared messages |
| [`surfaces.json`](surfaces.json) | Every kind of text there is to translate, a *surface*: where its English is, where each language's translation goes, who keeps it; and what is not translated, with the reason |
| [`../scripts/i18n/status.py`](../scripts/i18n/status.py) | The report: how far each language is in each surface, what is missing, stale or unused |
| `<lang>/` | The files of one language: its glossary and the catalogs of the site |

## How far each language is

```bash
python scripts/i18n/status.py            # one row per surface, one column per language
python scripts/i18n/status.py --lang zh  # one language in detail
python scripts/i18n/status.py files zh   # its files, present or to be written
python scripts/i18n/status.py check      # exit 1 on a format error
```

`--markdown` and `--json` give the table in those forms. The tests are `python -m unittest discover -s scripts/i18n`.

## Where a translation goes

| What | Where | Example |
| --- | --- | --- |
| The standard's chapters and the type specifications | beside the English, in a folder of the language | `spec/0.1/zh/ch02.md`, `spec/types/zh/ckt.md` |
| The glossary | `i18n/<lang>/glossary.json` | `i18n/zh/glossary.json` |
| The site's pages and scripts | `i18n/<lang>/site/<path under site/src/>.json` | `i18n/zh/site/index.html.json` |
| The site's shared messages | `i18n/<lang>/site/messages.json`, keyed by the names of the messages | `{"intro": "介绍"}` |

**A translated document** starts with the part it was translated from and the start of that part's SHA-256: `<!-- translated from en/ch02.md sha256:7681d4f444b8 -->`. `python scripts/spec/build.py --stamp` writes it once a translation is in line with its English.

**A catalog** is a JSON object keyed by the English text, with its runs of white space as one space. A text is the translation; `null` is one not made yet, shown in English; `""` is one left out on purpose. A translation keeps the placeholders of its English, such as `{version}`, adds none, and opens and closes the same elements; in a script it has no more straight quotes and backslashes than its English. In the site's sources the English stands in groups, `<en>…</en>`, and a group that names a context, `<en ctx=short>Standard</en>`, has the key `short|Standard`. An application that translates while it runs, such as Gazettoa, marks its texts as calls, `t('Copy the name')`, and keeps catalogs in the same format (`"markers": "t-calls"` in its `surfaces.json`).

**A glossary** lists how the language renders TOA's English terms: `en` and `term` are required; `sense` tells apart the senses of one English term; `avoid` lists renderings not to use, which the report flags in the site's texts; `kind` is `toa`, `eda` or `general`; `note` is optional; a term not settled yet says `"draft": true`.

## When a language is shown

A new language is registered as `incubating`: its pages are built for a preview and offered in no menu. It becomes `published` when the site's navigation, home page and page for agents are wholly translated, its other pages at least 80%, and the reviewer named in its entry has read it. A page under 80% translated is shown in English; between 80% and 100%, translated with English where a text is missing. Either way the page says so, in English, as every notice of the system is.
