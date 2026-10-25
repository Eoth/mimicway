#!/usr/bin/env bash
# Replays the tour of the README against a real Mimicway, as a reader pastes it, and fails as soon as the README stops
# telling the truth:
#
#   scripts/check-readme-tour.sh [binary]      (default: target/debug/mimicway; run from anywhere)
#
# The tour is every ```bash block between the `<!-- tour:start -->` and `<!-- tour:end -->` lines of README.md.
# README.fr.md must hold the same blocks, byte for byte: the French page translates the prose around the commands,
# never the commands. The commands run in order in one bash, so that variables, functions and the background job carry
# from one block to the next. What each command prints must match the `# ` lines written under it, `…` standing for
# any text (fake data, long values), and a command without such lines must print nothing.
#
# The tour is written for port 7343 and ./target/release/mimicway: it runs here on a free port, with the binary given,
# from a temporary directory deleted at the end, with nothing of the caller's environment but PATH. An instance already
# running, on 7342 or elsewhere, and its data are never touched. The tour may only call that one instance (no
# outbound connection), never listen beyond loopback, and must stop the instance itself.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
bin=${1:-$root/target/debug/mimicway}
if [ ! -x "$bin" ]; then
  echo "no Mimicway binary at $bin: run cargo build first, or pass the binary's path" >&2
  exit 2
fi
bin=$(cd "$(dirname "$bin")" && pwd)/$(basename "$bin")
started=$(date +%s%N)

# The fenced bash blocks between the markers, separated by an empty line; fails on a missing or repeated marker.
tour_of() {
  awk '
    $0 == "<!-- tour:start -->" { starts++; inside = 1; next }
    $0 == "<!-- tour:end -->" { ends++; inside = 0; next }
    inside && $0 == "```bash" { fenced = 1; blocks++; if (blocks > 1) print ""; next }
    inside && fenced && $0 == "```" { fenced = 0; next }
    fenced { print }
    END { if (starts != 1 || ends != 1 || blocks == 0) exit 1 }
  ' "$1"
}

if ! tour=$(tour_of "$root/README.md"); then
  echo "README.md: expected one <!-- tour:start --> line, one <!-- tour:end --> line and bash blocks between them" >&2
  exit 1
fi
if ! french=$(tour_of "$root/README.fr.md"); then
  echo "README.fr.md: expected one <!-- tour:start --> line, one <!-- tour:end --> line and bash blocks between them" >&2
  exit 1
fi
if [ "$tour" != "$french" ]; then
  echo "The commands of the tour differ between README.md and README.fr.md (- English, + French):" >&2
  diff -u <(printf '%s\n' "$tour") <(printf '%s\n' "$french") | tail -n +3 >&2
  exit 1
fi

# What the tour may do, checked on the text as written.
refuse() {
  echo "README.md, tour: $1" >&2
  exit 1
}
if grep -qE 'BIND_ADDRESS|0\.0\.0\.0|\[::\]' <<<"$tour"; then refuse "it must listen on 127.0.0.1 only"; fi
if grep -q '7342' <<<"$tour"; then refuse "it must leave port 7342 alone (an instance may run there)"; fi
if grep -oE '(https?://)?(localhost|127\.0\.0\.1|\[::1\])(:[0-9]+)?|https?://[^/ "]+' <<<"$tour" |
  grep -vqxE '(http://)?localhost:7343'; then
  refuse "every address must be localhost:7343, the instance it starts (no outbound connection)"
fi
if [ "$(grep -c '\./target/release/mimicway' <<<"$tour")" != 1 ]; then
  refuse "it must start ./target/release/mimicway once"
fi
if ! grep -q '^PORT=7343 ' <<<"$tour"; then refuse "it must start the instance with PORT=7343, the port it calls"; fi
if ! grep -qx 'kill %1' <<<"$tour"; then refuse "it must stop the instance it starts (kill %1)"; fi

