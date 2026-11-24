# The index of `toa`, format 2

Status: implemented in `toa` 0.1.0 (`toa index`, `toa cat`), 2026-11-23; format 2 since
2026-11-24 (the type and field names of the standard's decisions B32 and B33). The format may still
change before the tool's first release; readers check the version.

Format 2 against format 1: `based_on` and `based_on_by` are `base` and `base_by`; `history` has
`generation` and `generation_state`; each location has `latest`; `newer` is found from the `latest`
marks of library lists and its items have `marked_by` and `ifc_changed`; the members of a `lib`
object are among its references; the prefixes are those of 0.1 (`ifc`, `ckt`, `tb`, `evd`, `doc`,
`lib`). Since 2026-11-24, without a change of the version (a reader that does not know the
keys passes them over): `locations[].first_signature`, `first.signature` and `locations[].stamps`.

The standard leaves the layout of an index to the tool that writes it (TOA 0.1, 8.3 and Annex B).
This is the format of the reference tool `toa`. It is the tool's own format: it is versioned, and
it may change between versions of the tool; a reader checks the version first.

Section numbers such as (8.3) refer to the standard, `spec/toa-0.1.md`.

## 1 What the index is

`toa index` reads every library that a workspace list includes (8.1), and writes what it found:

- every object directory of every library, verified from git's listing and with the checks of its
  manifest (7.4), and the cross-check hash of its files (2.2);
- where each object is: library, path, and the first commit of the publication branch that held
  it, with its committer date (8.2, 8.3);
- what each library says about it: notes (4.1), removal records (4.2), source lists (5);
- the direct relations, both ways: references and who refers, `base` and who builds on it,
  the verified and the declared part of `history.json` and the generation, the `latest` marks of
  the library lists, and the newer versions they give (3.4, 5.2, 7.5, 8.3).

It is derived data: when lost, it can be built again (8.2).

## 2 Layout

```text
<out>/
  index.json                   the root: format version, the libraries, counts, alarms
  libraries/<key>.json         one per library
  objects/<hh>/<name>.json     one per object name; <hh> is the first two hex digits of its hash
  problems.jsonl               every finding, one JSON object per line (section 8)
  state.json                   what an incremental run needs (section 9); readers ignore it
```

- **Resolving one name reads one small file** (8.3): `objects/<hh>/<name>.json`, where `<hh>` is
  taken from the 40 hex digits after the underscore. A name the index does not know has no file.
- Every file is UTF-8 JSON without a byte order mark, ending with LF. Keys are written in the order
  this document gives. Paths inside the index use `/`.
- **No local paths** (8.3): the index holds library paths relative to the root of each library,
  and `git` addresses as the workspace list writes them, never a path on the machine that built it.
  A library that is only on the machine (a `local` source) appears with its key and without an
  address.

## 3 `index.json`

```json
{
  "toa-index": 2,
  "tool": "toa 0.1.0",
  "standard": "0.1",
  "built": "2026-11-23T12:00:00Z",
  "libraries": [
    {"key": "official", "address": "https://github.com/example/lib", "branch": "main",
     "commit": "<40 hex>", "date": "2026-11-20T09:15:00+08:00", "objects": 120, "readable": true}
  ],
  "counts": {"objects": 431, "valid": 425, "names": 450},
  "alarms": [
    {"kind": "sha256-conflict", "name": "ckt_…", "places": [{"library": "a", "path": "…", "sha256": "…"}]}
  ],
  "same_files": {"min_size": 256, "common_groups": 5, "common_authors": 5, "common_cap": 200, "listed": 50}
}
```

- `toa-index` is the format version. A reader that does not know the version stops.
- `built` is UTC. Two builds from the same commits differ only in `built`.
- **Dates** of commits are ISO 8601 with the committer's own offset, as git gives them (`%cI`):
  committers write their own dates, and the index records them as they are (8.3).
- `alarms` lists what must be looked at by people: one name with two different cross-check hashes
  in two places (8.3, chapter 11).
- `same_files`: the numbers this tool chose for the same files of 8.3 (the `same_files` of each
  object, section 5): files under `min_size` bytes are not compared; a file is common when the
  objects holding it fall into more than `common_groups` groups or are of more than
  `common_authors` authors, or are `common_cap` or more; `listed` objects at most each way per
  object.

## 4 `libraries/<key>.json`

```json
{
  "key": "official",
  "groups": ["official"],
  "address": "https://github.com/example/lib",
  "branch": "main",
  "commit": "<40 hex>",
  "date": "2026-11-20T09:15:00+08:00",
  "previous": {"commit": "<40 hex>", "ancestor": true},
  "readable": true,
  "library_yaml": {"toa": "library/1", "name": "…", "title": "…"},
  "includes": [
    {"from": "library.source.jsonl", "include": "library.source.jsonl", "branch": "main",
     "sources": [{"git": "https://…"}]}
  ],
  "objects": ["ckt_…", "ifc_…"],
  "removed": ["ckt_…"],
  "problems": [{"level": "error", "code": "E_LIB_ATTRIBUTES", "section": "6.1", "path": ".gitattributes", "message": "…"}]
}
```

- **The key** is the `group` of the include line that brings the library in, joined with `/` when
  it is included through several levels (`libraries/<a>/<b>.json`); when a line has no group, the
  key is the host and path of the address with every character other than `[a-z0-9-]` made `-`;
  when two libraries would get the same key, the second gets the same form of its branch
  appended when the first is another branch of the same repository, else that form of its
  address, then `-2`, `-3`, … while still taken. Keys keep 3.1 item 3 for every part, so they can
  be file names. A key names a record within one index; across runs a library is known by its
  `address` and `branch`.
- **One library, one record:** a library is an address and a publication branch. Two include
  lines with the same address (5.1, "two addresses are the same") and the same `branch` are one
  library; `groups` lists every group it is reached by. Two branches of one repository are two
  libraries, each with its own record, objects and commit.
- `commit` is the head of the publication branch that was read (6.3), `date` its committer date.
- `previous` is the commit read by the previous run, and whether it is an ancestor of `commit`.
  When it is not, history was rewritten: the run says so and reads the whole library again (7.5).
- `readable: false`, with the reason in `problems`, for a library that could not be fetched or
  read; everything else goes on (8.3). The rest of the record is then the previous run's, if any.
- `library_yaml` is `library.yaml` read by the rules of 3.2.3 and written as in section 6.
- `includes` are the include lines of the library list and its volumes that point to other
  libraries (those with `sources`); include lines of volumes are left out.
- `objects` are the object names whose directories are in `commit` (6.3: published in this
  library), sorted; `removed` the names that have only a removal record here.
- `problems` are those of the library check (6.4) that are about the library, not about one
  object: `.gitattributes`, `.gitmodules`, modes outside objects, names outside objects, source
  lists, notes. The problems of an object are in its own file.

## 5 `objects/<hh>/<name>.json`

One file per name the index knows: every object found in a library, every name with a removal
record, and every name that something refers to.

```json
{
  "toa-index": 2,
  "name": "ckt_1ec6249c…",
  "prefix": "ckt",
  "present": true,
  "valid": true,
  "problems": [],
  "sha256": "<64 hex>",
  "manifest": {"type": "ckt", "license": "MIT", "ifc": "ifc_945c…", "name": "wrap-lee", "form": "composition",
               "title": "StrongARM comparator wrapping Lee's latch"},
  "title": "StrongARM comparator wrapping Lee's latch",
  "references": [{"name": "ifc_945c…", "at": "toa.yaml:ifc"}, {"name": "ckt_f1ed…", "at": "toa.yaml:uses/dp/ckt"}],
  "base": ["ckt_cd15…"],
  "history": {
    "listed": ["ckt_cd15…"],
    "verified": ["ckt_cd15…"],
    "declared": [],
    "nearest": null,
    "state": "verified",
    "generation": 2,
    "generation_state": "verified"
  },
  "files": [{"path": "toa.yaml", "size": 412, "blob": "<40 hex>", "sha256": "<64 hex>"}],
  "locations": [
    {"library": "official", "path": "ckt/strongarm-comp/wrap-lee.g2/ckt_1ec6…",
     "first_commit": "<40 hex>", "first_date": "2026-11-03T12:00:00+08:00", "first_signature": "ssh",
     "notes": [{"file": "20261103-clk.md", "commit": "<40 hex>", "date": "…", "size": 812, "text": "…"}],
     "stamps": [{"file": "20261124-btc.ots", "kind": "opentimestamps", "size": 1210, "commit": "<40 hex>", "date": "…"}],
     "source_list": "ckt/strongarm-comp/wrap-lee.g2/ckt_1ec6….source.jsonl",
     "label": "strongarm-comp/wrap-lee.g2",
     "latest": true}
  ],
  "first": {"library": "official", "commit": "<40 hex>", "date": "2026-11-03T12:00:00+08:00", "signature": "ssh"},
  "removals": [
    {"library": "fork", "path": "ckt/…/ckt_1ec6….removed.json", "commit": "<40 hex>", "date": "…",
     "valid": true, "record": {"toa": "removed/1", "object": "ckt_1ec6…", "commit": "…", "path": "…"}}
  ],
  "referenced_by": [{"name": "evd_b7d7…", "at": "lock.json:tree/0/object"}],
  "base_by": ["ckt_77aa…"],
  "newer": [{"name": "ckt_77aa…", "marked_by": ["official"], "together": ["official"], "ifc_changed": false}],
  "same_files": {
    "earlier": [{"object": "ckt_5b0e…", "author": "Alice Lee", "basis": "library", "library": "official",
                 "files": [["netlist/core.scs", "netlist/latch.scs"]]}],
    "later": [],
    "common": ["netlist/models.inc"]
  }
}
```

Field by field:

- `present`: whether an object directory of this name is in some library. `false` for a name known
  only from references or removal records; such a file has only `name`, `prefix`, `present`,
  `history` (`{"listed": …}` from a valid removal record, else `null`), `removals`,
  `referenced_by`, `base_by` and `newer`.
- `valid` and `problems`: verifying from the listing (7.4) and the checks of the manifest,
  `lock.json` and `history.json` (6.4 items 2 and 4), plus the lineage across objects (8.3). When
  the copies in several places differ in what they find (they cannot, unless a copy is not the
  object), `problems` holds each place's own with its path. Problems use the form of section 8.
- `sha256`: the cross-check hash of 2.2, for a valid object. Two places giving two values raise an
  alarm (section 3) and `sha256` is then `null`, with each value in `problems`.
- `manifest`: `toa.yaml` read by 3.2.3, written as JSON (section 6). `null` when it cannot be read.
- `title`: the title to show. The manifest's `title` (3.2.1) when it has one; for a `doc` without
  one, the first heading of its `README.md`, as its type specification says (`types/doc`); absent
  otherwise, and absent when the manifest cannot be read. The first heading is found by CommonMark:
  an ATX heading (`#` to `######` and a space) or a setext heading (a paragraph underlined with
  `=` or `-`), not inside a fenced code block nor a YAML front matter at the top; its text trimmed,
  without the closing `#`s, its lines joined by a space, cut to 200 characters; an empty heading is
  passed over. Inline Markdown in it (`*`, `` ` ``, links) is left as written.
- `references`: every reference (3.3) of `toa.yaml`, of `lock.json` and of `members.source.jsonl`,
  in the order they are written, with `at` = `<file>:<keys and indexes from the top>`; for the
  members list, `members.source.jsonl:<line number>/object`, the line counted from 1 with the
  header. The names of `base` are among them (`toa.yaml:base/0`). (There is no `mentions` any
  more: a name meant only as a mention is written in the object's `README.md`, which is not
  searched.)
- `history`, the two parts of 8.3:
  - `listed`: `history.json` as written;
  - `verified`: from the start, the links that can be followed along the first items of
    `base`, each link found in the index (or as a removal record, checked by 4.2) and with a
    `history` that agrees;
  - `declared`: what comes after;
  - `nearest`: when the first item of `base` cannot be found, the nearest name in `listed`
    that can be, marked as declared by the object itself with a gap. Only for people; reverse
    lookups and `newer` do not use it (8.3);
  - `state`: `none` (no `base`), `verified` (all of `listed` verified), `partial` (a verified
    start, then declared), `gap` (the first link cannot be found), or `invalid` (a found link
    disagrees; the object is then invalid, with a problem);
  - `generation`: the generation of 3.4, as `history.json` writes it; 1 for an object without
    `base`;
  - `generation_state`: `verified` when the first item of `base` is an object found in the index
    and the generation is the one 3.4 gives from it (its generation plus one when the two are of
    one series, the same type and the same `name`; else 1), and for an object without `base`;
    `declared` when that object is not found (a removal record does not give a generation);
    `invalid` when it disagrees, and the object is then invalid, with a problem.
- `files`: every file of the object, sorted by path, with its size, git blob id and SHA-256 (the
  SHA-256 of `lock.json` and the viewer's lists use the same value).
- `locations`: every directory of this object in every library, sorted by library key and path:
  - `first_commit` and `first_date`: the earliest commit of the publication branch, counting along
    first parents, at which this path held the object, and its committer date (8.3). Committers
    write their own dates; the index records them as they are.
  - `first_signature`: the kind of signature of `first_commit` (6.3, 8.3): `ssh`, `openpgp`,
    `x509` or `other`, read from the armour of its `gpgsig` header; `null` when it is not signed.
    The signature is **not verified**, and the signer is not named: verifying needs the keys
    allowed for each signer (`gpg.ssh.allowedSignersFile`, a keyring), which are the reader's
    (8.3: the index should record who signed and whether it verified; a platform that verifies
    does so itself). A signed commit shows who put the object into that library, not who wrote it.
  - `notes`: the notes beside this copy (4.1), each with the commit that added the file, its date
    and size. `text` is the note as written when it is at most 64 KiB, else `null` (a note is at
    most 256 KiB; the reader fetches it by library, commit and path). Notes are untrusted text of
    others (chapter 11).
  - `stamps`: the timestamp proofs beside this copy (4.3), the files placed directly in its
    `.stamps/` directory, sorted by name: the file name, its `kind` by the extension
    (`opentimestamps` for `.ots`, `rfc3161` for `.tsr`, `other` otherwise), its size, and the
    commit that first added a file of that name, with its date; a proof replaced later by its
    completed form (4.3) keeps that commit. **The proofs are not checked**: a reader that shows
    them checks a proof itself against the service it names, or says it did not (4.3, chapter 11).
  - `source_list`: the path of the source list beside this copy, or `null`.
  - `label`: the `label` of this object's line in the library list or the volumes it includes
    (within 8 levels; the first line of the object wins), else of the object's own line in the
    source list beside this copy, else `null`. It is free text of others, at most 256 characters
    (5.1): shown, never used to find anything or to infer anything from.
  - `latest`: whether a line of this object in the library list or its volumes is marked
    `"latest": true` (5.2). A mark is read only there, never in a source list beside an object.
- `first`: the earliest of all `locations`, over all libraries, comparing the dates as instants,
  with the `signature` of that commit as in `first_signature`.
- `removals`: every removal record of this name in every library, with its path, the commit that
  added it and its date, whether it keeps 4.2 and is true, and its fields as written.
- `referenced_by`, `base_by`: the reverse lookups of 8.2, from valid objects only, with the
  field path of the reference. `base_by` uses only the verified part (8.3): it lists the
  objects whose first link of `history` to this one is verified. The items of `base` are not in
  `referenced_by`.
- `newer`: the newer versions of 7.5: valid objects N that some library marks `latest`, whose
  verified `history` holds this object, and that are of the same series as this object (the same
  type and the same `name`; for a type without `name`, the lineage alone). Each with the libraries
  that mark it (`marked_by`), the libraries in which both are published (`together`), and whether
  its `ifc` differs from this object's (`ifc_changed`: the newer version has another interface).
  Only facts; the index does not judge who made N (7.5).
- `same_files`: the same files without a declared lineage (8.3), over all the libraries of the
  index; only for a present object. A hint for people, never a judgement and never a problem: the
  index computes it once so that every reader shows the same, and `toa check` gives the same
  hints (`I_SAME_FILES`, info) from the same code.
  - `earlier`: the objects P earlier than this one O that hold a file byte for byte the same (the
    same git blob) as a file of O, where the two `author`s differ (compared with the white space
    at the ends removed, each run of white space inside as one space, and the case of letters
    ignored; only one written: they differ; neither written: left out; the `authors` of a
    circuit's `origin` take no part), and neither is in the `base` or the `history` of the other
    nor refers to the other (`references`). Earliest first: by the date of the commit that first
    held each, then along first parents, then by name; the first is the earliest P of 8.3. Each
    with its `author` as written (`null` when none), the `files` as pairs `[path in O, path in
    P]`, and how P is known to be earlier (`basis`):
    - `library`: a library holds both, and the first such library in the order of `index.json`
      (`library`) first held P at an earlier commit along first parents of its publication
      branch than O;
    - `dates`: no library holds both; the earliest `first_date` of P is before that of O. Those
      dates are written by the committers.
    When the order cannot be told (the same commit first held both, a date missing, or the library
    holding both held O first), the pair is left out.
  - `later`: the same seen from P: the objects that hold its files later, with the pairs as
    `[path in P, path in O]`.
  - `common`: the paths of this object's files that are common files, not compared: held by
    objects that fall into more than 5 groups (two objects related as above, by lineage or by a
    reference either way, are in one group), or that are of more than 5 authors (no `author`
    counts as one more), or by 200 objects or more (`index.json` `same_files`). At most 5 holders:
    never common. The generations and ports of one design are one group, so they do not make a
    file common.
  - Not compared at all: licence texts (names starting with `LICENSE` or `COPYING`, files under a
    directory `LICENSES/`), `history.json` and `lock.json`, files under 256 bytes, and a file of
    O that an object of O's lineage found in the index holds too.
  - `earlier` and `later` hold at most 50 objects each; `earlier_more` and `later_more`, present
    only then, count the rest.

## 6 Manifests as JSON

The values of `toa.yaml` are written by the table of 3.2.3 rule 9:

| In the manifest | In the index |
|---|---|
| null | `null` |
| boolean | `true`, `false` |
| integer | a JSON integer |
| finite floating-point number | a JSON number with a fraction or an exponent (`1.0`, `2.5e-9`) |
| `.inf`, `-.inf`, `.nan` | `{"float": ".inf"}`, `{"float": "-.inf"}`, `{"float": ".nan"}` |
| string (plain, quoted or block) | a JSON string |
| sequence | an array |
| mapping | an object, keys in the order written |

The style a value was written in (quoted or not) is not kept. A reader needs no YAML library.

## 7 What is read, and how

- **Libraries are never checked out** (7.3). An include line with a `git` source is fetched into
  the tool's cache (a bare repository) with the steps of 7.3: `fetch.fsckObjects`, only `https` and
  `ssh`, no redirects, a timeout, no remote left behind. The publication branch is the `branch` of
  the include line, `main` when not written (6.3). The whole first-parent history of the branch is
  fetched, because dates need it (8.3); later runs fetch only what is new.
- An include line with a `local` source is a library on this machine; its own git store is read,
  with the publication branch decided as 6.3 says (the remote-tracking ref first).
- **Objects are taken in from the listing** (7.4, last row of the table): the stored tree hash equals
  the name, the rebuilt canonical tree has it too, the tree is well-formed; then `toa.yaml`,
  `lock.json` and `history.json` are read and checked. Every file is read once to compute the
  cross-check hash and the SHA-256 of each file. Sizes are looked at before anything is read.
- **The library list is not trusted for objects** (8.2): objects are found in the listing. The list
  is read for its include lines only.
- **Includes:** the include lines of the workspace list that have sources are libraries. Include
  lines without sources, and object lines, are not followed: the index is of libraries. Include
  lines of the lists of libraries (to other libraries) are followed only with `--follow-includes`
  (section 10), and only to hosts allowed without asking.
- **Dates:** one `git log --first-parent -m --diff-filter=A --name-only` per library over the
  paths ending in `/toa.yaml` gives the commit at which each object path first appeared.
- **An object or a library that cannot be read is recorded alone** (8.3); the rest goes on.

## 8 `problems.jsonl`

One line per finding, in the form every `toa` command prints with `--json`, with the place added:

```json
{"kind": "diag", "level": "error", "code": "E_LIB_TREE_FORM", "section": "7.4", "library": "official", "object": "ckt_…", "path": "ckt/…/ckt_…", "message": "…"}
```

`library` and `object` are present when known. Paths are relative to the root of the library.

## 9 Incremental runs

`toa index --out <dir>` reads `<dir>/state.json` when it is there:

```json
{"toa-index": 2, "libraries": {"official": {"address": "…", "branch": "main", "commit": "<40 hex>"}}}
```

- What was read is reused for a library only when the record under its key has the same
  address and branch; otherwise the library is read as new.
- A library whose head did not move is not read again.
- When the old commit is an ancestor of the new one, only the commits after it are read for dates,
  and only objects new to the library are read in full. **An object is read in full once per
  library:** its name fixes its files, so what was computed for a name in that library's store
  (manifest, files, hashes) is kept. The library check (6.4), locations, notes, removal records and
  the reverse lookups are computed again on every new commit; they read only the listing and small
  files. The same files of 8.3 (`same_files`) are computed again over all libraries on every run,
  from the files kept per object and the order of commits along first parents kept per library.
- When the old commit is not an ancestor, the run says so (exit code stays 0, a warning
  `W_INDEX_REWRITTEN` in `problems.jsonl`) and reads that library whole (7.5).
- `--full` ignores the state.
- `state.json` holds, per library, what was read from it; its content is the tool's own and may
  change without a change of the format version. A state written by another format version, or by
  a tool that keeps other things per object, is not used: the next run reads everything again.

## 10 The commands

### `toa index`

```text
toa index [--workspace FILE] [--out DIR] [--full] [--follow-includes]
          [--offline] [--allow-host HOST]... [--cache DIR] [--json]
```

- `--workspace`: the workspace list; by default the first `workspace.source.jsonl` from here up.
- `--out`: where the index is written; `toa-index/` beside the workspace list by default. The new
  index is written into a temporary directory beside it and renamed into place, so a reader never
  sees half an index.
- **Never asks:** `toa index` does not prompt, in a terminal or not. Hosts the user wrote into the
  workspace list are allowed (8.1, the tool's choice); `--allow-host` allows others, such as the
  hosts that `--follow-includes` reaches. `--offline` connects nowhere and reads only the cache and
  local libraries.
- `--cache`: the cache directory; `TOA_CACHE`, or the user's cache directory, by default.
- Output: progress on standard error; with `--json`, the findings and one `result` line on
  standard output.
- **Exit codes:** `0` the index was written (objects and libraries with problems are recorded, not
  failures); `3` it could not run (no git, the workspace list unreadable, the output not
  writable); `4` the index was written, but some library could not be fetched (offline, a host not
  allowed, the network). A reader that wants every library present checks for `4`.

### `toa cat`

```text
toa cat NAME PATH [--offline] [--allow-host HOST]...
```

Writes one file of an object, as raw bytes, to standard output: for example the symbol of an
interface.
It looks where `toa fetch` looks (7.3): the local libraries of the workspace list and the
repository at hand, then the cache (including the libraries `toa index` fetched), then the
addresses. The object is verified from its files and kept in the cache before the file is
written out. Sizes are looked at before reading. Exit codes: `0`; `1` the object has no such file;
`4` the object was not found. `toa fetch NAME --into
DIR` gives the whole object as files, verified from its files.

### Later: in the browser

`toa-core` builds for WebAssembly without changes. A thin JavaScript interface is planned for a
later step: given a set of files, the name they make (2.2) or why there is none (3.1); given a
manifest and a prefix, the problems of 3.2. Its form is not settled.

## 11 Correspondence with the requests of gazetteer

gazetteer, the read-only browser for TOA libraries, asked for nine things of the index. Where each
is met:

| Request | Where |
|---|---|
| 1 a versioned format, in shards | sections 2 and 3: `toa-index: 2`, one file per name |
| 2 per object | section 5 |
| 3 per library | section 4 |
| 4 reverse lookups and newer versions | section 5: `referenced_by`, `base_by`, `newer` |
| 5 incremental, full rescan when history was rewritten | section 9 |
| 6 non-interactive in CI | section 10: never asks, `--allow-host`, `--offline`, `--cache`, exit 4, no checkout |
| 7 a few files | section 10: `toa cat`, `toa fetch --into` |
| 8 findings as JSON | section 8 |
| 9 WebAssembly and a JavaScript interface | section 10, later |

Later, gazetteer asked for the same files of 8.3 to be computed by the index, so that a site and
`toa check` show the same (its section 3.2, items 40 and 41): `same_files` of section 5, with the
grouping of common files that gazetteer's `authorship.py` used.
