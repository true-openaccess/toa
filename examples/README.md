# Small examples

Four small TOA libraries and a workspace list, made by the reference tool `toa` from drafts. They
are for tests and documentation; the case libraries of TOA are separate repositories.

The libraries here are the files of each library at its last commit, without the git history.
The conformance test `toa/tests/examples.rs` builds them again from the drafts it holds, step by
step and with fixed dates, so every object name and commit is the same on every machine:

```sh
cargo test --test examples                                   # build, check, compare with this directory
TOA_EXAMPLES_WRITE=1 cargo test --test examples              # write this directory again
TOA_EXAMPLES_OUT=/some/empty/dir cargo test --test examples  # keep the git repositories and their index
```

Each step seals the drafts (`toa seal`), tidies the source lists and gives or moves the `latest`
marks (`toa publish`), and commits. Every commit passes the library check (6.4) against the one
before, and `toa index` over the workspace list finds no problem. The addresses
`https://example.org/toa-examples/<library>.git` are made up.

| Library | What it shows |
|---|---|
| `official` | An interface `strongarm-comp` with a circuit `sky130-razavi.g1` and a testbench `tb_offset.g1`, sealed together while naming each other by draft names. A note added after publication beside the interface. `toa publish` marks the interface, the circuit and the testbench `latest` in the library list, each the first of its series here. Its first release frozen as a frozen library `official.g1` (type `lib`), whose `members.source.jsonl` lists the three objects with the commit and path where they are, itself marked `latest` |
| `lee` | Another interface `dyn-latch-cmp`; `lee-sky130.g2` with `base` `lee-sky130.g1`, so of generation 2. The library list is in volumes under `library.source/`; `toa publish` moved its `latest` mark from g1 to g2. `lee-sky130.g1` was removed later with `toa remove --record`: a removal record is left where it was, and its line in the list is marked `removed`. The lineage of g2 still checks through the record |
| `alice` | `wrap-lee.g1` and `g2`, circuits of official's interface made of lee's (`uses`), and an evidence record (`evd`, `kind: sim`) of g1 with official's testbench and a `lock.json`. No interface of its own. `toa publish` moved its `latest` mark from g1 to g2, so the index offers g2 as the newer version of g1 (7.5). Its library list includes the lists of `official` and `lee` by address, lines its maintainer added |
| `official-chen` | A fork of `official` taken after the release: the same objects under the same names and marks, and a new circuit `sky130-lowkick.g1` with `base` `sky130-razavi.g1`. It takes a name of its own, so it starts a series of its own, of generation 1, and is not a newer version of `sky130-razavi`. A document, and a note of its own |
| `workspace.source.jsonl` | The workspace list including the four library lists, each by its address and by a local checkout beside the list |

Where to look:

- An object directory, its name the hash of its content, and the files beside it:
  `official/ifc/strongarm-comp/`.
- A source list beside an object that needs objects of other libraries, each with the address,
  commit and path where it is published (5.1, 5.2):
  `alice/ckt/strongarm-comp/wrap-lee.g1/ckt_….source.jsonl`. The interface is listed both in
  `official` and in its fork.
- A library list in volumes, with a removed object and a `latest` mark: `lee/library.source.jsonl`
  and `lee/library.source/`; the removal record
  `lee/ckt/dyn-latch-cmp/lee-sky130.g1/ckt_….removed.json`.
- `history.json` of an object with an ancestor, with its generation:
  `alice/ckt/strongarm-comp/wrap-lee.g2/ckt_…/`; of one with a name of its own, generation 1:
  `official-chen/ckt/strongarm-comp/sky130-lowkick.g1/ckt_…/`.
- A frozen library and its list of members: `official/lib/official.g1/lib_…/members.source.jsonl`.

The local sources in the workspace list point to directories beside it. In this directory they
are not git repositories, so `toa index` here cannot read them; build the repositories with
`TOA_EXAMPLES_OUT` and run it there.

All files here are CC0-1.0 (`toa/REUSE.toml`), and every object says so in its `license`.
