# TOA library format standard, version 0.1

TOA (true open access) is a format standard for design libraries. Objects and references are plain files: naming an object gives the same value as git's tree hash, but needs no git. Version `0.1` publishes and shares objects through git repositories, so the same library works online and offline, in public and in private. Every object is named by the hash of its content, so a name always stands for the same content. This document is the standard of version `0.1`.

- The English text is normative. The Chinese version, `toa-0.1.zh.md`, is a translation; where the two differ, the English text prevails. The English terms are the only normative terminology; each translation keeps its own glossary.
- The type specifications in `types/` are part of this standard: each defines one type of object (chapter 9).
- The test vectors in `vectors/` are informative. Where they and this text differ, this text prevails (1.6).
- Versions `0.x` make no promise of compatibility with each other (chapter 10).
- Any tool, website or EDA integration may implement this standard. A reference tool, `toa`, is published with it.

## 1 Overview

### 1.1 Scope

TOA is a format for design libraries: the files of an interface, of its circuits, of their testbenches and evidence records, of documents about them, and of frozen libraries that freeze a set of them. Each of these is an object, a directory named by the hash of its content. Objects do not depend on git: the hash in an object name is the value git computes as a tree hash, but computing it needs no git (2.2). Version `0.1` publishes and shares objects through libraries, which are git repositories; on one machine, objects may also be kept in plain directories, which source lists point to by `local` sources (5.1). Objects refer to each other by name, so a design is the same design wherever its objects are kept.

This standard defines:

- how an object is named, and how the name is computed from its files (chapter 2);
- what an object directory may hold, its manifest, its references and its lineage (chapter 3);
- the files kept beside an object: notes, its source list and a removal record (chapter 4);
- source lists, which say where objects can be found (chapter 5);
- libraries: what a library is, how objects are arranged in it, and the checks it must pass (chapter 6);
- the operations on objects: sealing, publishing, fetching, verifying, following updates and assembling (chapter 7);
- the workspace list and the index (chapter 8);
- how types of object are registered and specified (chapter 9), and how the standard evolves (chapter 10);
- what must be said about security (chapter 11).

TOA does not define which licences a library accepts, who may write to a library, how contributions are reviewed, where accounts come from, or what a website looks like. Each deployment settles these for itself.

### 1.2 Structure of this standard

Chapters 2 to 4 define objects, chapters 5 and 6 where objects are kept, and chapters 7 and 8 what tools do with them. Chapter 9 registers the types of object; each type is specified in a type specification of its own, in `types/`. Annex A lists the limits set throughout the text; Annex B lists what is left for later versions. Both annexes are informative, as are notes and examples. Everything else is normative.

### 1.3 Requirement words

- **must**, **must not**: a requirement. An object or library that fails one is not a valid TOA object or TOA library; a tool that fails one does not conform (1.6).
- **should**, **should not**: a recommendation; there may be reasons to do otherwise, and they should be understood first.
- **may**: something allowed.

### 1.4 Terms

| Term | Meaning |
|---|---|
| ancestor | An object named in the history of another object (3.4) |
| arrangement | How the objects of a library are placed in its directories. Only the name of an object directory has meaning (6.2) |
| assemble | Turn a circuit and its expanded tree into a set of files that can be simulated directly, alone or with a testbench and the conditions of one run (7.6) |
| check | Test whether something keeps the rules of this standard, as the library check does (6.4). Not to be confused with verify |
| cross-check hash | The value computed as the hash of an object, with SHA-256 in place of SHA-1. It is not part of the name and serves only for independent checks (2.2) |
| deployment | A party that uses TOA, such as a library, a platform or a CI service, and settles what this standard leaves open |
| draft | An object directory that has not been sealed yet. Its name is a draft name (2.4) |
| expanded tree | A circuit, with the circuits it uses through `uses`, level by level to the bottom (7.6) |
| external file | A file that an object needs but does not carry, such as a model file of a PDK (3.5) |
| fetch | Get the files of an object by its name (7.3) |
| frozen library | An object that freezes a set of objects, such as one release of a library: a list of its members in the format of a source list (type `lib`). Not to be confused with library |
| generation | For people, how far an object is down its series: 1 for the first, one more than its parent for each next one. Sealing writes it in `history.json` (3.4) |
| group | A path of one to four parts that a source list gives an object, such as a library and a design. Groups are only for people (5.1) |
| history | The list of an object's ancestors along the first item of `base`, in its file `history.json`, beside its generation (3.4) |
| index | Data derived by reading libraries: who refers to an object, where and when it appeared, what libraries say about it (8.2) |
| library | A git repository, the whole repository, that holds objects: how version `0.1` publishes and shares them. It only answers where objects are (6.1). Objects kept in a plain directory on one machine are in no library |
| library check | The checks the maintainer of a library makes on every new commit before moving the publication branch (6.4) |
| library list | The source list at the top of a library, `library.source.jsonl`, listing all objects of the library (5.2) |
| latest mark | `"latest": true` on an object line of a library list: this library recommends the object in place of the earlier generations of its series (5.1, 7.5) |
| lineage | Which objects an object was derived from: `base` and `history.json` (3.4). Not to be confused with source |
| local file map | A file on the user's machine that maps each process and external file to a file on that machine; it never enters a public library (3.5, 7.6) |
| manifest | The file `toa.yaml` at the top of every object: its type, licence, references and the fields its type specification asks for (3.2) |
| note | One file in the notes directory of an object: what one library adds to say about it, never changed once published (4.1) |
| object | A directory whose name is computed from its content, with a manifest at its top. It never changes (chapter 3) |
| object name | `<prefix>_<hash>`, the name of an object (2.1) |
| prefix | The part of an object name before the underscore; it is also the name of the object's type (2.1) |
| publication branch | The branch of a library that others are meant to see; the users of the library say which, `main` by default (6.3) |
| published | In the latest commit of a library's publication branch (6.3) |
| readable name | The levels of directory before an object directory in the recommended arrangement, there for people, such as `strongarm-comp/wrap-lee.g2` (6.2) |
| reference | A value in a manifest or `lock.json` that is a whole object name, or the object of a line of `members.source.jsonl` (3.3) |
| removal record | A file that may be left where an object was removed: what it was, where it came from, where it can still be found (4.2) |
| series | The generations of one design: objects of one type with the same `name`, linked along the first item of `base` (3.4) |
| seal, unseal | Sealing turns drafts into objects: it computes their names, renames their directories and puts the names in place of the draft names. Unsealing turns objects that are not yet published back into drafts (7.1) |
| sidecar file | A file or directory beside an object directory whose name starts with the object name: the notes directory, the source list, the removal record and the stamps directory. Sidecar files are not part of the object (chapter 4) |
| source | Where an object can be fetched: a git repository or a local directory (5.1). Not to be confused with lineage |
| source list | A list of objects in the format of chapter 5, each with cached information and its known sources; it may include other source lists. The library list and the workspace list are source lists |
| type, type specification | The kind of an object, named by its prefix; the document that says which fields and files objects of a type have (chapter 9) |
| valid | Meeting every requirement of this standard that applies; the opposite is invalid |
| verify | Recompute the hash of an object and compare it with its name (7.4). Not to be confused with check |
| well-formed object tree | A directory that keeps the rules of 3.1, which look only at names, kinds of entry and sizes |
| workspace list | A source list in the user's working directory that lists all objects the current design uses, usually by including the library lists of some libraries (8.1) |

### 1.5 Principles

- **An object is known by its name alone.** Two objects with the same name are the same object, in whichever library, mirror or copy they are found.
- **A reference is a name, never an address.** Where an object is found is answered by source lists, including the workspace list, and by the index, all of which are outside the object.
- **An object never changes.** Any change to its content gives another name, and so another object (3.6).

Two properties of TOA follow from these principles. They are said here for readers, and add no requirement:

- **A design assembles itself.** When every object carries its source list beside it (chapter 4), that list says where the object and the objects it refers to can be found (5.2). Starting from one object and following its references, a tool gathers everything that object needs: from an evidence record, for example, the circuit tested, the whole tree of circuits it uses, the testbenches and the interfaces. No catalogue or registry kept by anyone is needed (7.3, 7.6).
- **A change cannot pass unnoticed.** An object is fixed by its name, the name is the hash of its content, and the names of the objects it refers to are part of that content. Changing any object gives it another name: the objects that referred to the old one still refer to it, unchanged, and do not silently come to mean something else. Anyone who has an object can compute its name again and see whether it is the one named (7.4). `base` and `history.json` (3.4) record what each object was made from, so the generations of a design can be followed back name by name.

### 1.6 Conformance

This section gathers the requirements of the other chapters by what they apply to. It adds none of its own.

- **A valid object** has an object name (2.1) and is a well-formed object tree (3.1) whose hash is the hash in its name (2.2). Its manifest keeps the rules of 3.2, its `type` is its prefix, and its `history.json`, `lock.json` and `members.source.jsonl`, where present, keep 3.4 and 3.3. A tool that knows the object's type also applies its type specification (chapter 9); a tool that does not cannot, and the object is still valid as far as that tool can tell.
- **A valid library** keeps 6.1, and every commit of its publication branch passes the library check (6.4).
- **A conforming tool** keeps the requirements on what it does:
  - any tool that reads objects treats an unknown prefix as an object (2.1), ignores fields it does not know (3.2), and reads the source lists of others leniently (5.1);
  - a tool that seals keeps 7.1, and one that publishes keeps 7.2;
  - a tool that fetches keeps the steps of 7.3;
  - a tool that verifies keeps 7.4, and verifies on the occasions 7.4 lists;
  - a tool that removes objects keeps 6.5, and one that writes removal records keeps 4.2;
  - a tool that builds an index keeps 8.3, and one that assembles keeps 7.6;
  - every tool keeps chapter 11.
- **Test vectors.** The test vectors in `vectors/` give, for each input, whether it is valid and the section it keeps or breaks. A tool passes a vector when it tells valid from invalid as the vector does; it need not report the section. The vectors are informative: where they and this text differ, this text prevails.

## 2 Object names

### 2.1 Form

```text
object name = prefix "_" hash
prefix      = a lowercase letter, then 1 to 7 lowercase letters or digits   2 to 8 characters
hash        = 40 characters, each 0-9 or a-f                               lowercase hexadecimal
```

- **Splitting.** An object name holds exactly one underscore. What comes before it is the prefix, what comes after it the hash. Neither the prefix nor the hash contains an underscore.
- **The hash always has 40 digits.** It is the value computed by 2.2, written in full, not shortened and not in upper case.
- **Object names are compared as whole strings,** case-sensitively. To decide whether a string is an object name, the whole string must match, with nothing before or after it, a line end included.
- **The reserved form.** `prefix_` followed by 64 lowercase hexadecimal digits is reserved for a later hash algorithm.
  - A library of version `0.1` must not hold such a name. It is also longer than the 64 characters a name may have (3.1).
  - A tool that meets a directory of this form in a working directory must not enter, rename or remove it, and should report it.
  - Other lengths have no special meaning: `tb_ac` is an ordinary string.
- **The prefix is the name of the type.** The `type` of the manifest must be the prefix, character for character (3.2). A type has no other name. The identity of an object is its hash.
- **Only one prefix is valid for a given content.** The manifest is part of the hash, and its `type` must be the prefix, so the same content under another prefix is not a valid object. Every tool can check this without knowing the type. There is no further check that one content has one name.
  - The same object may be kept several times in one library (6.2).
- **A tool that meets a prefix it does not know** must still treat the directory as an object: list it, verify its name and its `type`, copy it, fetch it by name and carry its sidecar files with it. It need not check a type specification or show details.

### 2.2 Computing the hash

The hash of a set of files is defined first. The input is a number of paths, each with the bytes of a file; a path is divided into levels of directory by `/`.

