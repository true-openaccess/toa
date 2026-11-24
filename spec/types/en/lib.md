# Type specification `lib`: frozen library

A frozen library is a set of objects frozen as an object: a library as it was at one moment, such as one release of it, or a selection of objects from several libraries. It holds a list of its members in the format of a source list, and since it is an object, that list never changes. The prefix is `lib`. This specification needs the common rules of TOA `0.1` or later (chapters 2 to 8); the definition of its fields is `types/lib.schema.json`.

- In this standard a library is still the git repository that holds objects (6.1). A frozen library is an object, kept in libraries like any other.

## Fields

Besides the common fields (3.2.1):

| Field | Requirement | Value | Meaning |
|---|---|---|---|
| `type` | must | `lib` | The type |
| `name` | must | `[a-z][a-z0-9]{0,39}(-[a-z0-9]+)*` | The name of the frozen library, such as `toa-official`. Its releases share it, and it marks their series (3.4) |

- **`title`** (3.2.1) says which release it is, such as `TOA official library, 2026-12`.
- **The next release** writes `base` with the previous one. A library marks the latest release `latest` (5.2).
- A frozen library names no process and declares no external files (3.5).

## Files

| File | Requirement | Content |
|---|---|---|
| `toa.yaml` | must | The manifest |
| `members.source.jsonl` | must | The list of members, a source list (chapter 5) |
| `README.md` | may | Notes on the release |

## The list of members

`members.source.jsonl` keeps the rules that 3.3 sets for this file in any object:

- Its header is `{"toa": "source/1"}` and nothing more. Every other line is an object line, with no include lines.
- An object line may have `object`, `label`, `refs`, `license`, `group` and `sources`; not `removed` and not `latest`. Its sources are only `git` sources, never `local`.
- It is read strictly, as the lists on a publication branch are (5.1), within the limits of an object (3.1) instead of those of a source list: at most 10 MiB. A line is still at most 64 KiB.
- **The `object` of every line is a reference** (3.3), and must be found like any other. The names in the cached fields, such as `refs`, are not references.

A frozen library adds:

- **Its sources are only hints:** when an address no longer works, a tool looks for the object elsewhere (7.3).
- **It should list everything:** the objects its members need (5.2) should each have a line of their own, with their sources, so that whoever fetches the frozen library need not look elsewhere. A tool that writes a frozen library lists them by default, and should tell the user when a list it reads does not.
- A frozen library may be a member of another, such as an official release made of several parts. A document may refer to it in `refs`, such as the notes of a release.

## Using it

- **A workspace list may include the list of members directly,** by an include line whose `include` is its path in a library (5.1, 8.1). Groups and readable names are shown as for any source list.
- **Fetching a frozen library** means fetching it, its members and the objects they need. The source list beside it lists them (5.2).
- **The library list and a frozen library differ:** the library list changes, as objects are added, removed and marked; the list of members never changes, and only says which objects there were at one moment, and where to find them.
- **A test plan** is a frozen library whose members are an interface and the testbenches that measure it, with circuits when needed. There is no type of its own: each testbench can be run alone, so the conditions they share are written in the presets of each.
- Unlike a git tag, a frozen library is an object: it does not change once sealed, other objects can refer to it, it can take members from several libraries, and it does not depend on any one repository still being there.
- **Source lists** follow frozen libraries further, through the objects of `members.source.jsonl` (5.2).

## Checks at sealing

1. `members.source.jsonl` exists and keeps the rules above.
2. No object name is on two lines.
3. When the members can be found, a tool should tell the user of the objects they need that are not listed.

Sealing replaces the draft names in `members.source.jsonl` as it does in `toa.yaml` and `lock.json`, in an object of any type (7.1).

## Readable name

`lib/<name>.g<generation>/`, for example `lib/toa-official.g3/lib_…/` (6.2). The generation is the one in its `history.json` (3.4). A frozen library has no interface level.

The readable name without the type directory and the slashes at its ends, `<name>.g<generation>` such as `toa-official.g3`, is the `label` a tool should write for the frozen library in a source list (5.1), and the name it should show for it.

## Example

```yaml
type: lib
license: CC0-1.0
name: toa-official
title: TOA official library, 2026-12
base:
- lib_3b9e5c1d7a2f4e6b8c0d9a1e3f5b7c9d2e4f6a8b
```

and the first lines of its `members.source.jsonl`:

```text
{"toa": "source/1"}
{"object": "ckt_1ec6249c0d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a", "label": "strongarm-comp/wrap-lee.g2", "group": "strongarm-comp", "sources": [{"git": "https://example.org/toa-lib.git"}]}
{"object": "ifc_945cdda7d9b4c0f6a2e1d3b5c7a9e0f1d2c3b4a5", "label": "strongarm-comp", "group": "strongarm-comp", "sources": [{"git": "https://example.org/toa-lib.git"}]}
```
