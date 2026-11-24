# Test vectors of TOA 0.1

Data for testing an implementation of the standard against the rules of [TOA 0.1](../README.md). Section numbers such as (2.2) refer to the standard.

- **What an expectation says.** Each vector gives the expected value, or whether the input is valid, and the section that a valid input satisfies or an invalid one breaks. It gives no message or problem code of any tool, so every implementation can use the same vectors.
- **Bytes as they are.** The [`.gitattributes`](.gitattributes) here is `* -text`: git stores and checks out every file in this directory byte for byte. The vector `bytes` depends on it, since one of its files ends in CR LF. Copy these files as raw bytes; a copy that converts line ends no longer matches.
- **Source.** The vectors are written by [`scripts/spec/vectors.py`](../../scripts/spec/vectors.py), which also checks them: `python scripts/spec/vectors.py --git` recomputes every value and cross-checks the hashes with git, in a SHA-1 and a SHA-256 repository.
- **Licence.** CC0-1.0, like all data of this repository.

## Files

| Path | Content |
| --- | --- |
| `hash.json` | Hashes of sets of files (2.2, 2.3), and git trees that list the same files in a non-canonical form |
| `hash/<id>/` | The files of each hash vector, as files. The hash of the directory `hash/<id>/` is the `hash` of that vector |
| `names.json` | Strings tested against the forms of an object name, the reserved form and a draft name (2.1, 2.4) |

## `hash.json`

`vectors` is a list of sets of files:

| Field | Content |
| --- | --- |
| `id` | Name of the vector |
| `dir` | Where its files are, relative to this directory |
| `files` | The same files as JSON: path, with `/` between levels, to the content as a string. All contents are ASCII; `\n` is the byte 0x0A and `\r` the byte 0x0D |
| `hash` | The hash of the set of files (2.2): 40 lowercase hexadecimal digits |
| `sha256` | The same three steps with SHA-256: the cross-check hash (2.2) |
| `valid_object` | Whether the set of files is also a valid object. Only `object` is: the others have no `toa.yaml` and only test the algorithm (2.3) |
| `section`, `note` | The sections tested, and what the vector is about |

`noncanonical_trees` is a list of git trees, written byte for byte, that all list the two files of the vector `object`:

| Field | Content |
| --- | --- |
| `id` | Name of the case |
| `files_of` | The vector whose files the tree lists |
| `tree_body_hex` | The body of the git tree, in hexadecimal: entries of a mode, a space, a name, a 0 byte and 20 bytes of id |
| `git_tree_id` | The id git gives this tree: SHA-1 of `"tree " + length + 0 byte + body` |
| `hash` | The hash of the files the tree lists (2.2) |
| `valid` | Whether the tree is canonical. A tree that is not gives a `git_tree_id` other than `hash`; verifying from the listing must find it (7.4), and a directory named after its id is not a valid object (2.2) |

## `names.json`

`vectors` is a list of strings. Each says, for the whole string, with nothing before or after it:

| Field | Content |
| --- | --- |
| `value` | The string. Some end in a line end or start with a space, written as JSON escapes |
| `object_name` | Whether it is an object name (2.1) |
| `reserved` | Whether it has the form reserved for a later hash algorithm, prefix and 64 hexadecimal digits (2.1). Such a name is not an object name in 0.1 |
| `draft_name` | Whether it is a draft name (2.4) |
| `section` | The section that defines the form it has or misses |
| `note` | What the case is about, where it is not plain |

At most one of `object_name`, `reserved` and `draft_name` is true. A string for which all three are false is an ordinary string: in a manifest it is no reference (3.3).

## Not here yet

Vectors for well-formed object trees (3.1), manifests (3.2), `history.json` (3.4), source lists (5.1) and library checks (6.4) are to follow. Trees that git cannot hold, such as names that differ only in case or reserved names of Windows, will be described in JSON rather than stored as files.