1. **The hash of a file:** `SHA-1("blob " + the number of bytes of the file in decimal + a 0 byte + all bytes of the file)`.
2. **The hash of a directory:** list the direct entries of the directory, one per entry:
   - a file: `"100644 " + the file name + a 0 byte + the hash of the file (20 raw bytes)`;
   - a subdirectory: `"40000 " + the directory name + a 0 byte + the hash of the subdirectory (20 raw bytes)`.
   - The entries are sorted by the bytes of their names, a subdirectory's name sorting as if it ended in `/`.
   - Joined end to end, the entries form the body. The hash of the directory is `SHA-1("tree " + the number of bytes of the body in decimal + a 0 byte + the body)`.
3. **The hash of the set of files** is the hash of the top directory, written as 40 lowercase hexadecimal digits.

- **The hash of an object** is the value computed in this way from all files in the object directory. **Only a well-formed object tree (3.1) has a hash of an object:** a directory that breaks the rules must be reported as invalid, and its hash must not be computed by skipping the entries that break them.
- **Files are taken byte for byte:** no conversion of line ends, no filter of any kind.
- **The hash depends only on the names and contents of the files** in the directory, not on what the directory is called, where it is, or in which commit.
- **Computing the hash without git,** an implementation of this standard should use SHA-1 with collision detection. An object in which a collision is detected must be judged invalid.
- **Git.** In a library that keeps the rules of 3.1 and 6.1, this value is the id git stores for the directory, its tree hash. This follows from the rules and is not the definition. The two differ in two cases, and in both the algorithm of this section prevails:
  - The settings of the repository break the rules (conversion of line ends, filters, ignore rules), and git stores bytes other than those in the working directory.
  - The tree stored in the repository is not written canonically: a mode with a leading 0, entries out of order, an entry name holding `/`. Git gives such a tree another id. Verifying from the listing must find it (7.4).
- **The cross-check hash:** the same three steps with SHA-256 in place of SHA-1 (the hashes of files and subdirectories are then 32 raw bytes) give a value of 64 hexadecimal digits. It is not part of the name and serves only for the independent checks of chapter 11. It equals the tree hash that git's SHA-256 object format computes for the same directory.

### 2.3 Test vectors

| Content of the directory (file name: content) | Hash | Cross-check hash (SHA-256) |
|---|---|---|
| `toa.yaml`: `type: doc\nlicense: MIT\n`; `README.md`: `hello\n` | `ea44863a8d2f242bc651caf709d45a27e7531d9d` | `e282624f05b86cc1bae3ef0abf32ad2540fb72fc15d309cea0988289b9ee2bd9` |
| `a.txt`: `1\n`; `a/z`: `2\n`; `a-b`: `3\n`; `a0`: `4\n` (tests the order: `a-b`, `a.txt`, the directory `a`, `a0`) | `497bcf8f72cdfb77f6ab7d4e0ac6cfeaafa81c8c` | `6405c1e3597fbe6fd1f4f30a3f99bed0edb1858cb9d72e829ee1116ce4012b7d` |
| `empty`: an empty file; `crlf.txt`: `x\r\n` (tests bytes taken as they are) | `372f060b024c23ddf17d022472fff124c6d37efb` | `cd440b4b25949dc63433404279def2ffcb0ff07a3a3ad3c3b0177b1078df2d4c` |

- `\n` is a line feed byte (0x0A), `\r` the byte 0x0D. Both values of all three rows were checked with git, in a repository of the SHA-1 and one of the SHA-256 object format.
- The last two rows only test the algorithm: they have no `toa.yaml` and are not valid objects; they only have a hash of a set of files.
- **Trees not written canonically.** For the two files of the first row, a git tree that writes the mode of `toa.yaml` as `0100644` has the id `88877fcc08dc665dc776a5f121bebfd1bc99469e`, and one that lists the two entries in the opposite order has `ac31b57cf9c3bd3ff8bd22112d2b5e480e6ffad1`. Neither is the hash of these files, and a directory named after either is not a valid object.
- The same vectors, these trees and more vectors on names and draft names are in `vectors/` (1.6).

### 2.4 Draft names

- **An object directory that has not been sealed has a draft name:** `<prefix>_draft-<tag>`. The tag is 1 to 40 lowercase letters, digits or hyphens, starting with a letter or a digit.
- **A draft name can never be an object name,** since what follows its underscore is not 40 hexadecimal digits.
- **A tag holds no dot.** The sidecar files of a draft are named `<draft name>.notes/` and `<draft name>.source.jsonl`; they hold a dot, so they are never taken for another draft.
- **The form is reserved:** in a manifest, a `lock.json` or a `members.source.jsonl`, every value that as a whole has the form of a draft name is taken as a reference to a draft. No type specification may allow a field to take a value of this form.
- **No draft name may appear on a publication branch.** This covers:
  - the names of directories and files, and names that start with a draft name and a dot;
  - every value anywhere in the `toa.yaml` of every object;
  - every value in the `lock.json` of every object;
  - every value in the `members.source.jsonl` of every object.
- Other branches may hold drafts.

## 3 Object directories

### 3.1 Well-formed object trees

A directory is a valid object only if all of the following hold:

1. **It holds only regular files and directories:** no symbolic links, junctions, submodules or device files, and no empty directories. In git the mode of every entry is `100644` or `40000`, so no file is executable.
2. **It has a `toa.yaml` at its top, a regular file** (3.2). The files `history.json` (3.4), `lock.json` and `members.source.jsonl` (3.3) at the top have fixed meanings, whatever the type, and where present they must be regular files.
3. **Every name** (of a file, and of every directory on its path):
   - uses only `A-Z a-z 0-9 . _ -` and is 1 to 64 characters long;
   - starts with a letter, a digit or an underscore. So no name starts with a dot: `.git`, `.gitattributes`, `.gitignore` and `.gitmodules` cannot occur in an object;
   - does not end with a dot;
   - is not, in the part before its first dot and in any case, `con`, `prn`, `aux`, `nul`, `com0` to `com9` or `lpt0` to `lpt9`.
4. **No two names in one directory differ only in case.**
5. **No name at any level within the object is an object name or a draft name.** Objects do not nest.
6. **Limits:**
   - at most 6 levels of directory below the object directory: the relative path of a file holds at most 6 `/`;
   - the relative path of any file within the object is at most 80 characters long;
   - at most 2,000 files;
   - at most 10 MiB for a file, and 50 MiB for the whole object. A library may set lower limits.

- **These rules look only at names, kinds of entry and sizes,** never at the content of files.
- **Checked on a listing of git,** two more things must hold: no path occurs twice, and every directory on the path of an entry is itself listed as a directory. A git tree can hold an entry whose name contains `/`, which in the listing looks like an ordinary path; this check is what finds it.
- **On a file system without permission bits (Windows),** verifying from the files does not check whether a file is executable. The second computation of sealing (7.1) and the library check find it.
- An object fetched from elsewhere must be checked on its listing before it is written to disk (7.3).

### 3.2 The manifest `toa.yaml`

#### 3.2.1 Common fields

- **Every object has exactly one manifest, `toa.yaml`, at its top.** It has this name for every type.
- **The common fields:**

  | Field | Requirement | Meaning |
  |---|---|---|
  | `type` | must | The type of the object: its prefix, character for character, such as `ckt`. A prefix that happens to be a word such as `on` or `no` is quoted (3.2.3, rule 10) |
  | `license` | must | The licence of the whole object, as an SPDX licence expression (3.2.2) |
  | `author` | may | A credit, in free text. It is fixed with the content, and is no proof of identity (below) |
  | `base` | may | A list of one or more object names: the objects of the same type this object was directly derived from (3.4). An object derived from none has no such field |
  | `title` | may | A title of one line, at most 200 characters. The interface must write it |
  | `description` | may | What the object is, in a paragraph or a few. A longer text belongs in `README.md` |
  | `tags` | may | A list of words to find the object by |

- **The other fields are defined by the type specification** (chapter 9).
- **`author` goes with the content.** It is in the manifest, and so in the hash: every copy passed on as it is keeps it, and a copy with another `author` is another object, with another name. It is no proof of identity: anyone can write any name. Who put an object into a library is shown by a signed commit (6.3), and that it existed by some day by the evidence of chapter 11.
- **Names of fields.** The name of every field this standard and its type specifications define is one lowercase English word. Where two words are needed, the field is nested, as in `simulator: {name, version}`. Underscores are not forbidden, but no standard field uses one. This is about the names of fields only: values, such as the names of pins, subcircuits, metrics and testbenches, keep their own rules.
- **The manifest does not hold the object's own name.** No file can hold its own hash.

#### 3.2.2 Licence

- `license` must follow the syntax of an SPDX licence expression: licence identifiers, `LicenseRef-<name>`, `AND`, `OR`, `WITH`, brackets and `+`.
- `DocumentRef-…` must not be used: it points to an SPDX document outside the object. `NONE` and `NOASSERTION` are not licence expressions and must not be used either.
- For every `LicenseRef-<name>` in the expression, the object must hold a file whose name starts with `LICENSE` and whose content is the full text of that licence.
- A licence identifier should be one on the SPDX licence list. A tool that does not know an identifier should warn, and must not judge the object invalid for it: the list grows, and tools need not follow it.
- Whether files in an object carry licence headers, and how they must agree with `license`, is for the type specifications to say.

#### 3.2.3 Syntax

A manifest is written in a subset of YAML. The rules below look only at how the file is written; they do not depend on how any YAML library reads version `1.1` or `1.2` of YAML. With them, a tool can read a manifest without a YAML library.

1. **The whole file:** UTF-8 without a byte order mark. Lines end with LF only, and the file ends with LF. It holds no CR, NUL, U+0085, U+2028 or U+2029 (a string that needs one of these writes it with an escape in double quotes). At most 256 KiB.
2. **One document:** no directives (`%YAML`, `%TAG`), and no line `---` or `...`. The top is a block mapping whose keys start in the first column. Nesting is at most 16 levels deep, the top mapping being level 1.
3. **No anchors, aliases or tags.** No mapping holds the same key twice. Every key is a string. No complex keys (`?`).
4. **Indentation is made of spaces.** No tab occurs anywhere, except inside a quoted scalar and in the content of a block scalar.
5. **One line each:** a plain scalar, a quoted scalar and a flow collection each start and end on the same line. A text of several lines is written as a block scalar.
6. **Block collections:**
   - A mapping entry is a key, `:`, and then either a space and a value on the same line, or nothing. When nothing follows, the value is the block collection on the next lines, which is indented more than the key or, for a sequence, may be indented as much as the key; with no such lines the value is null.
   - A sequence entry is `-` and then either a space and a value, or a space and the first entry of a mapping whose other entries are indented to the same column as that first key, or nothing, with a block collection on the next lines indented more than the `-`.
   - The entries of one collection have the same indentation.
7. **Block scalars:** the header is `|`, `|-`, `>` or `>-` and nothing else; nothing follows it on its line. Its content is on the next lines, indented more than the line of the header. The indentation of the content is that of its first line, which must not be empty; every other line of the content that is not empty has at least that indentation.
8. **Flow collections:** `[…]` and `{…}`, which may nest, on one line. Their entries are separated by `,` with optional spaces around it; there is no trailing comma and no empty entry. An entry of a flow mapping is a key, `:`, a space and a value. An entry of a flow sequence is a scalar or a flow collection. A plain scalar in a flow collection contains none of `,` `[` `]` `{` `}`. `[]` and `{}` are an empty sequence and an empty mapping.
9. **Plain scalars** (without quotes) are read by this table:

   | Written | Read as |
   |---|---|
   | empty, `~`, `null` | null |
   | `true`, `false` | a boolean |
   | `[-+]?(0\|[1-9][0-9]{0,17})` | an integer (at most 18 digits) |
   | `[-+]?(0\|[1-9][0-9]*)\.[0-9]+([eE][-+][0-9]+)?`, at most 32 characters, with a finite value | a floating-point number |
   | `.inf`, `-.inf`, `.nan` | a floating-point number |
   | anything else | a string, under rules 10 and 11 |

