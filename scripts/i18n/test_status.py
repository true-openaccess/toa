"""Tests of status.py: python -m unittest discover -s scripts/i18n (from toa/)."""
from __future__ import annotations

import contextlib
import hashlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import status  # noqa: E402

REGISTRY = {
    "format": 1,
    "default": "en",
    "languages": [
        {"code": "en", "name": "English", "short": "EN", "html_lang": "en", "matches": ["en"],
         "status": "published", "messages": {"intro": "Introduction", "count": "{n} objects",
                                             "not_translated": "Not yet in {language}."}},
        {"code": "zh", "name": "简体中文", "short": "中文", "html_lang": "zh-CN", "matches": ["zh"],
         "status": "incubating"},
    ],
}
SURFACES = {
    "format": 1,
    "source_language": "en",
    "surfaces": [
        {"id": "spec", "kind": "document", "source": "spec/en/*.md", "translation": "spec/{lang}/{name}",
         "stamp": "en/{name}"},
        {"id": "glossary", "kind": "glossary", "translation": "i18n/{lang}/glossary.json"},
        {"id": "pages", "kind": "inline", "markers": "en-groups", "source": ["site/src/*.html", "site/src/assets/*.js"],
         "base": "site/src", "translation": "i18n/{lang}/site/{path}.json"},
        {"id": "app", "kind": "inline", "markers": "t-calls", "source": "app/*.js", "base": "app",
         "translation": "i18n/{lang}/app/{path}.json"},
        {"id": "messages", "kind": "dictionary", "format": "registry-messages", "source": "i18n/languages.json",
         "translation": "i18n/{lang}/site/messages.json", "keep": ["not_translated"]},
    ],
}
GLOSSARY = {"language": "zh", "terms": [
    {"en": "seal", "term": "定稿", "avoid": ["封存"], "kind": "toa"},
    {"en": "object", "term": "对象", "kind": "toa"},
    {"en": "source list", "term": "来源表", "kind": "toa", "draft": True},
]}


def stamp(en: bytes, name: str) -> str:
    return f"<!-- translated from en/{name} sha256:{hashlib.sha256(en).hexdigest()[:12]} -->\n"


class TreeTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.write("i18n/languages.json", REGISTRY)
        self.write("i18n/surfaces.json", SURFACES)
        self.write("i18n/zh/glossary.json", GLOSSARY)

    def tearDown(self):
        self._tmp.cleanup()

    def write(self, rel: str, content):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(content, (dict, list)):
            content = json.dumps(content, ensure_ascii=False, indent=1)
        path.write_bytes(content.encode("utf-8") if isinstance(content, str) else content)

    def tree(self) -> status.Tree:
        return status.Tree(self.root)

    def result(self, sid: str, lang: str = "zh") -> status.Result:
        t = self.tree()
        return t.result(next(s for s in t.surface_list() if s["id"] == sid), lang)

    def run_main(self, *argv) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = status.main([*argv, "--root", str(self.root)])
        return code, out.getvalue(), err.getvalue()


class Documents(TreeTest):
    def setUp(self):
        super().setUp()
        self.en = {"ch01.md": b"## 1 Overview\n", "ch02.md": b"## 2 Names\n", "ch03.md": b"## 3 Objects\n"}
        for name, data in self.en.items():
            self.write(f"spec/en/{name}", data)

    def test_current_stale_missing(self):
        self.write("spec/zh/ch01.md", stamp(self.en["ch01.md"], "ch01.md") + "## 1 概述\n")
        self.write("spec/zh/ch02.md", stamp(b"old English\n", "ch02.md") + "## 2 名字\n")
        r = self.result("spec")
        self.assertEqual((r.done, r.total), (1, 3))
        self.assertEqual(len(r.stale), 1)
        self.assertIn("spec/en/ch02.md changed", r.stale[0])
        self.assertEqual(r.missing, ["spec/zh/ch03.md"])
        self.assertEqual(r.errors, [])

    def test_not_stamped_is_stale_not_an_error(self):
        self.write("spec/zh/ch01.md", "## 1 概述\n")
        r = self.result("spec")
        self.assertEqual(r.stale, ["spec/zh/ch01.md: not stamped"])
        self.assertEqual(r.errors, [])

    def test_malformed_and_foreign_stamps_are_errors(self):
        self.write("spec/zh/ch01.md", "<!-- translated from en/ch01.md sha256:xyz -->\n## 1\n")
        self.write("spec/zh/ch02.md", stamp(self.en["ch01.md"], "ch01.md") + "## 2\n")
        r = self.result("spec")
        self.assertEqual(len(r.errors), 2)
        self.assertIn("looks like a stamp", r.errors[0])
        self.assertIn("expected en/ch02.md", r.errors[1])

    def test_cr_is_an_error(self):
        self.write("spec/zh/ch01.md", (stamp(self.en["ch01.md"], "ch01.md") + "## 1\r\n").encode())
        self.assertIn("CR", self.result("spec").errors[0])

    def test_translation_without_source_is_unused(self):
        self.write("spec/zh/ch09.md", "## 9\n")
        self.assertEqual(self.result("spec").unused, ["spec/zh/ch09.md: no English source"])


