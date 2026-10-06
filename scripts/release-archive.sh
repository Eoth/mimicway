#!/usr/bin/env bash
# Packs a release binary with the documents that ship with it, into the archive a release publishes:
#
#   scripts/release-archive.sh <binary> <target> <version> <output directory>
#
# Run from the root of a checkout, on Linux (GNU tar, gzip, zip). The archive depends on its inputs alone: entries
# sorted by name, every time set to the commit's (SOURCE_DATE_EPOCH when set), owner root, fixed permissions, no time
# in the gzip header. Two builds of the same tag thus give the same archive, which SECURITY.md relies on to compare a
# rebuild with a release. Windows targets get a zip, every other target a .tar.gz.
set -euo pipefail

if [ $# -ne 4 ]; then
  echo "usage: $0 <binary> <target> <version> <output directory>" >&2
  exit 2
fi
binary=$1
target=$2
version=$3
out=$4
docs=(LICENSE README.md CHANGELOG.md MIGRATING.md)

# A checkout with Windows line endings holds other bytes than the release's: refuse it rather than differ.
for doc in "${docs[@]}"; do
  if grep -q $'\r' "$doc"; then
    echo "$doc has Windows line endings: check the repository out with core.autocrlf=false." >&2
    exit 1
  fi
done

epoch=${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}
name="mimicway-$version-$target"
exe=mimicway
if [[ $target == *windows* ]]; then exe=mimicway.exe; fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/$name"
cp "$binary" "$work/$name/$exe"
cp "${docs[@]}" "$work/$name/"
chmod 0755 "$work/$name" "$work/$name/$exe"
for doc in "${docs[@]}"; do chmod 0644 "$work/$name/$doc"; done
touch -d "@$epoch" "$work/$name" "$work/$name"/*

mkdir -p "$out"
out=$(cd "$out" && pwd)
if [[ $target == *windows* ]]; then
  rm -f "$out/$name.zip"
  # -X leaves out the owner and the extra time fields; zip stores local time, made the same everywhere by TZ.
  (cd "$work" && find "$name" | LC_ALL=C sort | TZ=UTC zip -X -q -@ "$out/$name.zip")
else
  tar --sort=name --format=gnu --mtime="@$epoch" --owner=0 --group=0 --numeric-owner -C "$work" -cf - "$name" |
    gzip -9 -n > "$out/$name.tar.gz"
fi