10. **Quotes are required** for a scalar that falls in the last row of the table and:
    - starts with a digit, `+`, `-` or `.`; for example `'2026-10-30'`, `'23.1.0'`, `'1e-9'`, `'0x1F'`, `'007'`, `'../lib'`;
    - equals, in any case, `y`, `n`, `yes`, `no`, `on`, `off`, `true`, `false` or `null`; for example `'on'`, `'True'`, `'NULL'`;
    - or equals `=` or `<<`.

    Keys keep this rule too.
11. **A plain scalar read as a string** does not start with a space or with any of `-` `?` `:` `,` `[` `]` `{` `}` `#` `&` `*` `!` `|` `>` `'` `"` `%` `@` `` ` ``; does not contain `: ` or ` #`; and does not end with `:` or a space.
12. **Quoted scalars:** in single quotes, `''` stands for one quote, and there is no other escape. In double quotes, the escapes are `\0` `\a` `\b` `\t` `\n` `\v` `\f` `\r` `\e` `\"` `\\` `\N` `\_` `\L` `\P`, `\x` with 2, `\u` with 4 and `\U` with 8 hexadecimal digits; no other escape, such as `\/`, is allowed. Quoted scalars and block scalars are always strings.
13. **Comments and empty lines:** a line that is empty, or holds only spaces, or whose first character other than spaces is `#`, is ignored. After a value on its line, at least one space and `#` start a comment to the end of the line. There is no comment on the line of a block scalar header, and within the content of a block scalar `#` is content.

#### 3.2.4 Reading and writing

- **An object whose manifest breaks these rules is not a valid object.** Tools report it, do not assemble it and do not take it as part of an expanded tree. They may still list it by name, copy it and verify its hash.
- **Sealing must stop with an error on a manifest that breaks the rules,** and must not rewrite it for the author: whether `on` was meant as a string or a boolean is not for a tool to guess.
- **Reading,** a tool must ignore fields it does not know, and must not reject an object for them. Sealing checks strictly against the type specification.
- **The version of a type specification is not written in the object.** A type specification changes only compatibly; an incompatible change takes a new prefix, which is a new type (chapter 10).
- **Every past version must stay readable forever.** Objects do not change, so a format is never upgraded in place.

### 3.3 References

- **Every value in a manifest, at any level, that as a whole is an object name is a reference.** Only the value counts, not the key. An object name meant only as a mention, not as a reference, is written in `README.md`, whose text is not searched for references.
- **The names in `lock.json` are references too.** When an object has a file `lock.json` at its top, whatever its type:
  - it must be a JSON object, in UTF-8, of at most 1 MiB;
  - every string value in it, at any level, that as a whole is an object name is a reference of the object;
  - the rest of its format is defined by the type specification. Only the evidence record uses it at present (chapter 9).
- **So are the members in `members.source.jsonl`.** When an object has a file `members.source.jsonl` at its top, whatever its type, it is a list of members:
  - it is a source list (5.1) that is valid when read strictly, within the limits of 3.1 instead of those of a source list;
  - its header is `{"toa": "source/1"}` and nothing more, and every other line is an object line: no include lines, since an object that never changes cannot take in a list that changes;
  - its object lines have no `removed` and no `latest`, and their sources are only `git` sources;
  - the `object` of every line is a reference of the object. The names in the cached fields of the list, such as `refs`, are not;
  - what else it means is defined by the type specification. Only the frozen library (type `lib`) uses it at present, and requires it (chapter 9).
- **References must be found.** They are looked up among the objects that a workspace list lists when expanded (8.1): the objects on the publication branches of the libraries it includes, and the objects it lists itself. A reference not found there is dangling, and a tool must report it.
- **When the object referred to has been removed and a removal record left,** a tool reports in which library it was removed and gives what the removal record says (4.2).
- **The library check knows no workspace list.** It lists the references not in the library itself (6.4, item 9); whether these are allowed is for the deployment to say.
- **The names in `history.json` need not be found.**
- **References point one way only, from the later object to the earlier one.** A circuit refers to the interface it implements, never the other way round. Which circuits an interface has is gathered by the index. References cannot form a cycle: the name of an object depends on the names it refers to.
- **A reference fixes the whole tree.** The manifest is part of the hash, so the name of an object fixes every object it refers to, level by level to the bottom.
- **Naming the fields that hold references.** A type specification names a field whose values are references:
  - by the prefix of the type it points to, when it may point to one type only, with `s` added for a list: `ifc`, `ckt`, `tbs`;
  - by an English word for its role, when it may point to several types, or when an object refers to objects of one type in two roles: `dut`, `refs`. The type specification says which prefixes it allows.

### 3.4 Lineage: `base` and `history.json`

- **`base` is written by the author:** the objects this object was directly derived from. The next version of the same design and a change to someone else's object both write it. An object made from scratch has no such field; a draft that writes it as an empty list loses it when it is sealed (7.1).
- **Every item of `base` has the prefix of the object itself:** an object is derived only from objects of its own type. Sealing and the library check check this from the names alone.
- **`base` says what an object was made from, never that it replaces anything.** Which object is the one to use now is said by libraries, by marking it `latest` in their library lists (5.2, 7.5).
- **A reference or `base`?** Ask whether the object would still say the same if the field pointed elsewhere. A circuit with another subcircuit, or a record with another testbench, says something else: such fields are references named by the type specification (3.3). A newer circuit says the same without its older version: that is `base`. The object `base` points to comes with the object, but is not followed further (5.2), and the names in `history.json` are not fetched at all.
- **A series** is the generations of one design: objects of one type with the same `name`, linked along the first item of `base`. For a type without `name`, such as `evd` and `doc`, the whole lineage along the first item of `base` is one series. The readable name without its generation names the series (6.2).
  - A port to another process, or a variant made only for one simulation, takes another `name`, and starts a series of its own from generation 1.
  - Someone else's change that keeps the `name` stays in the series. Whether it is a newer version that the original library recommends is said by the libraries that mark it (7.5).
- **The generation** of an object without `base` is 1. Of an object with `base`, it is the generation of the first item of `base` plus one when that item is in the same series, and 1 when it is not. Sealing computes it and writes it in `history.json`, so that an object shows the same generation everywhere.
  - The generation is for people: tools show it in readable names and labels (6.2). It is never written in the manifest. Two objects may have the same generation, and it says nothing about who made them or when.
- **`history.json` is written by sealing:** a file at the top of the object directory. Its content is a JSON object with two keys:
  - `history`: a list of object names, the most recent first. The list is the first item of `base`, followed by the `history` of that item. When `base` has several items, only the first is followed;
  - `generation`: the generation of the object, an integer from 1 up; it is 1 when the first item of `base` is in another series.
  - An object with `base` must have this file; an object without `base` must not, and is of generation 1.
- **It has exactly one way of being written:** an indentation of two spaces, one name per line, ending with LF; byte for byte what Python's `json.dumps({"history": names, "generation": n}, indent=2)` writes, the keys in this order, followed by one LF. It is part of the object and of its hash, so different tools must write the same bytes. For example:

  ```json
  {
    "history": [
      "ckt_cd15342bb4ed1cc1137937195f7cf3c3b8b03af5",
      "ckt_f1ede73af6ba50ae0621e34b2b6f6b946dbf1ff1"
    ],
    "generation": 3
  }
  ```

- **Before sealing copies the `history` and reads the generation of the parent,** the parent must be found on the machine and verified from its files (7.4). A parent without `history.json` is of generation 1.
- **The library check verifies it** (6.4, item 4): it is written the one way; its first item is the first item of `base`; when that item is in the library too, the other items are its `history`, and `generation` is computed from it as above; when only a removal record of it is left in the library, the other items are the `history` of the removal record (4.2); otherwise the other items and `generation` are not checked.
- **`history.json` is part of the object and of its hash.** So every object carries its full lineage and its generation, and when one link of the chain is lost, the earlier ancestors can still be found.
- **What it proves:** the ancestors it names that can be found have a fixed content and existed before the object.
- **What it does not prove:** who the author is, on which day it was made, or that nothing was borrowed from outside the list. The part whose ancestors cannot be found is only declared, and so is a generation that could not be checked against the parent; an index must keep the verified part apart from the declared one (8.3).

### 3.5 External dependencies: files an object needs but does not carry

Public objects do not carry the models of a PDK or other restricted files. An object only declares what it needs; the files themselves are on the user's machine.