class InlineGroups(TreeTest):
    PAGE = """<h1><en>TOA,  a format
    standard</en></h1>
<p><en>Read the <b>standard</b> {version}</en></p>
<button aria-label="<en>Switch theme</en>"></button>
<span><en ctx=short>Standard</en></span><span><en>Standard</en></span>
<p><en>Read the <b>standard</b> {version}</en></p>
"""

    def setUp(self):
        super().setUp()
        self.write("site/src/index.html", self.PAGE)

    def test_keys(self):
        keys = self.tree()._groups(SURFACES["surfaces"][2], self.root / "site/src/index.html")
        self.assertEqual(keys, ["TOA, a format standard", "Read the <b>standard</b> {version}",
                                "Switch theme", "short|Standard", "Standard"])

    def test_counts_and_unused(self):
        self.write("i18n/zh/site/index.html.json", {
            "TOA, a format standard": "TOA，一种格式标准",
            "Read the <b>standard</b> {version}": "阅读<b>标准</b> {version}",
            "Switch theme": None,
            "short|Standard": "",
            "Gone": "没了",
        })
        r = self.result("pages")
        self.assertEqual((r.done, r.total, r.omitted), (3, 5, 1))
        self.assertEqual(r.missing, ["i18n/zh/site/index.html.json: Switch theme",
                                     "i18n/zh/site/index.html.json: Standard"])
        self.assertEqual(r.unused, ["i18n/zh/site/index.html.json: Gone"])
        self.assertEqual(r.errors, [])

    def test_no_catalog_is_all_missing(self):
        r = self.result("pages")
        self.assertEqual((r.done, r.total, len(r.missing)), (0, 5, 5))
        self.assertEqual(r.files, [("i18n/zh/site/index.html.json", False)])

    def test_format_errors(self):
        self.write("i18n/zh/site/index.html.json", {
            "Read the <b>standard</b> {version}": "阅读标准 {version} {extra}",
        })
        errors = self.result("pages").errors
        self.assertEqual(len(errors), 2)
        self.assertIn("placeholders", errors[0])
        self.assertIn("other elements", errors[1])

    def test_catalog_must_be_texts(self):
        self.write("i18n/zh/site/index.html.json", {"Switch theme": 3})
        self.assertIn("object of texts or null", self.result("pages").errors[0])
        self.write("i18n/zh/site/index.html.json", "{not json")
        self.assertIn("not valid JSON", self.result("pages").errors[0])

    def test_quotes_in_scripts(self):
        self.write("site/src/assets/app.js", "cap: '<en>Open it</en>'\n")
        self.write("i18n/zh/site/assets/app.js.json", {"Open it": "打开'它'"})
        r = self.result("pages")
        self.assertTrue(any("typographic quotes" in e for e in r.errors))

    def test_glossary_avoid_warns(self):
        self.write("site/src/index.html", "<p><en>Seal the drafts</en></p>")
        self.write("i18n/zh/site/index.html.json", {"Seal the drafts": "封存草稿"})
        r = self.result("pages")
        self.assertEqual(r.errors, [])
        self.assertIn("avoids for 'seal'", r.warnings[0])

    def test_no_source_yet(self):
        (self.root / "site/src/index.html").unlink()
        self.assertEqual(self.result("pages").cell(), "no source yet")


class TCalls(TreeTest):
    def test_runtime_texts(self):
        self.write("app/main.js", "el.title = t('Copy the name');\nh(t(\"Object \\\"{0}\\\"\"));\nt('Copy the name')\n")
        self.write("i18n/zh/app/main.js.json", {"Copy the name": "复制名字"})
        r = self.result("app")
        self.assertEqual((r.done, r.total), (1, 2))
        self.assertEqual(r.missing, ['i18n/zh/app/main.js.json: Object "{0}"'])


