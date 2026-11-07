#!/usr/bin/env bash
# Mutation gate of the rule engine: cargo-mutants changes the code of the files below one mutation at a time (a `&&`
# made `||`, a match arm deleted, a function that returns a default), and the library's tests must fail on each one.
# A mutant they pass is a change of behaviour no test sees: it fails the gate, until a test catches it or it is shown
# equivalent (it cannot change any result) and excluded in .cargo/mutants.toml, with its reason in the test module of
# its file.
#
# Usage:
#   scripts/mutants-gate.sh <diff>   the mutants in the lines the diff touches (pushes and pull requests)
#   scripts/mutants-gate.sh          every mutant of the covered files (the weekly run)
# Environment: MUTANTS_JOBS (default 1) builds and tests that many mutants at once, each in its own copy of the tree;
# MUTANTS_IN_PLACE=1 mutates the checkout itself instead, one at a time, reusing its target/ (disposable checkouts
# only: an interrupted run can leave a mutation in the file). MUTANTS_OUT keeps the report there (default: a temporary
# directory).
#
# A mutant that makes the tests run until the timeout counts as caught: an endless loop fails the suite too.
set -euo pipefail

# The files the gate covers, each with no surviving mutant left unexplained. A file of src/engine/ joins once a run
# over it (`cargo mutants -f <file>`) leaves none.
SCOPE=(src/engine/matcher.rs)

args=()
for file in "${SCOPE[@]}"; do
  # A file renamed or split would otherwise give no mutant, and a green gate that judged nothing.
  [ -f "$file" ] || { echo "$file, covered by the gate, does not exist: update SCOPE" >&2; exit 1; }
  args+=(--file "$file")
done
if [ $# -ge 1 ]; then args+=(--in-diff "$1"); fi
if [ "${MUTANTS_IN_PLACE:-}" = 1 ]; then args+=(--in-place); else args+=(--jobs "${MUTANTS_JOBS:-1}"); fi
out=${MUTANTS_OUT:-$(mktemp -d)}
args+=(--output "$out")

rc=0
cargo mutants "${args[@]}" || rc=$?
# 2: mutants missed, 3: mutants timed out (it wins over 2, so the missed list decides below). Anything else but 0 is a
# baseline that fails or a diff that does not apply.
case $rc in
  0 | 2 | 3) ;;
  *)
    echo "cargo-mutants failed before judging the mutants (exit code $rc)" >&2
    exit "$rc"
    ;;
esac
# A diff that touches none of the covered files ends before any report is written.
missed="$out/mutants.out/missed.txt"
if [ -s "$missed" ]; then
  echo "Surviving mutants: catch each one with a test, or exclude it in .cargo/mutants.toml if it is equivalent:" >&2
  sed 's/^/  /' "$missed" >&2
  exit 1
fi
# Every covered file has mutants the tests catch: none caught on a full run means nothing was judged.
if [ $# -eq 0 ] && [ ! -s "$out/mutants.out/caught.txt" ]; then
  echo "No mutant was caught in the covered files: the gate judged nothing" >&2
  exit 1
fi
echo "No surviving mutant."
