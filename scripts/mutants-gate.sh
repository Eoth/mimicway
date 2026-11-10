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
# directory). MUTANTS_SHARD=k/n judges only the k-th of n shares of the mutants (k from 0), one CI job each: the whole
# scope takes hours, and a GitHub job stops at six.
#
# A mutant that makes the tests run until the timeout counts as caught: an endless loop fails the suite too. One that
# makes them allocate without end is stopped by a memory limit, and counts as caught as well.
set -euo pipefail

# The files the gate covers, each with no surviving mutant left unexplained. A file of src/engine/ joins once a run
# over it (`cargo mutants -f <file>`) leaves none.
SCOPE=(src/engine/matcher.rs src/engine/template.rs src/engine/script.rs)

args=()
for file in "${SCOPE[@]}"; do
  # A file renamed or split would otherwise give no mutant, and a green gate that judged nothing.
  [ -f "$file" ] || { echo "$file, covered by the gate, does not exist: update SCOPE" >&2; exit 1; }
  args+=(--file "$file")
done
if [ $# -ge 1 ]; then args+=(--in-diff "$1"); fi
if [ -n "${MUTANTS_SHARD:-}" ]; then
  # Round-robin, mutant i in share i % n: the mutants of one function, which may all hang until the timeout, spread
  # over the shares instead of making one of them the longest.
  args+=(--shard "$MUTANTS_SHARD" --sharding round-robin)
fi
if [ -n "${MUTANTS_SHARD:-}" ] || [ $# -ge 1 ]; then
  # What a CI job judges: about 40 s a mutant, 400 of them take four and a half hours, and the job stops at six.
  # Beyond, judging would end in a stopped job, with no verdict.
  list=$(cargo mutants --list "${args[@]}") || { echo "cargo-mutants could not list the mutants to judge" >&2; exit 1; }
  count=$(printf '%s' "$list" | grep -c '' || true)
  if [ "$count" -gt 400 ]; then
    echo "$count mutants to judge, more than the 400 a CI job can: raise MUTANT_SHARDS in scripts/ci-plan.mjs (weekly" \
      "run), or push the change in smaller parts (diff)" >&2
    exit 1
  fi
  echo "$count mutants to judge."
fi
if [ "${MUTANTS_IN_PLACE:-}" = 1 ]; then args+=(--in-place); else args+=(--jobs "${MUTANTS_JOBS:-1}"); fi
out=${MUTANTS_OUT:-$(mktemp -d)}
args+=(--output "$out")

# Each run of the tests gets 2 GiB of data at most; the suite needs about 100 MB, and passes under 256 MiB. A mutant that
# makes a loop allocate without end (an index that stays in place while each turn pushes a byte, `i *= 1` in
# url_decode: 6 GB in 19 s) then aborts within seconds, caught, instead of filling the memory before its timeout: on a
# GitHub runner (16 GB), the whole job is stopped with it (exit code 143). Cargo runs each test binary through the
# runner of its target. Not limited: the compiler, and a mutant that writes without end to the disk. prlimit makes the
# gate Linux only.
command -v prlimit > /dev/null || { echo "prlimit (util-linux) is missing: it limits the memory of the tests" >&2; exit 1; }
host=$(rustc -vV | sed -n 's/^host: //p')
export "CARGO_TARGET_$(echo "$host" | tr 'a-z.-' 'A-Z__')_RUNNER=prlimit --data=2147483648 --"

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