class Messages(TreeTest):
    def test_keep_is_not_counted(self):
        self.write("i18n/zh/site/messages.json", {"intro": "介绍", "not_translated": "尚无", "old": "旧"})
        r = self.result("messages")
        self.assertEqual((r.done, r.total), (1, 2))
        self.assertEqual(r.missing, ["i18n/zh/site/messages.json: count"])
        self.assertEqual(r.unused, ["i18n/zh/site/messages.json: old"])
        self.assertIn("stays in English", r.warnings[0])

    def test_placeholder_error(self):
        self.write("i18n/zh/site/messages.json", {"count": "{m} 个对象"})
        self.assertIn("placeholders", self.result("messages").errors[0])


class Registries(TreeTest):
    def test_real_tree_has_no_format_errors(self):
        o, e = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(o), contextlib.redirect_stderr(e):
            code = status.main(["check"])
        self.assertEqual(code, 0, e.getvalue())

    def test_messages_in_a_translation_entry(self):
        reg = json.loads(json.dumps(REGISTRY))
        reg["languages"][1]["messages"] = {"intro": "介绍"}
        self.write("i18n/languages.json", reg)
        self.assertTrue(any("not in the registry" in e for e in self.tree().errors))

    def test_registry_errors(self):
        reg = json.loads(json.dumps(REGISTRY))
        reg["default"] = "fr"
        reg["languages"][1]["status"] = "maybe"
        reg["languages"][1]["colour"] = "red"
        reg["languages"].append(dict(reg["languages"][0]))
        self.write("i18n/languages.json", reg)
        errors = " | ".join(self.tree().errors)
        for part in ("default 'fr'", "status must be", "unknown field 'colour'", "codes listed twice"):
            self.assertIn(part, errors)

    def test_surface_errors(self):
        sur = json.loads(json.dumps(SURFACES))
        sur["surfaces"][0].pop("stamp")
        sur["surfaces"][2].pop("base")
        sur["surfaces"].append({"id": "x", "kind": "poem", "translation": "x"})
        self.write("i18n/surfaces.json", sur)
        errors = " | ".join(self.tree().errors)
        for part in ("needs stamp", "needs base", "kind must be one of"):
            self.assertIn(part, errors)


class Glossaries(TreeTest):
    def test_counts_and_drafts(self):
        r = self.result("glossary")
        self.assertEqual((r.done, r.total), (2, 3))
        self.assertEqual(r.cell(), "3 terms (1 drafts)")

    def test_errors(self):
        self.write("i18n/zh/glossary.json", {"language": "ja", "terms": [
            {"en": "seal", "term": "定稿", "kind": "magic"},
            {"en": "seal", "term": "封"},
            {"en": "object", "term": ""},
        ]})
        errors = " | ".join(self.result("glossary").errors)
        for part in ("language must be 'zh'", "kind must be one of", "listed 2 times", "term must be"):
            self.assertIn(part, errors)

    def test_none(self):
        (self.root / "i18n/zh/glossary.json").unlink()
        self.assertEqual(self.result("glossary").cell(), "none")


class CommandLine(TreeTest):
    def test_report_check_files_json(self):
        self.write("spec/en/ch01.md", "## 1\n")
        code, out, _ = self.run_main()
        self.assertEqual(code, 0)
        self.assertIn("spec", out)
        self.assertIn("0/1", out)

        code, out, _ = self.run_main("files", "zh")
        self.assertIn("write  spec/zh/ch01.md", out)

        code, out, _ = self.run_main("--json")
        data = json.loads(out)
        self.assertEqual(data["surfaces"]["spec"]["zh"]["missing"], ["spec/zh/ch01.md"])

        code, out, _ = self.run_main("--markdown")
        self.assertTrue(out.startswith("| surface | zh |"))

        self.assertEqual(self.run_main("check")[0], 0)
        self.write("spec/zh/ch01.md", "<!-- translated from en/ch01.md sha256:zz -->\n## 1\n")
        code, _, err = self.run_main("check")
        self.assertEqual(code, 1)
        self.assertIn("1 format errors", err)

    def test_missing_and_stale_do_not_fail(self):
        self.write("spec/en/ch01.md", "## 1\n")
        self.write("spec/zh/ch01.md", stamp(b"old", "ch01.md") + "## 1\n")
        self.assertEqual(self.run_main("check")[0], 0)


if __name__ == "__main__":
    unittest.main()
