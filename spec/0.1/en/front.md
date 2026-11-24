# TOA library format standard, version 0.1

TOA (true open access) is a format standard for design libraries. Objects and references are plain files: naming an object gives the same value as git's tree hash, but needs no git. Version `0.1` publishes and shares objects through git repositories, so the same library works online and offline, in public and in private. Every object is named by the hash of its content, so a name always stands for the same content. This document is the standard of version `0.1`.

- The English text is normative. The Chinese version, `toa-0.1.zh.md`, is a translation; where the two differ, the English text prevails. The English terms are the only normative terminology; each translation keeps its own glossary.
- The type specifications in `types/` are part of this standard: each defines one type of object (chapter 9).
- The test vectors in `vectors/` are informative. Where they and this text differ, this text prevails (1.6).
- Versions `0.x` make no promise of compatibility with each other (chapter 10).
- Any tool, website or EDA integration may implement this standard. A reference tool, `toa`, is published with it.
