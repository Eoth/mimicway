# The image ClusterFuzzLite builds the fuzz targets in (fuzz/): OSS-Fuzz's Rust builder, with a nightly toolchain,
# cargo-fuzz and the sanitizers. Pinned by digest like every image of the repository; Dependabot proposes the updates.
FROM gcr.io/oss-fuzz-base/base-builder-rust@sha256:5fd6a6afd7369a3277b9697b39cb44bf8ed3f389bb440e7294a3f3869891ef97
COPY . $SRC/mimicway
WORKDIR $SRC/mimicway
COPY .clusterfuzzlite/build.sh $SRC/
