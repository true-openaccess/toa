# TOA (true open access)

> **Preview.** This is a preview of TOA for demonstrations, not a release: there is no released version yet, and anything here may still change.

TOA is a format standard for design libraries. Libraries, objects and references are plain files and directories, so the same library works online and offline, in public and in private, and can be kept in git or shared in other ways. Every object is named by the hash of its content, so a name always stands for the same content.

**Status:** under development; nothing has been released yet. The standard, TOA 0.1, is being written in English, which is normative, with a Chinese translation. The reference tool `toa` and the project website are being built.

## Repository map

| Path | Content |
| --- | --- |
| `spec/` | The standard: TOA 0.1 by chapter in English and Chinese, the type specifications and the test vectors |
| `crates/` | The reference tool `toa`, a single executable for Windows, Linux and macOS |
| `tests/` | Conformance tests: valid and invalid objects and libraries |
| `examples/` | Small sample libraries |
| `site/` | The project website |
| `i18n/` | Translations; English is the only core language |
| `scripts/` | Build and check scripts |

## Licenses

Code is under MIT. The specification text and the documentation (`spec/`, `docs/`) are under CC-BY-4.0. Data (`spec/vectors/`, `tests/`, `examples/`) is under CC0-1.0. [`REUSE.toml`](REUSE.toml) gives the mapping and [`LICENSES/`](LICENSES/) the full texts.
