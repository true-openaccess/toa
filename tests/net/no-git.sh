#!/usr/bin/env bash
# toa on a machine without git (docs/lines/tools.md 9.5): the built-in git stands in.
# Usage: no-git.sh PATH-TO-TOA LIBRARIES-DIR, where LIBRARIES-DIR holds the example libraries
# built with git beforehand (TOA_EXAMPLES_OUT). Run it after git has been made unavailable.
set -euo pipefail

TOA=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
LIBS=$2
fail() { echo "FAIL: $*" >&2; exit 1; }

if command -v git >/dev/null; then fail "git is still installed: $(command -v git)"; fi

# reading libraries: the check of each, and the index
for l in official lee alice official-chen; do
  (cd "$LIBS/$l" && "$TOA" check 2>"$LIBS/$l.err") || { cat "$LIBS/$l.err"; fail "toa check in $l"; }
  grep -q "using the built-in git" "$LIBS/$l.err" || fail "$l: the user was not told the built-in git is used"
done
(cd "$LIBS" && "$TOA" index --offline --out "$LIBS/index-no-git") || fail "toa index"
grep -q '"valid": 11' "$LIBS/index-no-git/index.json" || fail "the index does not have 11 valid objects"

# asking for the system git: exit code 3
set +e
(cd "$LIBS/official" && "$TOA" --git=system check 2>/dev/null)
code=$?
set -e
[ $code = 3 ] || fail "--git=system without git gave exit code $code, not 3"

# sealing: the files hashed, the second computation by git not made, and the user told
mkdir -p "$LIBS/official/doc/n/doc_draft-n"
printf 'type: doc\nlicense: CC0-1.0\n' > "$LIBS/official/doc/n/doc_draft-n/toa.yaml"
printf 'hello\n' > "$LIBS/official/doc/n/doc_draft-n/README.md"
(cd "$LIBS/official" && "$TOA" seal 2>"$LIBS/seal.err") || { cat "$LIBS/seal.err"; fail "toa seal"; }
grep -q "second computation by git was not made" "$LIBS/seal.err" || fail "sealing did not say the second computation was not made"

# committing needs git
set +e
(cd "$LIBS/official" && "$TOA" publish --to https://example.org/x.git --commit 2>"$LIBS/publish.err")
code=$?
set -e
[ $code = 3 ] || { cat "$LIBS/publish.err"; fail "publish --commit without git gave exit code $code, not 3"; }
grep -q "needs git installed" "$LIBS/publish.err" || fail "publish --commit did not ask for git"

# deriving a draft needs no git: from the directory, and by name as toa fetch finds it
D=$(cd "$LIBS/official/doc/n" && ls -d doc_* | grep -v '\.' | grep -v _draft-)
(cd "$LIBS/official" && "$TOA" derive "doc/n/$D" --into doc/n2) || fail "toa derive from a directory"
grep -qx "base: \[$D\]" "$LIBS/official/doc/n2/doc_draft-from-${D:4:8}/toa.yaml" || fail "the derived draft has no base"
(cd "$LIBS/official" && "$TOA" derive "$D" --tag by-name --into doc/n3) || fail "toa derive by name"
[ -f "$LIBS/official/doc/n3/doc_draft-by-name/README.md" ] || fail "toa derive by name wrote no draft"
echo "ok: without git"
