# The image ClusterFuzzLite builds the fuzz targets in (fuzz/): OSS-Fuzz's Rust builder, with a nightly toolchain,
# cargo-fuzz and the sanitizers. Pinned by digest like every image of the repository; Dependabot proposes the updates.
FROM gcr.io/oss-fuzz-base/base-builder-rust@sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f
COPY . $SRC/mimicway
WORKDIR $SRC/mimicway
COPY .clusterfuzzlite/build.sh $SRC/