- **One mechanism for every type; the type specification decides who uses it.** A process and other external files are declared in the same way whatever the type. Each type specification says whether objects of its type may name a process, in which field, and whether they may have `external`. In `0.1`, circuits name a process in `specifications.process`, and circuits and testbenches may have `external`; the other types declare neither.
- **The process** an object names is the process whose models its devices come from: a short name such as `sky130`, of the form `[a-z][a-z0-9_-]{0,39}`.
- **`external`:** a list at the top level of the manifest, each item one other external file.

  | Field | Requirement | Meaning |
  |---|---|---|
  | `name` | must | What the object calls it; not repeated within the object |
  | `role` | must | What kind of file it is, such as `modelfile`, `techfile` or `other`: a word of the form `[a-z][a-z0-9_-]{0,31}`. The words are an open list: a tool must accept a role it does not know |
  | `process` | may | The process it belongs to |
  | `file` | may | A file name, for people to recognise it. It contains no `/` and no `\` |
  | `version` | may | A readable label of its version |
  | `match` | may | A list of short strings that must all occur in the file. They confirm that the file at hand is the right one, while allowing it to be renamed or slightly changed |
  | `sha256` | may | The SHA-256 of the whole file. Written only where this value may be made public and the file must be the same byte for byte |

- **The model files a simulation actually used** are still recorded in the `conditions.models` of the evidence record: a file name (likewise without `/` and `\`), `section`, `version` and `match`. Each item may add `process` and `sha256`.
- **The external files themselves must not enter a public library.**
- **The local file map** is a file on the user's machine that says which actual file on that machine stands for each `process` and each external file. It is in the working directory and never enters a public library. Its format and how it is matched are in 7.6.
- **Two objects may use the same `name` for different files.** A `name` has meaning only within its object, so an entry of the local file map may add `process` to tell them apart.
- **This standard does not require the processes of an expanded tree to agree.** Assembling, a tool must list all external dependencies of the whole tree and match each in the local file map, reporting those that do not match. What to do when the models of two processes have devices of the same name is for the assembling tool; this standard does not say.

### 3.6 Immutability

- Once the content of an object is fixed, so is its name. Any change gives another name, and so another object.
- To change an existing object, copy it to a draft whose `base` points back to the original, change it, and seal it.

## 4 Sidecar files

Beside an object directory, in the same parent directory, there may be four things whose names start with the object name. They are sidecar files: they are not part of the object and not part of its hash.

| Name | What it is |
|---|---|
| `<object name>.notes/` | The notes directory: what this library adds to say about the object (4.1) |
| `<object name>.source.jsonl` | The source list: the known sources of this object and of the objects it needs (chapter 5) |
| `<object name>.removed.json` | The removal record: the object was removed from here (4.2) |
| `<object name>.stamps/` | The stamps directory: proofs from third parties that the object name existed at a time (4.3) |

- **No other name may start with `<object name>.`**
- **They must be regular directories and regular files,** not symbolic links. A source list and a removal record are at most 1 MiB each.
- **When an object is kept several times in a library,** each copy may have sidecar files of its own. 4.1 says how their notes relate.
- A library may hold sidecar files of an object without the object itself. A removal record is such a file; other sidecar files may then be anywhere.

### 4.1 Notes

- **One note, one file:** `<object name>.notes/<file name>.md`. The file name keeps item 3 of 3.1, must end with `.md`, and should be written `<date>-<short name>.md`, such as `20261118-erratum.md`.
- **The directory holds only regular `.md` files placed directly in it,** no subdirectories. A note is at most 256 KiB.
- **A note is known by the object name and its file name,** not by where in the library its notes directory is. When an object is kept several times in a library and each copy has a notes directory, notes of the same file name must have the same content, and together they count as one notes directory.
- **Notes are only added:** once a note is on the publication branch, its content must not be changed and it must not be removed. The whole notes directory may move with its object. A correction is a further note. The one exception: when the object is removed (6.5), its notes may be removed with it.
- **Notes are what this library says about the object.** The same object may have different notes in the original library and in a fork. An index keeps them per library.
- **Who wrote a note** is told by the commit that added the file. A credit in the file is only text.
- **Showing notes, a tool must treat them as untrusted text,** and say which library they come from (chapter 11).
- **States for machines, such as withdrawn or superseded, are not written in notes** but in the files that the maintainer of the library keeps (6.6).
- Uses: errata, clarifications, additions. A correction to the text of an interface is a note, not a new interface.

### 4.2 Removal records

After an object has been removed from a library (6.5), a file may be left where it was, telling others what was removed, where it came from and where it can still be found.

- **This standard does not require one.** To remove every trace of an object from the current state of a library, none is left. A tool that removes objects should be able to write one.
- **The file:** `<object name>.removed.json`. It counts wherever it is in the library, and should be in the parent directory where the object was. It is a JSON object in UTF-8, of at most 1 MiB.
- **Fields:**

  | Field | Requirement | Meaning |
  |---|---|---|
  | `toa` | must | `"removed/1"` |
  | `object` | must | The name of the removed object, the same as in the file name |
  | `commit` | must | A commit of the publication branch that holds the object. Tools write the last one before the removal |
  | `path` | must | The path of the object in that commit |
  | `license` | may | Copied from the object's manifest |
  | `label` | may | A name for people, as in 5.1 |
  | `base` | must, if the object has one | Copied from the object's manifest |
  | `history` | must, if the object has one | Copied from the object's `history.json` |
  | `reason` | may | Why it was removed. Free text, at most 1,000 characters |
  | `sources` | may | Known sources elsewhere, at most 3, and only `git` sources (5.1) |

- **The content is read from git, not from the working directory:** a tool reads the object at `path` in `commit` and copies the fields above from it.
- **What it says must be true** (6.4, item 8): `commit` is in the history of the new commit; at its `path` is the object the name says; `license`, `base` and `history` are those of that object.
- **It may be changed, removed or moved.** A changed removal record is checked like a new one. When the object comes back later, the removal record may stay.
- **Uses:**
  - Someone who looks for the name sees it marked as removed in the library list (5.2), and reads the record where the object was.
  - `commit` and `path` let them still fetch it from the history of this library (7.3); `sources` gives other places.
  - `history` lets the lineage of the object's descendants still be verified link by link (3.4, 8.3).
- The text of a removal record is written by others, and is treated as chapter 11 says.

### 4.3 Timestamp proofs

A library may keep beside an object proofs, made by third parties, that the object name existed at some time. This standard requires none.

- **The directory:** `<object name>.stamps/`, holding only regular files placed directly in it, each at most 64 KiB. A file name keeps item 3 of 3.1, and should be written `<date>-<short name>.<extension>`, such as `20261124-btc.ots`.
- **What is stamped is the object name:** its ASCII bytes, with no line end. A proof made for a name holds for that object wherever it is kept.
- **The extension says what kind of proof a file is:**

  | Extension | Proof |
  |---|---|
  | `.ots` | An OpenTimestamps proof |
  | `.tsr` | A time-stamp token of RFC 3161, in DER |

  Other extensions may be used. A tool that does not know one leaves the file as it is.
- **Proofs may be added, replaced or removed at any time;** an OpenTimestamps proof, for example, is replaced by its completed form once it is anchored. Nothing else in a library depends on them.
- **A tool checks a proof itself,** against the service it names. The library check looks only at the names, kinds and sizes of the files (6.4, item 5). What a proof shows, and what it does not, is said in chapter 11.
- **When an object is kept several times in a library,** each copy may have a stamps directory; together they count as one. A draft has none, since its name is not yet fixed.

## 5 Source lists

A source list is a file whose name ends in `.source.jsonl`. It lists some TOA objects, each with some cached information and zero or more known sources, and it may include other source lists. The source list beside an object, the library list at the top of a library and the user's workspace list are all of this one kind. A file inside an object directory is content of the object, never a source list, whatever its name, with one exception: a `members.source.jsonl` at the top of an object is a source list too (3.3), which a workspace list may include.

### 5.1 Format

- UTF-8 without a byte order mark, lines ending with LF. One JSON object per line. A line is at most 64 KiB.
- **Header:** the first line is `{"toa": "source/1"}`.
- **Every other line is an object line or an include line.** A line with `object` is an object line, a line with `include` an include line; a line with both or neither is invalid.
- **An object line says something about one object:**

  | Field | Requirement | Meaning |
  |---|---|---|
  | `object` | must | The object name |
  | `label` | may | Cached: a name for people, such as `strongarm-comp/wrap-lee.g2`, the readable name (6.2). Free text, from which tools must not infer anything |
  | `refs` | may | Cached: all its references (3.3, including `base`, the names in `lock.json` and the objects of `members.source.jsonl`), each name once |
  | `license` | may | Cached: its licence |
  | `group` | may | Its group (below) |
  | `removed` | may | Only in a library list: `true` means the object was removed from this library (5.2) |
  | `latest` | may | Only in a library list: `true` means this library recommends the object in place of the earlier generations of its series (5.2, 7.5) |
  | `sources` | may | A list of known sources |

  - `label` and `license` are strings of at most 256 characters. `refs` is a list of object names. The type of an object is its prefix and is not written in the list.
- **An include line brings in the content of another source list:**

  | Field | Requirement | Meaning |
  |---|---|---|
  | `include` | must | The path of the included list: `name/name/…`, each part keeping item 3 of 3.1, ending in `.source.jsonl`, at most 256 characters |
  | `sources` | may | Where to find the list. Each source points to a root directory (a library, or a local directory) to which the path of `include` is relative. A `git` source has no `path` here |
  | `group` | may | The group the included objects are put in |
  | `branch` | may | When the source is a library, its publication branch (6.3); `main` when not written |

- **What paths are relative to:**
  - in a list in a library (the library list, its volumes, the source list beside an object): `include` and `local` are relative to the root of the library;
  - in any other list: relative to the directory of the list.
  - An include line without `sources` refers to a list in the same place as the list that holds it: the same library, the same machine, or the same commit fetched from an address.
- **Expanding includes:**
  - A list is read once, however often it is included, so a cycle is read once too. At most 8 levels. The tool sets a limit on the total size of all lists.
  - A list that cannot be read affects only itself.
  - **The include lines of every list are followed,** whoever wrote the list. An include line with `sources` is resolved in the order of 7.3: what is already on the machine first (when one of its `git` addresses is the address of a library on the machine, that library is used), then its sources. Whether to connect to a host is for the tool and the user to decide; this standard does not say (chapter 11).
  - In the list of a library fetched from an address, a source with a path relative to the library is a place in that library: the `git` address of that library with this path.
- **Groups:**
  - `group` is a group path: 1 to 4 parts separated by `/`, each keeping item 3 of 3.1, at most 128 characters in all.
  - The group of an object is the `group` of each include line on the way to it, joined in order, followed by the `group` of its own line. A level without `group` adds no part.
  - An object included along several ways appears under each of their groups.
  - The type of an object is not part of its group: tools group by prefix.
  - Groups are only for people. Looking up, verifying, references and fetching ignore them.
- **Every source is one of:**

  | Written | Meaning |
  |---|---|
  | `{"git": address, "commit": commit, "path": path}` | In a git repository. `commit` and `path` may be left out. No other keys |
  | `{"local": path}` | In a directory on the machine. No other keys |

- **A `git` address:**

  ```text
  https://host[:port]/path
  ssh://[user@]host[:port]/path
  ```

  - Host: parts separated by dots, each 1 to 63 letters, digits or hyphens, not starting or ending with a hyphen. Written in lower case.
  - Port: 1 to 65535.
  - User: only in an `ssh` address. `A-Z a-z 0-9 . _ -`, not starting with a hyphen, at most 64 characters.
  - Path: `A-Z a-z 0-9 . _ ~ / -`; no part is empty, `.` or `..`.
  - No password, query or fragment. The whole address is at most 512 characters.
  - **Two addresses are the same** when, with the host in lower case and a trailing `/` removed, they are the same string.
- **`commit`** is 40 lowercase hexadecimal digits: any commit of that repository that holds the object. It should be the earliest commit that holds it, counting back along first parents on the publication branch.
- **`path`** is the path of the object directory in that commit: relative, separated by `/`, each part keeping item 3 of 3.1.
- **`local`** is, on an object line, the path of the object directory itself, and on an include line the path of the root directory. It is absolute, or relative as said above. It is separated by `/`, at most 1,024 characters, and holds no control characters and no invisible format characters.
- **Later forms of source:** a source with neither `git` nor `local` is ignored when read.
- **Cached information can always be computed again from the object** (except `label` and `group`; `removed` is computed from the removal record). `latest` is not cached information: it is what the library says. Where it differs from the object, the object prevails.
- **The same object may be on several lines;** they are merged when read.
- **Reading the lists of others, leniently, from the largest unit down:**
  - A list over the size limit, or whose header has a version the tool does not know, is ignored as a whole. A list without a header is read as this version.
  - A line that is not a JSON object, an object line whose `object` is not an object name, and an invalid include line: the line is dropped.
  - A cached field of the wrong shape, an invalid source: only that field or source is dropped.
  - Only the first 3 sources of each object are used. Unknown fields are ignored.
- **The lists on a publication branch are checked strictly** (6.4, item 6): the header is `{"toa": "source/1"}` and nothing more; every line, field and source is valid; every object and every include line has at most 3 sources; the `git` sources of include lines have no `path`; an include line without `sources` points to a list in the same commit; `removed` and `latest` occur only in the library list and its volumes; local paths are only those 5.3 allows. Which addresses a list points to, and which libraries' lists it includes, is not restricted.

### 5.2 Where they are used

| Place | File | Content |
|---|---|---|
| Beside an object | `<object name>.source.jsonl` | The object itself, and the objects needed to use it |
| At the top of a library | `library.source.jsonl`, and in volumes the lists it includes under `library.source/` | All objects of the library, and those removed with a removal record left; it may include other lists |
| Inside an object | `members.source.jsonl` at the top of the object | A list of members, frozen with the object (3.3); a frozen library lists its members and the objects they need (type `lib`) |
| In the user's working directory | `workspace.source.jsonl` (the default name) | All objects the current design uses: usually the library lists of some libraries, included (8.1) |
| Anywhere else | Any file whose name ends in `.source.jsonl` | Caches of tools or users |

- **The source list beside an object:**
  - Its first line is the object itself. Whoever takes the object with this list has, at the same time, what it refers to and where those are. Before publication this line has no source; publication adds it (5.3).
  - The other lines are the objects needed to use it, found as follows:
    1. All its references (3.3), including the objects `base` points to.
    2. For every object found so far whose type is followed (below), not counting those reached only through `base`: add its references, except its `base`. Repeat for the objects added, until none is new.
    3. Which types are followed is said by their type specifications. In `0.1`, interfaces, circuits, testbenches and frozen libraries are followed, a frozen library through its list of members; evidence records and documents are not, nor are types the tool does not know. Only the objects `base` of the object itself points to are listed; earlier ancestors in `history.json` are not.
  - These lines carry cached information too. Whoever uses the object knows, before fetching anything, which objects the whole tree has and where each is.
  - Order of sources: `git` sources first, local paths after. After the tidying of publication each object keeps at most 3.
  - It may have include lines too, for example to include the library lists of the libraries it uses.
- **The library list:**
  - One line per object, sorted by object name. Sources are `{"local": path}`: the path is relative to the root of the library (in volumes too) and has only the form `name/name/…`, each part keeping item 3 of 3.1. For an object kept several times in the library, the paths of the first 3 copies.
  - **Groups:** each object may have a `group`, for example by design. The library decides how; the sample libraries take the interface name of the recommended arrangement (6.2).
  - **Objects removed with a removal record left** (6.5) are listed too, with `"removed": true`; their source is the path of the removal record without the trailing `.removed.json`, usually where the object was.
  - **Latest marks:** the line of an object may have `"latest": true`: this library recommends the object in place of the earlier generations of its series (3.4). The mark counts only here, in the library list and its volumes, like `removed`; written in any other source list it means nothing.
    - A marked object is in this library and not removed. Of two marked objects of one series in this library, neither is in the `history` of the other (6.4, item 6). Two marked objects that branched from one ancestor are allowed; a tool reports both.
    - Marks may be moved or taken away at any time: to a better generation when one comes, or away from one found to be wrong.
  - Someone looking for something in a library reads this list first, instead of walking the whole library.
  - **Including other lists:** the maintainer may add include lines to the library list, for example to include by address the lists of the libraries whose objects this library uses. A user who includes this one library then finds the whole tree. When a tool writes the list again, it should keep these include lines.
  - **Volumes when it grows large:** the object lines go into several lists under `library.source/`, and `library.source.jsonl` keeps the header, the include lines of the volumes and the include lines the maintainer added. They should be split by the first two digits of the hash, `00.source.jsonl` to `ff.source.jsonl`, so that looking up a name reads one volume. A volume is at most 16 MiB. Every list under `library.source/` must be included.
  - The object lines are derived, written by the tools of the maintainer's side, and may lag behind. Where the list and the library differ, the library prevails.
  - A contribution should not change it; the maintainer writes it again after merging. After merging two branches it is always written again, whether git reported a conflict or not.

### 5.3 Local paths and publication

- **On the machine, a source list may hold local paths.** It is then a local cache too, which saves walking directories every time.
- **On a publication branch:**
  - the source list beside an object has no `local` source at all;
  - the `local` sources of the library list and its volumes are only the paths relative to the library described in 5.2;
  - which `git` addresses a list holds, and which lists it includes, is not restricted.
- **Publishing (7.2), a tool changes only the source lists beside the objects published this time,** and must:
  1. Remove every `local` source. An include line whose sources were all local is removed as a whole.
  2. Add, to every object of the list that is in this library (the object itself and the objects it needs), the address of the library it is being published to and the path of the object in it. An object already on the publication branch gets its commit too; one published only now has none.
  3. Keep the `git` addresses of other places.
  4. After this, for every object of the list that is not in this library and has no `git` source left: keep only `object` on its line, drop the cached label, references and licence, and list the object to the user. Once public, others cannot find these objects by name (chapter 11).
  5. List to the user the ancestors in the `history.json` of these new objects that are neither in this library nor in a library with a `git` address in the workspace list.
- **What to show the user before writing is for the tool.** The addresses of other places become public with the list, and the user name in an `ssh` address may be a personal account. A tool may list the addresses it will write and let the user remove those that should not become public (chapter 11).
- **The address of this library:**
  - given by the user; when not given, the address of the remote named `origin`.
  - An `https` address loses its user name and password. The host is written in lower case.
  - A tool does not rewrite the form `user@host:path` by itself: on an ordinary server its path is relative to the home directory, and as an `ssh://` address it may point elsewhere. The user is asked for an address of the form `https://` or `ssh://`.
  - An address that does not keep 5.1 (a local directory, for example) is not added, and the user is told.
