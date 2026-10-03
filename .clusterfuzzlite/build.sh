#!/bin/bash -eu
# Builds the fuzz targets of fuzz/ for ClusterFuzzLite (run inside the image of the Dockerfile next to this file), against
# the lock file the server ships with, so that the fuzzed crates are the shipped ones.
cd "$SRC/mimicway"
cp Cargo.lock fuzz/Cargo.lock
cargo fuzz build -O --debug-assertions
for target in $(cargo fuzz list); do
  cp "fuzz/target/x86_64-unknown-linux-gnu/release/$target" "$OUT/"
done
# The configurations shipped in examples/ start the corpus of the import target: the fuzzer mutates real ones.
zip -j -q "$OUT/config_import_seed_corpus.zip" examples/*.json