port_in_use() {
  if command -v ss >/dev/null; then
    ss -Hltn "sport = :$1" | grep -q .
  else
    (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null
  fi
}
free_port() {
  local candidate
  for _ in $(seq 1 50); do
    candidate=$((20000 + RANDOM % 40000))
    if [ "$candidate" != "${port:-}" ] && ! port_in_use "$candidate"; then
      echo "$candidate"
      return 0
    fi
  done
  echo "no free port found" >&2
  return 1
}
port=$(free_port)
# Where every request that is not for the instance goes, curl's and the instance's proxy alike: a port nothing listens
# on, so that an address the check above misses fails the tour instead of leaving the machine.
dead=$(free_port)

work=$(mktemp -d)
cleanup() {
  local pid
  if [ -s "$work/out/pid" ]; then
    pid=$(<"$work/out/pid")
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      sleep 1
      kill -9 "$pid" 2>/dev/null || true
    fi
  fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$work/home" "$work/tmp" "$work/out"

tour=${tour//7343/$port}
release=./target/release/mimicway
quoted_bin=$(printf '%q' "$bin")
tour=${tour//"$release"/"$quoted_bin"}

# Cuts the tour into commands (as many lines as bash needs to read one whole) and the `# ` lines under each.
commands=()
expected=()
pending=
while IFS= read -r line; do
  if [ -z "$pending" ] && [[ $line == \#* ]]; then
    if [ ${#commands[@]} -eq 0 ]; then refuse "a '# ' line comes before any command"; fi
    last=$((${#commands[@]} - 1))
    expected[last]+="${line#\# }"$'\n'
    continue
  fi
  if [ -z "$pending" ] && [ -z "$line" ]; then continue; fi
  pending+="$line"$'\n'
  if bash -n <<<"$pending" 2>/dev/null; then
    commands+=("$pending")
    expected+=("")
    pending=
  fi
done <<<"$tour"
if [ -n "$pending" ]; then refuse "a command never ends: $(head -n 1 <<<"$pending")"; fi

# One script, so that the shell keeps its variables and its job from one command to the next; each command's output
# and exit status go to their own files. The instance's own output goes to the file of the command that starts it.
script=$work/tour.sh
{
  echo 'set -uo pipefail'
  for i in "${!commands[@]}"; do
    command=${commands[i]}
    if [[ $command == 'kill %1'* ]]; then
      echo "grep VmHWM /proc/\$(cat '$work/out/pid')/status > '$work/out/vmhwm' 2>/dev/null"
    fi
    printf '{\n%s} > %q 2>&1\necho $? > %q\n' "$command" "$work/out/$i" "$work/out/$i.status"
    if [[ $command == *"$quoted_bin"* ]]; then
      echo "echo \$! > '$work/out/pid'"
      start=$i
    fi
  done
} >"$script"
if [ -z "${start:-}" ]; then refuse "it must start the binary in the background"; fi

rc=0
proxy=http://127.0.0.1:$dead
(cd "$work/home" && env -i PATH="$PATH" HOME="$work/home" TMPDIR="$work/tmp" LANG=C.UTF-8 \
  http_proxy="$proxy" HTTP_PROXY="$proxy" https_proxy="$proxy" HTTPS_PROXY="$proxy" all_proxy="$proxy" \
  ALL_PROXY="$proxy" no_proxy=localhost,127.0.0.1 NO_PROXY=localhost,127.0.0.1 \
  timeout 60 bash "$script") || rc=$?

# Text matches pattern, where `…` in pattern stands for any text, possibly empty.
matches() {
  local text=$1 pattern=$2 parts part i n
  mapfile -t parts <<<"${pattern//…/$'\n'}"
  n=${#parts[@]}
  if [ "$n" -eq 1 ]; then [ "$text" = "$pattern" ] && return 0 || return 1; fi
  [[ $text == "${parts[0]}"* ]] || return 1
  text=${text:${#parts[0]}}
  for ((i = 1; i < n - 1; i++)); do
    part=${parts[i]}
    [[ $text == *"$part"* ]] || return 1
    text=${text#*"$part"}
  done
  [[ $text == *"${parts[n - 1]}" ]]
}
trimmed() { sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' "$@"; }

fail() {
  local i=$1
  echo "README tour, command $((i + 1)) of ${#commands[@]}: $2" >&2
  printf '%s' "${commands[i]}" | sed 's/^/  $ /' >&2
  if [ -n "${3:-}" ]; then printf '%s\n' "$3" >&2; fi
  if [ -s "$work/out/$start" ]; then
    echo "The instance said:" >&2
    tail -n 20 "$work/out/$start" | sed 's/^/  | /' >&2
  fi
  exit 1
}
for i in "${!commands[@]}"; do
  if [ ! -f "$work/out/$i.status" ]; then
    fail "$i" "never ran (the tour stopped or took more than 60 s, exit status $rc)"
  fi
  if [ "$(<"$work/out/$i.status")" != 0 ]; then
    fail "$i" "failed with exit status $(<"$work/out/$i.status")" "$(sed 's/^/  > /' "$work/out/$i")"
  fi
  if [ "$i" = "$start" ]; then continue; fi
  mapfile -t got < <(trimmed "$work/out/$i")
  mapfile -t want < <(printf '%s' "${expected[i]}" | trimmed)
  ok=1
  if [ ${#got[@]} -ne ${#want[@]} ]; then ok=; fi
  for j in "${!want[@]}"; do
    if [ -n "$ok" ] && ! matches "${got[j]-}" "${want[j]}"; then ok=; fi
  done
  if [ -z "$ok" ]; then
    fail "$i" "the answer differs from the README" \
      "$(printf '  expected: %s\n' "${want[@]:-(nothing)}")"$'\n'"$(printf '  got:      %s\n' "${got[@]:-(nothing)}")"
  fi
done
if [ "$rc" != 0 ]; then
  echo "README tour: the replay ended with exit status $rc" >&2
  exit 1
fi

# The tour stops the instance itself: it must be gone within a few seconds, leaving no process behind.
pid=$(<"$work/out/pid")
for _ in $(seq 1 50); do
  if ! kill -0 "$pid" 2>/dev/null; then break; fi
  sleep 0.1
done
if kill -0 "$pid" 2>/dev/null; then
  echo "README tour: the instance (pid $pid) still runs after 'kill %1'" >&2
  exit 1
fi

elapsed=$((($(date +%s%N) - started) / 1000000))
peak=$(awk '{ print $2 " " $3 }' "$work/out/vmhwm" 2>/dev/null || true)
echo "README tour: ${#commands[@]} commands, the same in English and French, every answer as written" \
  "(${elapsed} ms; peak memory of the instance: ${peak:-not measured})"