- **The source lists beside objects already published are not changed.** Objects brought in by a fork keep lists that point to the original library.
- **When a contribution is merged into another library,** the maintainer's side of that library tidies the new objects in the same way: it adds its own address and keeps the earlier ones.
- **`local` sources in a list obtained from elsewhere:** only paths relative to a library, in a list in that library, are used; they are places in that library, and once resolved they must still be within its directory. Others are ignored.

### 5.4 A cache

- A source list that is missing, out of date or wrong does not affect whether an object is valid.
- A tool must not reject an object because a source list is missing or fails; when one way does not lead anywhere, it takes another (7.3).
- Everything in a source list is written by others, and is treated as chapter 11 says.

## 6 Libraries

### 6.1 What a library is

- **A library is a git repository, the whole repository.** This is how version `0.1` publishes and shares objects; the objects themselves do not depend on git (1.1). Version `0.1` does not define a directory within a repository as a library. A repository may hold other files, but this section and 6.2 and 6.4 apply to the whole repository.
- **A library is not a level with a meaning of its own:** the identity and the references of objects do not depend on it, and it has no identifier.
- **The repository must use the SHA-1 object format.** A tool that creates a library should say so explicitly, and a tool that reads one should check it.
- **The whole library holds only regular files and directories.** On the publication branch every entry has the mode `100644` or `40000`: no symbolic links, no submodules, no executable files.
- **There is exactly one `.gitattributes`, at the root of the library, named `.gitattributes` in lower case.**
  - It must have a line `* -text`.
  - No other line may set `text`, `eol`, `crlf`, `filter`, `ident` or `working-tree-encoding`, or define a macro (`[attr]`).
  - No `.gitattributes` may be anywhere else, in any case. On a file system that ignores case, git reads `.GITATTRIBUTES` as if it were this file.
- **No `.gitmodules` may be anywhere,** in any case.
- **The root of a library may hold:**

  | File | Requirement | Content |
  |---|---|---|
  | `library.yaml` | may | `toa: library/1`; `name`, `title` and `description` for display |
  | `library.source.jsonl`, and in volumes `library.source/` | should | The library list of all objects (5.2) |

- **A library writes neither its own address nor its own publication branch.** An address or branch name written in the library would not hold for a fork or a mirror. The one exception is the addresses that publication adds to source lists, which are caches.
- **A library need not be complete in itself:** its objects may refer to objects in other libraries.

### 6.2 Arrangement

- **A tool must find objects at any depth of a library, and must not read any meaning from their paths.** Only the name of an object directory has meaning.
- **The same object may be kept several times in one library;** all copies are the same object, and this is not an error. An object name may only be the name of a directory.
- **The recommended arrangement:**

  ```text
  ifc/strongarm-comp/ifc_945cdda7…/
  ifc/strongarm-comp/ifc_945cdda7….notes/20261007-clk-edge.md
  ifc/strongarm-comp/ifc_945cdda7….source.jsonl
  ckt/strongarm-comp/wrap-lee.g2/ckt_1ec6249c…/
  ckt/strongarm-comp/wrap-lee.g2/ckt_1ec6249c….source.jsonl
  ckt/dyn-latch-cmp/lee-sky130.g1/ckt_f1ede73a….removed.json
  tb/strongarm-comp/tb_offset.g1/tb_ad96a147…/
  evd/strongarm-comp/20261103T1200Z-wrap-off/evd_b7d77959…/
  doc/strongarm-comp/kickback-mitigation/doc_044f932c…/
  lib/toa-official.g3/lib_5d0e7b3a…/
  library.source.jsonl
  ```

  - The first level is the type directory, named by the prefix; the second is the interface name; the third the rest of the readable name; last comes the object directory. An interface has no third level; a document about no interface, and a frozen library, have no interface level.
  - A tool makes the readable name from fields, and from the generation (3.4), when it creates an object, and does not check later that it still agrees with them. In `wrap-lee.g2`, `g2` is the generation.
- **Every name of a level outside objects** keeps items 3 and 4 of 3.1, except that it may start with a dot (`.gitattributes`, `.gitignore`, `.github/`).
- **Path budget:**

  | Part | At most |
  |---|---|
  | The path before the object directory, with its trailing `/` | 60 characters |
  | The object name | 49 characters (a prefix of at most 8, the underscore, and 40) |
  | The path within the object (3.1), with its leading `/` | 81 characters |
  | In all: the path of any file in the library | 190 characters |

- **Windows:** with git's default settings, a checkout fails at a full path of 260 characters, and then the whole library cannot be checked out. A tool that clones a library should turn on `core.longpaths` for that repository.

### 6.3 The publication branch and "published"

- **Which branch is the publication branch is said by those who use the library:** the `branch` of the include line that includes the library (5.1, 8.1), `main` when not written. What the user's own lists say comes first. The maintainer of a library says it for their own library in the same way.
  - A branch name: `A-Z a-z 0-9 . _ / -`, starting with a letter or digit, at most 100 characters, not containing `..`, not ending with `/` or `.lock`.
- **An object is published in a library** when its directory is in the latest commit of that library's publication branch: others can get it from this library now.
- **Deciding this in a checkout:**
  - When the branch has a remote-tracking ref (`refs/remotes/origin/<branch>`), that ref decides. An object committed locally but not yet pushed is not published.
  - In a repository without remote-tracking refs (the library itself, or a local library without remotes), the local branch decides.
  - In a repository that has remote-tracking refs, but none for this branch (a branch never pushed, for example), the local branch decides too. Objects committed only locally then count as published, which errs on the safe side: they cannot be unsealed (7.1). A tool should tell the user that the branch has no remote-tracking ref.
  - A repository that has commits but not this branch is an error, and must not be taken as "nothing is published".
- **This is the only test of whether an object is in a library.** That the object can be fetched by name from the library's address does not mean it is in the library: objects on unmerged branches or in dropped commits may be fetched too.
- **The commit that puts an object on the publication branch should be signed,** with the SSH or GPG signing that git has. A tool that commits signs by the user's git settings. A verified signature shows who put the object into this library, in that commit; it does not show who wrote the object, and the date of the commit is still the one the committer wrote (chapter 11).
- The `commit` of a source list can be used to confirm where an object came from: it is an ancestor of the publication branch, and the tree hash at its `path` equals the hash of the object.

### 6.4 The library check

- **The maintainer of a library must check every new commit as below before moving the publication branch to it.**
- **The previous head** is the current head of the publication branch. The new commit must be a descendant of it; if it is not, history was rewritten, and it is rejected.
- **Removal** (6.5) is not restricted: no list of removals is required, and no removal record.
- The check needs only git's listings and a few small files. The size of every file is looked at before the file is read. Listings are counted while they are read, and rejected as soon as they exceed the limit the deployment sets, not after they have been read in full.
- **A commit whose listing cannot be read is rejected.**

1. **Repository and whole library:** the repository has the SHA-1 format; `.gitattributes` and `.gitmodules` keep 6.1; the library has no symbolic links, submodules or executable files; the new commit is a descendant of the previous head.
2. **Every directory whose name is an object name:**
   - its tree hash equals the hash in its name;
   - the canonical tree hash rebuilt from its listing equals it too (7.4);
   - it is a well-formed object tree (3.1), and every file listed exists and is a file;
   - when an object is kept several times, every copy is checked so;
   - an object name is not the name of anything other than a directory.
