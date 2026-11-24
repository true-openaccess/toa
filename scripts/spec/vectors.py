#!/usr/bin/env python3
"""Write and check the test vectors of the TOA standard in spec/vectors/.

    python scripts/spec/vectors.py            # check the vectors as they are committed
    python scripts/spec/vectors.py --write    # write them again from the definitions below
    python scripts/spec/vectors.py --git      # also cross-check every hash with git

The definitions in this file are the source of the vectors; spec/vectors/README.md
describes their format. Python 3.10 or later, standard library only; --git needs git.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VECTORS = ROOT / "spec" / "vectors"

# ---------------------------------------------------------------- the algorithm (2.2)


def _tree(node: dict, algo: str) -> bytes:
    entries = []
    for name, value in node.items():
        if isinstance(value, dict):
            entries.append((name.encode() + b"/", b"40000 " + name.encode() + b"\0" + _tree(value, algo)))
        else:
            entries.append((name.encode(), b"100644 " + name.encode() + b"\0" + _blob(value, algo)))
    body = b"".join(entry for _, entry in sorted(entries))
    return hashlib.new(algo, b"tree " + str(len(body)).encode() + b"\0" + body).digest()


def _blob(data: bytes, algo: str) -> bytes:
    return hashlib.new(algo, b"blob " + str(len(data)).encode() + b"\0" + data).digest()


def _nest(files: dict[str, bytes]) -> dict:
    root: dict = {}
    for path, data in files.items():
        *dirs, leaf = path.split("/")
        node = root
        for part in dirs:
            node = node.setdefault(part, {})
        node[leaf] = data
    return root


def hash_files(files: dict[str, bytes], algo: str = "sha1") -> str:
    return _tree(_nest(files), algo).hex()


def tree_body(entries: list[tuple[str, str, bytes]]) -> bytes:
    """The body of a git tree written exactly as given: (mode, name, id) in this order."""
    return b"".join(mode.encode() + b" " + name.encode() + b"\0" + oid for mode, name, oid in entries)


def git_tree_id(body: bytes) -> str:
    return hashlib.sha1(b"tree " + str(len(body)).encode() + b"\0" + body).hexdigest()


# ---------------------------------------------------------------- the definitions

HASH = [
    {
        "id": "object",
        "files": {"toa.yaml": b"type: doc\nlicense: MIT\n", "README.md": b"hello\n"},
        "note": "A whole object: a manifest and one more file.",
    },
    {
        "id": "order",
        "files": {"a.txt": b"1\n", "a/z": b"2\n", "a-b": b"3\n", "a0": b"4\n"},
        "note": "Order of entries: a-b, a.txt, the directory a, a0. A directory sorts as if its name ended in '/'.",
    },
    {
        "id": "bytes",
        "files": {"empty": b"", "crlf.txt": b"x\r\n"},
        "note": "Bytes as they are: an empty file, and a CR LF line end that is not converted.",
    },
]

HASH_SECTION = "2.2, 2.3"


def noncanonical() -> list[dict]:
    """Git trees that list the files of the vector 'object' in a form other than the canonical one (2.2, 7.4)."""
    files = next(v for v in HASH if v["id"] == "object")["files"]
    readme, entry = _blob(files["README.md"], "sha1"), _blob(files["toa.yaml"], "sha1")
    canonical = hash_files(files)
    cases = [
        ("canonical", [("100644", "README.md", readme), ("100644", "toa.yaml", entry)], True,
         "The canonical tree: its id is the hash of the files."),
        ("mode-leading-zero", [("100644", "README.md", readme), ("0100644", "toa.yaml", entry)], False,
         "The mode of toa.yaml written 0100644."),
        ("order-swapped", [("100644", "toa.yaml", entry), ("100644", "README.md", readme)], False,
         "The two entries in the wrong order."),
    ]
    out = []
    for case_id, entries, valid, note in cases:
        body = tree_body(entries)
        out.append({
            "id": case_id,
            "files_of": "object",
            "tree_body_hex": body.hex(),
            "git_tree_id": git_tree_id(body),
            "hash": canonical,
            "valid": valid,
            "section": "2.2, 7.4",
            "note": note + ("" if valid else " Git gives the tree another id; a directory named after that id is not a valid object."),
        })
    return out


H40 = hash_files(HASH[0]["files"])            # a 40-digit lowercase hexadecimal value
H64 = hash_files(HASH[0]["files"], "sha256")  # a 64-digit one

# (value, note); every value is checked against both forms, so each entry says what it is.
NAMES = [
    # valid object names (2.1)
    (f"tb_{H40}", "The shortest registered prefix."),
    (f"ifc_{H40}", None),
    (f"ckt_{H40}", None),
    (f"evd_{H40}", None),
    (f"doc_{H40}", None),
    (f"lib_{H40}", None),
    (f"lay_{H40}", "A prefix no type is registered for is still an object name."),
    (f"x9_{H40}", "Prefix of 2 characters: a letter, then a digit."),
    (f"abcdefgh_{H40}", "Prefix of 8 characters, the longest."),
    (f"a1234567_{H40}", "Prefix of 8 characters with digits."),
    # not object names (2.1)
    (f"a_{H40}", "Prefix of 1 character."),
    (f"abcdefghi_{H40}", "Prefix of 9 characters."),
    (f"1p_{H40}", "Prefix starting with a digit."),
    (f"IFC_{H40}", "Upper case in the prefix."),
    (f"i-p_{H40}", "Hyphen in the prefix."),
    (f"ifc_{H40.upper()}", "Upper-case hexadecimal."),
    (f"ifc_{H40[:39]}", "39 hexadecimal digits."),
    (f"ifc_{H40}0", "41 hexadecimal digits."),
    (f"ifc_{H40[:39]}g", "A character that is not hexadecimal."),
    (f"my_ifc_{H40}", "Two underscores."),
    (f"ifc__{H40}", "Two underscores in a row."),
    (f"ifc-{H40}", "No underscore."),
    (f"ifc_{H40}\n", "A line end after the name: the whole string must match."),
    (f" ifc_{H40}", "A space before the name."),
    ("tb_ac", "An ordinary string; other lengths than 40 and 64 have no special meaning."),
    ("", "The empty string."),
    # the reserved form (2.1)
    (f"ifc_{H64}", "Reserved for a later hash algorithm: not an object name in 0.1, and must not be renamed or removed by tools."),
    (f"lay_{H64}", "Reserved form with another prefix."),
    (f"ifc_{H64[:63]}", "63 digits: neither an object name nor the reserved form."),
    # valid draft names (2.4)
    ("ckt_draft-lee-sky130", None),
    ("ifc_draft-1", "A tag of one digit."),
    ("evd_draft-a", "A tag of one letter."),
    ("lay_draft-x", "Any prefix."),
    ("ckt_draft-" + "a" * 40, "A tag of 40 characters, the longest."),
    ("ckt_draft-x-", "A tag may end with a hyphen; it only has to start with a letter or digit."),
    ("ckt_draft-a--b", "Hyphens may follow each other."),
    # not draft names (2.4)
    ("ckt_draft-", "An empty tag."),
    ("ckt_draft--x", "A tag starting with a hyphen."),
    ("ckt_draft-" + "a" * 41, "A tag of 41 characters."),
    ("ckt_draft-Lee", "Upper case in the tag."),
    ("ckt_draft-a.b", "A dot in the tag; names of sidecar files start with a draft name and a dot."),
    ("ckt_draft-a_b", "An underscore in the tag."),
    ("ckt_draft-a b", "A space in the tag."),
    ("ckt_draft_x", "No hyphen after 'draft'."),
    ("ckt-draft-x", "No underscore."),
    ("i_draft-x", "Prefix of 1 character."),
    ("IMP_draft-x", "Upper case in the prefix."),
    ("ckt_DRAFT-x", "'draft' in upper case."),
    ("ckt_draft-x\n", "A line end after the name."),
    ("ckt_draft-x.notes", "The notes directory of a draft: a name beside a draft, not a draft name."),
    ("ckt_draft-x.source.jsonl", "The source list of a draft."),
]

PREFIX = r"[a-z][a-z0-9]{1,7}"
OBJECT_RE = re.compile(PREFIX + r"_[0-9a-f]{40}")
RESERVED_RE = re.compile(PREFIX + r"_[0-9a-f]{64}")
DRAFT_RE = re.compile(PREFIX + r"_draft-[a-z0-9][a-z0-9-]{0,39}")


def classify(value: str) -> dict:
    # \Z, not $: a trailing line end must not match (2.1).
    whole = lambda rx: re.fullmatch(rx.pattern + r"\Z", value) is not None  # noqa: E731
    return {"object_name": whole(OBJECT_RE), "reserved": whole(RESERVED_RE), "draft_name": whole(DRAFT_RE)}


def name_vectors() -> list[dict]:
    out = []
    for value, note in NAMES:
        row = {"value": value, **classify(value)}
        row["section"] = "2.4" if "draft" in value.lower() else "2.1"
        if note:
            row["note"] = note
        out.append(row)
    return out


# ---------------------------------------------------------------- writing and checking


def dump(data) -> bytes:
    return (json.dumps(data, indent=2, ensure_ascii=False) + "\n").encode()


def expected() -> dict[str, bytes]:
    hash_rows = [{"id": v["id"], "dir": f"hash/{v['id']}",
                  "files": {p: d.decode() for p, d in sorted(v["files"].items())},
                  "hash": hash_files(v["files"]), "sha256": hash_files(v["files"], "sha256"),
                  "valid_object": "toa.yaml" in v["files"], "section": HASH_SECTION, "note": v["note"]} for v in HASH]
    return {
        "hash.json": dump({"about": "Hashes of sets of files (2.2, 2.3). See README.md.", "vectors": hash_rows,
                           "noncanonical_trees": noncanonical()}),
        "names.json": dump({"about": "Object names, the reserved form and draft names (2.1, 2.4). See README.md.",
                            "vectors": name_vectors()}),
    }


def read_dir(directory: Path) -> dict[str, bytes]:
    files = {}
    for path in sorted(directory.rglob("*")):
        if path.is_file():
            files[path.relative_to(directory).as_posix()] = path.read_bytes()
    return files


def write():
    for v in HASH:
        target = VECTORS / "hash" / v["id"]
        if target.exists():
            shutil.rmtree(target)
        for rel, data in v["files"].items():
            path = target / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for name, data in expected().items():
        (VECTORS / name).write_bytes(data)
    (VECTORS / ".gitattributes").write_bytes(b"* -text\n")


def check(with_git: bool) -> list[str]:
    problems = []
    if (VECTORS / ".gitattributes").read_bytes() != b"* -text\n":
        problems.append(".gitattributes must be exactly '* -text'")
    for name, data in expected().items():
        if (VECTORS / name).read_bytes() != data:
            problems.append(f"{name} is not what the definitions give; run with --write")
    table = json.loads((VECTORS / "hash.json").read_bytes())
    for row in table["vectors"]:
        on_disk = read_dir(VECTORS / row["dir"])
        if on_disk != {p: s.encode() for p, s in row["files"].items()}:
            problems.append(f"{row['dir']}: the files on disk differ from hash.json")
        elif hash_files(on_disk) != row["hash"] or hash_files(on_disk, "sha256") != row["sha256"]:
            problems.append(f"{row['dir']}: hashes differ")
    if with_git:
        problems += cross_check_with_git(table)
    return problems


def git(cwd: Path, *args, data: bytes | None = None) -> str:
    env = {**os.environ, "GIT_CONFIG_NOSYSTEM": "1", "HOME": str(cwd), "XDG_CONFIG_HOME": str(cwd)}
    return subprocess.run(["git", "-c", "core.autocrlf=false", *args], cwd=cwd, input=data, env=env,
                          capture_output=True, check=True).stdout.decode().strip()


def cross_check_with_git(table: dict) -> list[str]:
    problems = []
    with tempfile.TemporaryDirectory() as tmp:
        for fmt, key in (("sha1", "hash"), ("sha256", "sha256")):
            repo = Path(tmp) / fmt
            repo.mkdir()
            git(repo, "init", "-q", f"--object-format={fmt}")
            for row in table["vectors"]:
                for rel, text in row["files"].items():
                    path = repo / row["dir"] / rel
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(text.encode())
            git(repo, "add", "-A")
            root = git(repo, "write-tree")
            for row in table["vectors"]:
                got = git(repo, "rev-parse", f"{root}:{row['dir']}")
                if got != row[key]:
                    problems.append(f"{row['dir']}: git ({fmt}) gives {got}, hash.json says {row[key]}")
        repo = Path(tmp) / "sha1"
        for row in table["noncanonical_trees"]:
            got = git(repo, "hash-object", "-t", "tree", "--literally", "--stdin", data=bytes.fromhex(row["tree_body_hex"]))
            if got != row["git_tree_id"]:
                problems.append(f"noncanonical {row['id']}: git gives {got}, hash.json says {row['git_tree_id']}")
    return problems


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--write", action="store_true", help="write the vectors from the definitions")
    ap.add_argument("--git", action="store_true", help="also cross-check the hashes with git")
    args = ap.parse_args()
    if args.write:
        write()
    problems = check(args.git)
    for p in problems:
        print("error:", p, file=sys.stderr)
    if not problems:
        print("vectors: ok" + (" (cross-checked with git)" if args.git else ""))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
