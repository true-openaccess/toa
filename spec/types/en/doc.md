# Type specification `doc`: document

A document is a text about interfaces, circuits, testbenches, records or frozen libraries: a design note, a guide to a testbench, an architecture, a reference. Its body is the `README.md` of the object. The prefix is `doc`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/doc.schema.json`.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `doc` | The type |
| `refs` | may | a list of object names | The objects it is about. They are references (3.3) |
| `date` | may | a date, `'YYYY-MM-DD'`, in quotes | The date the author gives it |

- **`title`** (3.2.1), when not written, is the first heading of `README.md`. What kind of document it is, such as `design-note` or `tb-guide`, may be written among its `tags`.
- Objects the document only names in passing, without needing them found, are named in `README.md`, not in `refs` (3.3).
- A correction to another object is a note beside that object (4.1), not a document.

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| `README.md` | must | The body, in Markdown |
| other files | may | Images and data the body uses, referred to by relative paths |

- Showing a document, a tool must not load resources outside the object on its own (chapter 11).
- **Source lists** do not follow documents further (5.2).

## Checks at sealing

1. `README.md` exists.
2. Every relative link and image of `README.md` points to a file in the object.

## Readable name

`doc/<interface name>/<short name>/`, for example `doc/strongarm-comp/kickback-mitigation/doc_…/` (6.2).

The readable name without the type directory and the slashes at its ends, such as `strongarm-comp/kickback-mitigation`, is the `label` a tool should write for the document in a source list (5.1), and the name it should show for it. `<interface name>` is the `name` of the first interface among `refs`; a document whose `refs` name no interface has no such level, as in `doc/kickback-mitigation/doc_…/`. The short name is not in the manifest: the tool that creates the document takes it from the user. A tool showing a document it did not create uses the `label` of its line in a source list, or else its title.

## Example

```yaml
type: doc
license: CC-BY-4.0
title: Reducing kickback in StrongARM comparators
tags: [design-note, comparator]
refs:
- ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5
date: '2026-10-28'
```