3. **No draft names** (the places of 2.4).
4. **The files of every object:** `toa.yaml` keeps the syntax of 3.2, its common fields are valid and its `type` is the prefix; `lock.json` and `members.source.jsonl`, where present, keep 3.3; `history.json` is written the one way and agrees with `base`, and its `generation` with the parent where the parent is in the library (3.4); every item of `base` has the prefix of the object.
5. **Outside objects:** the rules on the names of every level; the path budget; names that start with an object name and a dot are only of four kinds: the `.notes` directory, the regular file `.source.jsonl`, the regular file `.removed.json` and the `.stamps` directory, which holds only regular files placed directly in it, each at most 64 KiB, named by item 3 of 3.1 (4.3).
6. **Source lists** (every file outside object directories whose name ends in `.source.jsonl`; a file inside an object directory is content of the object, not a source list; a `members.source.jsonl` at the top of an object is checked by item 4): within the size limits; valid when read strictly (5.1); no `local` source beside an object; the `local` sources of the library list and its volumes are paths relative to the library; an include line without `sources` points to a list in this commit; every list under `library.source/` is included from `library.source.jsonl`, within 8 levels. `latest` marks keep 5.2: every marked object is in this library and not removed, and of two marked objects of one series, neither is in the `history` of the other. Where lists point is not checked.
7. **Notes directories:** only regular `.md` files placed directly in them, each at most 256 KiB; notes of the same name beside copies of the same object have the same content; compared with the previous head, no note (known by object name and file name) was changed or removed, except the notes of removed objects.
8. **Removal records are true:** every removal record added or changed keeps 4.2 and is true. Objects that disappeared need not leave a record, and existing records need not stay.
9. **The references not in this library are listed.** Whether a public library allows them is for the deployment to say.

- An error in one object is reported for that object only, and must not keep the check from going on with the others.

### 6.5 Removal

- **This standard does not restrict removing objects.** What is removed is this copy in this library. The object name stays, and so do references to it from others, the records of it in every `history.json`, and the copy in this library's git history.
- **A removal record is not required** (4.2). With one, others can still follow the object; without one, the current state of the library holds no trace of it (git history still does; see below). A tool that removes objects should be able to leave one where the object was; the library check only checks that records left are true (6.4, item 8).
- The source list beside the object is removed with it (when a removal record is left, the `git` sources of other places in that list are copied into it); the notes directory and the stamps directory may stay or be removed too.
- In the library list: an object with a removal record left is marked `removed` (5.2); one without has its line removed.
- **Withdrawing an object is marking it** (6.6), not removing it: a withdrawn object is still there; the library only says not to use it any more.
- **Removing does not mean others cannot get it.** What was pushed to a public library should be treated as public: git history still has it, and others may have copied it long ago. To erase it from history (for example restricted content uploaded by mistake), history must be rewritten, which the library check rejects (item 1); the deployment deals with this outside this standard.
- Source lists are caches, and may be removed or written again at any time.

### 6.6 Other files of a library that change

States such as withdrawn and superseded, aliases, the register of processes and the rules for contributions are kept in files that the maintainer of a library looks after. Their formats are outside version `0.1`. They speak only for their own library: of one object in two libraries, each library says its own.

## 7 Operations

### 7.1 Sealing and unsealing

Sealing turns drafts into objects. It is done before committing, and may be done again and again.

- **It completes, or it changes nothing.** When any step fails (including a directory held open by another program, or a rename that fails), every file and directory name already changed must be restored.

1. **Tell four kinds of directory apart:**
   - a directory with a draft name: to be sealed now;
   - a directory with an object name whose content matches its name: left alone;
   - a directory with an object name that is not a well-formed object tree, or whose content does not match its name: **stop with an error, without renaming anything.** It may be someone else's object that was changed;
   - an object name that occurs several times: each copy by the two rules above (6.2).
2. **Order the drafts by what they depend on,** the drafts named first. A draft depends on every draft it names, in `toa.yaml`, in `lock.json` and in `members.source.jsonl`. A cycle is an error.
3. **For each draft, in that order:**
   1. Put the object names obtained so far in place of the draft names: in every value of `toa.yaml`, in the values of `lock.json` and in the values of `members.source.jsonl`. A value of the form of a draft name left after this is an error.
      - **Only the characters of the value are replaced.** Quotes, indentation, comments and all other bytes of the file stay as they are. A draft name and an object name both use only lowercase letters, digits, underscores and hyphens, which need no escape in any form of scalar, so the replacement has one result.
      - **So a draft name in a draft must be written as it is:** in `toa.yaml`, as a scalar on one line without escapes; in `lock.json` and `members.source.jsonl`, as a JSON string without escapes. Sealing stops with an error on a draft name written otherwise.
      - **A file that needs no change is left as it is,** not one byte changed.
   2. For a draft with `base`, write `history.json` (3.4). When the first item of `base` cannot be found on the machine, or is not verified, it is an error. For a draft without `base`, remove any `history.json` in it. A draft may write `base: []`, which must then be a line of its own at the top level; sealing removes that line and leaves the other bytes as they are.
   3. Check: a well-formed object tree; the syntax and common fields of the manifest; `type` equals the prefix; what the type specification asks. A tool seals types it does not know as well; it only cannot do the last of these checks.
   4. Compute the hash **in both ways:** once from the files by 2.2, and once by handing the files to git with the actual settings of this repository (ignore rules, attributes and file modes all apply; for example `git add` with a temporary index, then `git write-tree`). When the two differ, it is an error, and the tool says why (often a file ignored by `.gitignore`, an executable file, or converted line ends).
      - **A draft that is not in a git repository** has only the first computation. The tool must tell the user that the second was not made. When the object is later committed to a library, verifying it before publication (7.2) and the library check (6.4, item 2) find what the second computation would have found.
   5. Rename the directory to `<prefix>_<hash>`. Its notes directory and source list are renamed with it. When something already has the new name, it is an error (two drafts of the same content in one directory, for example). That the object is elsewhere in the library already is not an error.
   6. Write its source list (5.2).
4. **Sealing needs no saved table of names.** Drafts refer to each other by their draft names, and that is enough.

- **Unsealing:** to change an object after sealing it and before publishing it, first turn it back into a draft.
  - The tool must at the same time turn back into drafts the objects that refer to it and are not published either (level by level up, until there are none), and put draft names in place of the names among them, in `toa.yaml`, `lock.json` and `members.source.jsonl`, replacing characters as the first sub-step of step 3 does. Sidecar files are renamed with them.
  - The tool chooses the draft names. When something already has such a name, it is an error. When an object to be unsealed is kept more than once in the library, it is an error too: the user is asked to remove the extra copies first.
  - A published object (6.3) must not be unsealed.
- **A commit hook is optional and only checks:** it refuses a commit that has draft names or directories whose names do not match. It does not rename anything.
- **The real guarantee is the library check of 6.4,** not the tools on a user's machine.

### 7.2 Publishing

Before objects that have been sealed are put on the publication branch, a tool must:

1. Make sure there are no drafts; verify from the files each object published this time (7.4); and run the checks of 6.4 that can be made on the machine.
2. Tidy, by 5.3, the source lists beside the objects published this time. An object without one is given one.
3. Write the library list again (the maintainer's side may do this after merging).

- **These changes are made in a commit.** A git push cannot change commits that already exist, so "removing local paths when pushing" cannot be done; what can be done is to check once more before pushing, and not to push what fails.

### 7.3 Fetching

Fetching gets the files of an object by its name.

- **Where to look, in order:**
  1. What is already on the machine: local libraries and local caches, by name. **It must be verified from its files before it is used** (7.4): a local copy may have been changed too.
  2. What the workspace list gives: the objects it lists, and the libraries it includes. For a library with a library list, the list is read first, then the object fetched.
  3. The addresses given by the source lists at hand.
- **Whether to connect to a host** is for the tool and the user to decide; this standard does not say (chapter 11).
- **Fetching from a git address takes fixed steps:**
  1. **The name first.** The name to fetch must be a whole object name; otherwise nothing is done. At most 3 sources are tried for an object.
  2. **An invalid source** (5.1) is dropped and never handed to git.
  3. **Calling git:** the address is placed after `--`, so that it cannot be read as an option. Git must not prompt for input; a tool that needs to ask the user asks before calling git. It does not follow redirects. A timeout is set, and when it runs out, git and the processes git started are ended. Only the transports `https` and `ssh` are allowed (git's `GIT_ALLOW_PROTOCOL`). No remote pointing to this address is left on the machine.
     - **Credentials** (the user's git credentials, credential helpers, keys) are used, like connecting to a host, as the tool and the user decide: a tool must not use them for a host the user has not agreed to. What automated services use is for the deployment (chapter 11).
  4. **Git checks what it receives** (`fetch.fsckObjects`).
  5. **Before anything is written to disk,** the listing is read, and every level of every path is checked by 3.1, as are modes, sizes and numbers. The listing is counted while it is read, and reading stops beyond the number of entries a valid object can have.
  6. **The files are written, as raw bytes, into a temporary directory.** Before each file is written, its target path is confirmed to be within the temporary directory. Then the hash is computed again by 2.2, and the syntax of the manifest and `type` equals the prefix are checked. Only when all of this passes is the directory renamed into place; when any step fails, the temporary directory is removed completely.
  - Steps 4 and 5 each stand on their own: neither may be left out because the other was done.
- **Sidecar files, the lists of libraries and whole libraries take these steps too.**
  - To fetch a list (an include line of 5.1), the latest commit of that branch is fetched, and only that one list is read in it, its size looked at first.
  - The notes and source list beside an object are fetched by the `commit` and `path` of a source.
  - A whole library is cloned without a checkout first, and checked out only after its listing has been checked by items 1 and 5 of 6.4; or it is never checked out, and only what is stored in git is read. A symbolic link in a notes directory, once checked out, would point outside the library.
- **Fetching by name directly:** `git fetch <address> <hash>` fetches only this one directory. It needs protocol version 2 of git (the default from git `2.26`), or a server that allows fetching any object by its id.
- **Fallbacks:** when fetching by name fails, the `commit` and `path` of the source are used; failing that, the whole library is cloned. When only a removal record is left in the library, its `commit` and `path`, or its `sources`, are tried (4.2). When the server ignores the filter of a partial clone, the tool must tell the user that this was a full clone.
- **Several at once:** to fetch several objects, one request is made per address. A single name the other side does not have makes the whole request fail; it is then split up and retried, and the failing address is remembered for the rest of this run.
- **Keeping them:** a fetched object should be written as files. When it is kept only in git's store, a ref must be made for it, or git will clean it away.
- **Large files:** for an object with large files, the directory is fetched first, then only the files missing on the machine.
- **Sidecar files do not come with the object.** Fetching by name gets only the object directory.
- **Passing objects offline:** with `git bundle`, or by copying object directories. What is received is always verified from its files.
- A fetched object proves only that its content is the one its name stands for. Whether it is in some library is decided separately, by 6.3.

### 7.4 Verifying

| Way | What it does | When it holds |
|---|---|---|
| Verifying from the files | Compute the hash again by 2.2 and compare it with the directory's name; check 3.1; check the syntax of the manifest (3.2); check that `type` equals the prefix | Anywhere. It is the final word |
| Verifying from the listing | Read only git's listing: the stored tree hash equals the name; the canonical tree hash rebuilt from the listing equals it too; the modes and names of entries keep the rules | In a repository obtained through git with `fsckObjects` on, or after `git fsck`. Not in a repository that was copied |

- **Rebuilding:** the files of the listing are put together into trees by the rules of 2.2 for directories, with the hashes of files taken from the listing, without reading their content. When the stored tree has modes with a leading 0, entries out of order, entry names with `/` or empty directories, the rebuilt hash differs from the stored one.
- **When objects must be verified:**

  | Occasion | Way |
  |---|---|
  | An object obtained from elsewhere, before it is written into place | From the files (7.3, step 6) |
  | An object already on the machine: before it is used (expanded, assembled, its external dependencies listed), before its `history` is copied, before it is published | From the files |
  | The library check: every object of the new commit | From the listing, and item 4 of 6.4 |
  | An index taking in an object | From the listing, and the checks of the manifest |

- Objects that are exported or copied must be written as raw bytes. Ways that convert line ends by the settings of the machine, such as `git archive <hash>`, must not be used.
- In a git working directory, when verifying from the files fails while git reports no change, the cause is usually conversion at checkout or stray files in the directory, not a changed object.

### 7.5 Updates and subscriptions

- **The git history of a library is what it broadcasts.** Comparing the sets of object names of two commits gives the objects added and those that disappeared; those that disappeared must be reported, with the content of their removal record where one was left. A tool remembers the last commit it saw of each library; when that commit is not an ancestor of the new one, the tool must say so and read the whole library again.
- **A referenced object P has a newer version N** when all of these hold:
  - a library the user reads marks N `latest` in its library list (5.2);
  - the verified part (8.3) of the `history` of N holds P;
  - N and P are in one series (3.4).
- **When N and P have different `ifc`,** the interface has changed, and a tool must say so. Following then changes the `ifc` that goes with the reference as well, such as the `ifc` of an item of `uses` together with its `ckt`; where the pins changed, the netlist must change too.
- **Which libraries say so is for the user to weigh.** A tool reports which libraries mark N, and on the publication branches of which libraries N and P are found; it does not conclude who made N.
  - A mark is what one library says. A fork carries all objects of its original library and may mark what it likes, so a mark in a fork does not show that N is from the original author; users judge by which libraries they trust.
  - A public platform may add: both were added by the same verified account.
  - The `author` an object writes about itself must not be used to judge this. The `name` only says which generations a mark covers (3.4): someone who takes the same name can mark the object only in their own library, and the user sees which library that is.
- **Descendants that no library marks** are listed only when the user asks for them; they are not offered as newer versions.
- **Following is voluntary, and level by level.** The object that refers copies itself to a draft, changes the reference and is sealed, which gives its newer version; existing evidence records stay with the old version. Following once makes a new object at every level of referring objects above; a tool should tell the user first how many there will be.
- **There is no shortcut through evidence records:** a record tests the expanded tree of its circuit as it is, and cannot use another version of a submodule in place of the one the tree names (`evd` type specification).

### 7.6 Assembling

Assembling turns a circuit, with its expanded tree, into a set of files that can be simulated directly. To run a testbench, a tool assembles it together with the circuit tested, once for each set of conditions.

- The whole tree is expanded by references; each object is expanded once; each object is verified before it is expanded (7.4).
- When subcircuits have the same name, the tool renames them.
- All external dependencies of the whole tree (3.5) are listed and matched one by one in the local file map; `match` and `sha256` are checked, and the lines that include the model files are written.
- `lock.json` is written: the name of every object of the tree, and the SHA-256 of each file the type specification names (chapter 9).
- **The local file map:** a tool should look for `local-files.yaml` in the directory of the workspace list by default; the user may name another file. Whether a tool also has a map shared by all workspaces of a machine, and where, is for the tool. Its format (for illustration):

  ```yaml
  toa: local-files/1
  process:
    sky130:
      release: <name of the release of these models on this machine>
      models:
      - file: <path on this machine>/design.scs
      - file: <path on this machine>/models.scs
        sections: {typical: tt, slow: ss, fast: ff}
  external:
  - name: <the name of an external item of an object>
    process: <optional: applies only to objects of this process>
    path: <path on this machine>
  ```

- **How it is matched:**
  - The process an object names (in `0.1`, the `specifications.process` of a circuit) is looked up by name under `process` in the map.
  - Each `external` of an object is matched with the entry of `external` in the map that has the same `name`; an entry with `process` must also have the `process` of that external file (or else the process its object names).
  - Every string of `match` must occur in its file.
- **Assembling a run.** An evidence record of the kind `sim` (`evd`) is made by running each of its testbenches on the circuit tested under one set of conditions. For each testbench, the tool writes a directory holding:
  - `tb/`: the files of the testbench, at their paths in the object;
  - `ckt/<object name>/`: the files of each circuit of the expanded tree, at their paths in the object;
  - the entry file, the only file the simulator is given: `input.scs` when the first netlist of the testbench is in Spectre, `input.sp` when it is in SPICE. A netlist whose name ends in `.scs` is in Spectre and any other is in SPICE, as Spectre itself reads included files.
- **The entry file** holds these, in this order, and nothing else:
  1. a first line: `simulator lang=spectre` in Spectre; in SPICE, whose first line is a title, a comment;
  2. the model files of the conditions (`models`), as the local file map gives them, each with its sections;
  3. the netlists of the circuits of the expanded tree, in the order of `tree` in `lock.json`, so the circuit tested comes first; for each circuit, the netlists of `files.netlist` in their order;
  4. the first netlist of the testbench;
  5. the conditions of the run: the values of the parameters of the testbench and the options of the simulator;
  6. in SPICE, a last line `.end`.
- **How the lines are written:**

  | Line | Spectre | SPICE |
  |---|---|---|
  | a model file | `include "<path>"` | `.include "<path>"` |
  | a model file with sections | `include "<path>" section=<section>`, one line per section | `.lib "<path>" <section>`, one line per section |
  | a netlist of an object | `include "<path>"` | `.include "<path>"` |
  | the parameters | one statement `parameters <name>=<value> …` | one statement `.param <name>=<value> …` |
  | the option `temp` | with the other options | `.temp <value>` |
  | the other options | one statement `toaConditions options <name>=<value> …` | one statement `.options <name>=<value> …` |

  - The path of a netlist is relative to the directory; the path of a model file is the one in the local file map. The section written is the one that the `sections` of the map give for the section of the conditions, or that section itself when they give none.
  - A number is written as a decimal number, a boolean as `1` or `0`, and a string as it is: the author writes it as the simulator wants it.
  - The netlists of the testbench declare none of its parameters and set none of the options of the conditions (`tb`); when they do, the tool stops.
  - A SPICE netlist in a Spectre run is read as SPICE, by its name. A Spectre netlist cannot be used in a SPICE run: the tool stops.
- **The block measured:** in the copies of its netlists in the directory, the subcircuit of the circuit tested takes the name in `socket` of the testbench; its ports stay in the order of the pins of the interface. Other subcircuits of the tree are renamed when their names clash with it, with each other, or with subcircuits that the testbench defines.
- **Probes:** in the copies of the netlists of the testbench, each `<instance>.<probe>` (`tb`) becomes `<instance>.<net>`, with the net that `probes` of the record gives. Before running, the tool checks that each such net is in the circuit tested: a net of its subcircuit, or, for a net further down, the instances down to it and the net. When a net is missing, or the tool cannot tell, it stops: it never runs with a probe that leads nowhere.
- **The results** are written in the format of `analyze.results` of the testbench into a directory `<results>`, with `conditions.json`, and then the analysis script is called (`tb`).
- A tool must not run commands carried by objects on its own, and must not install dependencies that objects list on its own (chapter 11).

## 8 Workspace list and index

### 8.1 The workspace list

The workspace list is a source list (5.1) in the user's working directory that lists all objects the current design uses. Its default name is `workspace.source.jsonl`.

```text
{"toa": "source/1"}
{"include": "library.source.jsonl", "group": "official", "branch": "main", "sources": [{"git": "https://example.org/rnl-hub-lib.git"}, {"local": "../checkouts/rnl-hub-lib"}]}
{"include": "library.source.jsonl", "group": "mylib", "sources": [{"local": "../mylib"}]}
{"include": "team/shared.source.jsonl", "group": "team"}
{"object": "ckt_…", "group": "mine/blocks", "sources": [{"local": "blocks/ckt_…"}]}
```

- **To use a library is to include its library list.** The `sources` of the include line say where the library is: an address others can use too, and where it is checked out on this machine. When there is a local checkout, a tool may walk that library instead of reading its list (the list is a cache, 5.2).
- When the list of a library includes the lists of other libraries (5.2), those libraries come in too. When the address of one of them is the address of a library in the workspace list, the local one is used (7.3).
- It may also include other lists of the working directory, or list objects directly (lines 4 and 5 of the example).
- **A frozen library is used in the same way:** an include line whose `include` is the path of its list of members in a library, such as `lib/toa-official.g3/lib_5d0e7b3a…/members.source.jsonl` (type `lib`). A tool may also expand a frozen library given by its name.
- **Scope:** all objects listed once it is expanded form a scope, in which references are looked up by name (3.3).
- **Groups:** the `group` of an include line usually names the library, and the `group` in the library list adds a level (a design, for example); the level of the type is given by the prefix. The user sees levels such as `official/strongarm-comp/circuits/…`, not a flat list of objects.
- **Lists the user wrote and lists others wrote are read the same way.** Tools may treat them differently only when connecting to hosts: the tool of the sample libraries connects without asking to the hosts the user wrote into their own list, and asks before others. This is how that tool does it; this standard does not say (chapter 11).
- A workspace list holds local paths, and should not enter a public library (5.3).

### 8.2 What the index is

- The index is derived data: what is found by reading the libraries a workspace list includes. When lost, it can be built again.
- The library list is what each library hands to an index ready-made. An index may use it directly, but the list is a cache; where trust is needed (a platform taking in objects), it must be checked against the objects themselves.
- The index answers three kinds of question:
  - **Reverse lookup:** who refers to an object directly; whose `base` names it.
  - **Where and when:** in which libraries, at which paths it is (one library may have several); which commit of each library's publication branch first held it, and on which day.
  - **State:** the notes of each library on it; each library's removal records, `latest` marks, and withdrawn and superseded marks.

### 8.3 Requirements on the index

- **Only direct relations are stored.** "All descendants of an object" is found at query time by following `base` step by step.
- **`history.json` is recorded in two parts:** from the object, the links that can be followed along `base`, each link's `history` agreeing, form the verified part; what comes after is the declared part. The `generation` is verified when the first item of `base` can be found and agrees with it, and declared otherwise. An object whose `history` or `generation` disagrees with the chain of `base` that can be found is marked invalid.
- **When a link is left in its library only as a removal record,** it is checked against the `history` of the removal record, after the removal record itself has been checked by 4.2.
- **Reverse lookups and subscriptions use only the verified part.**
- **Objects whose `base` cannot be found are kept in a column of their own:** the nearest ancestor in their `history` that can be found is recorded, marked "declared by the object itself, with a gap". This column is only for people; it is not mixed into the reverse lookups above, and is not used to judge newer versions.
- **Dates:** the committer date of the earliest commit that held the object, counting along first parents on the publication branch. Committers write their own names and dates. Where that commit is signed (6.3), the index records who signed it and whether the signature verified. A public platform should also record when it first took in the object, and the verified account.
- **The same files without a declared lineage.** An index should point out an object O that holds a file byte for byte the same as a file of another object P (the two hashes of 2.2 are equal), when all four conditions below hold. A tool that checks objects on a machine should say the same, within the objects it can find. The hint names the earliest such P and the files; it may list the other objects too, and an index may say on the page of P which later objects hold its files. It is a hint for people: never a reason to judge an object invalid, and not counted among the problems of an object.
  - **P is earlier than O.** Within one library, the order in which the two first appeared along first parents of the publication branch decides. Otherwise the tool uses the dates it has, such as the day it first saw each object and the dates above, and says which it went by; committer dates are written by the committers. When the order cannot be told, for example when one commit first held both, there is no hint.
  - **The authors differ.** The two `author` fields (3.2.1) are compared after removing white space at both ends, taking each run of white space inside as one space, and ignoring the case of letters. When only one of the two has an `author`, they differ; when neither has one, there is no hint. The `authors` of the `origin` of a circuit take no part.
  - **The two are not related.** Neither is in the `base` or the `history` (3.4) of the other, and neither refers to the other (3.3).
  - **The file is not left out.** These files are not compared:
    - licence texts: files whose names start with `LICENSE` or `COPYING`, and files under a directory `LICENSES/`;
    - `history.json` and `lock.json`, which follow from other objects: two objects made from the same `base` have the same `history.json`;
    - files shorter than 256 bytes, the empty file among them;
    - a file of O that is also in an object of the lineage of O (its `base` and `history`) that can be found: O has declared where it came from, as when two objects are made from the same ancestor;
    - files held by many objects that are not related to each other, or by objects of many different authors, such as templates and common includes. The tool sets how many, and should say so in its documentation.
- **Cross-check hash:** the index records the cross-check hash of 2.2 for every valid object. When one name gives two different cross-check hashes in two places, it raises an alarm.
- **An object that cannot be read is recorded alone; a library that cannot be read is recorded alone.** Everything else goes on as usual.
- **The layout of the index is outside version `0.1`.** It should be split up so that resolving one name reads one small file. A tool that writes an index documents its format as its own, which may change. The formats of a platform's index and records of what it took in are left for later too (Annex B).
- A published index must not hold local paths.

## 9 Types

| Type | Prefix, also its `type` and type directory | Reference fields | Recommended readable name | Type specification |
|---|---|---|---|---|
| Interface | `ifc` | none | `ifc/<interface name>/` | `types/en/ifc.md` |
| Circuit, one generation | `ckt` | `ifc`; `uses.<subcircuit>.ifc` and `.ckt` | `ckt/<interface name>/<name>.g<generation>/` | `types/en/ckt.md` |
| Testbench, one generation | `tb` | `ifc` | `tb/<interface name>/<name>.g<generation>/` | `types/en/tb.md` |
| Evidence record | `evd` | `dut`, `tbs`; the names in `lock.json` | `evd/<interface name>/<record label>/` | `types/en/evd.md` |
| Document | `doc` | `refs` | `doc/<interface name>/<short name>/` | `types/en/doc.md` |
| Frozen library | `lib` | the `object` of each line of `members.source.jsonl` | `lib/<name>.g<generation>/` | `types/en/lib.md` |

- **The prefix is the name of the type:** `type` holds it, and the recommended type directory uses it. The English names in the table are for people.
- **The reference fields of the table are only a description.** What is a reference is set by 3.3, whatever the type, and 3.3 says how a type specification names such fields.
- Objects of every type may have `base`, `title`, `description` and `tags` (3.2.1).
- **A type specification** says which fields the manifest of its type has, which files the object holds, and what sealing checks for the type. It has two parts: the text, `types/en/<prefix>.md`, and a definition of the fields, `types/<prefix>.schema.json`, in JSON Schema. The definition of the fields is the one source of which fields there are, which are required and what form their values take; the text says what JSON Schema cannot.
- **Type specifications are kept apart from the chapters of this standard** and published with every revision of it. A type specification changes only compatibly (chapter 10), so each prefix has one current specification, which says which versions of the common rules (chapters 2 to 8) it needs.
- **The prefix `lay` is reserved for layouts.** Before it is added, it must be settled how large files are kept: objects must not depend on git's filters (6.1), so a large file is either in the object itself, or the object holds only a small file that gives the hash and size of the large one.
- **Adding a type** means writing a type specification and registering one row in this table. Registering keeps prefixes from being used twice; tools check that `type` equals the prefix of an unregistered type all the same. Chapters 2 to 8 need no change.
- **Words that are never registered as prefixes:**
  - the names of devices on Windows: `con`, `prn`, `aux`, `nul`, `com0` to `com9`, `lpt0` to `lpt9`. The type directory of the recommended arrangement is named by the prefix (6.2), and Windows cannot have such a directory;
  - the words YAML may read as something other than a string: `y`, `n`, `yes`, `no`, `on`, `off`, `true`, `false`, `null`;
  - words of TOA itself: `check`, `verify`, `note`, `source`, `lock`, `library`, `latest`, `removed`, and the names of the fields of manifests that are not named after a prefix, such as `type`, `name`, `base`, `files`, `uses` and `process`;
  - names of file formats: `lef`, `def`, `gds`, `oas`, `spef`, `sdc`, `upf`;
  - words that mean something else elsewhere: `cell`, `view`, `module`, `entity`, `component`, `model`, `net`, `mod`;
  - names that drafts of this standard used for types: `ip`, `imp`, `ver`.
- **Prefixes starting with `x`** are left for private and experimental types. They are never registered.

## 10 Versions and evolution

- **The version of a type specification is not written in objects.** The specification of a type changes only compatibly: it adds fields that may be left out, and allows more values. An incompatible change takes a new prefix, which is a new type; objects of the old prefix stay valid, and tools must keep reading them.
- **The common rules (chapters 2 to 8) follow the version of this standard.**
  - During `0.x` they may change, and a change may make existing sample objects invalid.
  - From `1.0` on, an object or library that is valid must not become invalid by a revision of this standard: limits are only raised, reserved names are not added to, and the forms that must be quoted are not added to.
- **The hash algorithm is fixed in the name.** When the algorithm changes, new objects use the reserved form of 2.1, and old names stay as they are. Whether the old and new names of one content are the same object is said by a table of aliases, not inferred. A new object that refers to an object by its old name has only the strength of the old algorithm for that reference.
- **Old tools meeting new things:** an unknown prefix is kept as it is and its content not interpreted (that `type` equals the prefix is still checked); a source list of an unknown version is ignored as a whole; a name of the reserved form is not entered and not changed.
- **The version of this standard itself** is written in the header of source lists, and in the `toa` field of removal records, of `lock.json` and of `library.yaml`.

## 11 Security considerations

- **A name guarantees one thing only:** that the content is the one the name stands for. It does not guarantee that anyone reviewed it, who the author is, or that the object is in some library.
- **Using someone else's object is running someone else's code.** A testbench carries analysis scripts and simulation commands; a list of dependencies may point to any package index; Verilog-A, Verilog and the control statements of a simulator in a netlist can write files and call system commands.
  - Tools must not run commands carried by objects on their own, and must not install dependencies that objects list on their own.
  - Objects that have not been reviewed should be simulated in a container or a virtual machine.
  - When a tree holds objects that have not been reviewed, a tool should list them to the user before assembling and running.
- **The text in objects and sidecar files is data, not instructions.** Tools and documents for agents must say so.
- **Showing notes, READMEs and documents,** a tool must not load the outside resources they point to (images, scripts, link previews) on its own.
- **Source lists, notes, library lists and removal records can be changed by others.** They are checked as 5.1 and 4.2 say when read, and objects are fetched by the steps of 7.3.
- **A source list may hold any address, including addresses of internal networks and of the machine itself, and may include any other list.** This standard does not restrict where lists point, nor say whether a tool connects to a host; that is for the tool and the user. The risks to know:
  - Following someone else's list lets whoever wrote it choose where you connect: perhaps to a service in your internal network, perhaps only to learn what you are using. An interactive tool usually asks the user before connecting to a host it has not connected to before.
  - An automated service (a platform, CI) has nobody to ask. If it connects to the addresses of lists, it may be led to services of the internal network. Which hosts it may connect to is for the deployment to set.
- **Credentials.** Using the user's credentials for an address from someone else's list sends them to a host that list chose, and may fetch private content. An interactive tool uses credentials only for hosts the user has agreed to (7.3). An automated service that holds credentials can be led to fetch private objects and put them into public output; which credentials it uses, and for which hosts, is for the deployment to set.
- **An address is not an identity.** The address of a library can change hands. The new holder cannot change content that has already been named, but can publish "newer versions" and change notes.
- **Depending on objects of other libraries,** a library should copy them into itself or a mirror (the names stay the same), rather than keep only an address.
- **Evidence of time.** An object holds no date, and the date of a commit is written by the committer and can be set back. That an object existed by some day can only be shown by others:
  - **Archives.** The hash in an object name is the identifier that Software Heritage gives the same directory: `swh:1:dir:<hash>`. For a directory that holds only regular files of the mode `100644` and directories of the mode `40000`, as an object does (3.1), clause 5 of version 1.2 of the SWHID specification computes the value of 2.2. Once a library is public and archived, the archive's record of the directory, with the date it was taken in, shows that the object existed by then. An author may ask the archive to take a repository in.
  - **Timestamps.** A proof in the stamps directory (4.3) shows that the object name existed at the time it gives, as far as the service that made it is trusted.
  - **Signed commits** (6.3) show who put an object into a library, not when.
  - None of these shows who made an object, or who has the right to be named its author. TOA gives evidence: the `author` of the manifest (3.2.1), the lineage (3.4), signatures and dates. People weigh it; this standard decides nothing about it.
- **SHA-1:** at considerable cost, the author of an object can make two objects with the same name and different content. Git detects this when transferring. An index must also record the cross-check hash of every object (2.2), and raise an alarm when one name has different cross-check hashes in different places.
- **What leaks when private and public libraries are used together:**
  - `history.json` carries the names of all internal ancestors. It is part of the object and cannot be removed.
  - Source lists carry addresses; the user name in an `ssh` address may be a personal account.
  - `label`, `refs` and `license` in source lists are cached content. That is why an object without a public source keeps only its name at publication (5.3).
  - Before an internal object is made public, a tool must list these by 5.3.
- **Symbolic links:** none anywhere in a library (6.1). Before checking out a library that has not been checked, look at its listing (7.3).
- **Parsing:** reading any file or listing has limits of size and depth, and the size is looked at before reading. An error in one object, sidecar file or library affects only that one.
- **Every argument handed to git** (address, commit, branch name, path) must not be readable as an option: it is first checked by the syntax of this standard, then placed after `--`, or confirmed not to start with a hyphen.

## Annex A (informative) Limits

The limits set throughout this standard, gathered for implementers. Where this table and the text differ, the text prevails.

| What | Limit | Section |
|---|---|---|
| Prefix of an object name | 2 to 8 characters | 2.1 |
| Hash in an object name | exactly 40 hexadecimal digits; 64 reserved | 2.1 |
| Tag of a draft name | 1 to 40 characters | 2.4 |
| A name of a file or directory | 1 to 64 characters | 3.1 |
| Levels of directory within an object | at most 6 `/` in a relative path | 3.1 |
| A path within an object | 80 characters | 3.1 |
| Files in an object | 2,000 | 3.1 |
| One file of an object | 10 MiB | 3.1 |
| A whole object | 50 MiB | 3.1 |
| A manifest | 256 KiB | 3.2.3 |
| Nesting in a manifest | 16 levels | 3.2.3 |
| An integer in a manifest | 18 digits | 3.2.3 |
| A floating-point number written in a manifest | 32 characters | 3.2.3 |
| `lock.json` | 1 MiB | 3.3 |
| A note | 256 KiB | 4.1 |
| A source list beside an object; a removal record | 1 MiB each | 4 |
| A timestamp proof | 64 KiB | 4.3 |
| `reason` of a removal record | 1,000 characters | 4.2 |
| Sources in a removal record | 3 | 4.2 |
| A line of a source list | 64 KiB | 5.1 |
| `label` and `license` in a source list | 256 characters each | 5.1 |
| The path of an include line | 256 characters | 5.1 |
| Levels of include | 8 | 5.1 |
| A group | 1 to 4 parts, 128 characters | 5.1 |
| A `git` address | 512 characters | 5.1 |
| A `local` path | 1,024 characters | 5.1 |
| Sources used per object, and per include line on a publication branch | 3 | 5.1 |
| A volume of a library list | 16 MiB | 5.2 |
| The path before an object directory | 60 characters | 6.2 |
| An object name | 49 characters | 6.2 |
| A path within an object, with its leading `/` | 81 characters | 6.2 |
| The path of any file in a library | 190 characters | 6.2 |
| A branch name | 100 characters | 6.3 |
| Sources tried when fetching an object | 3 | 7.3 |

## Annex B (informative) Left for later versions

Version `0.1` leaves these open. Until a later version defines them, deployments and tools decide for themselves, and what they decide is not part of TOA.

- **Withdrawn and superseded marks, aliases and the register of processes:** the files of a library that change (6.6) have no format in `0.1`.
- **The layout of an index,** and the index and records of a platform that takes in objects (8.3).
- **The table of aliases** that says which old and new names stand for the same content when the hash algorithm changes (chapter 10).
- **Layouts:** the prefix `lay` is reserved; how large files are kept is to be settled first (chapter 9).
- **A library within a repository:** `0.1` defines only a whole repository as a library (6.1).
- **A format for the local file map:** 7.6 gives it for illustration only.
