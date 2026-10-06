# Base images are pinned by digest (the tag is kept for readability); Dependabot proposes the updates.
FROM node:24-alpine@sha256:ebfe2f90462722a7a4de65e91990e97fe0d401c70e0e762c5b53302f905ec1c1 AS frontend
WORKDIR /build/frontend
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci --ignore-scripts
COPY frontend/ ./
RUN npm run build

FROM rust:1.98-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS backend
RUN apk add --no-cache musl-dev
WORKDIR /build
# The dependencies first, built against a placeholder main in a layer of their own: every build whose Cargo.toml and
# Cargo.lock did not change reuses it. The placeholder's outputs are deleted, or Cargo could take the real sources, older
# than them, for already built.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release --locked \
    && rm -rf src target/release/mimicway target/release/deps/mimicway-* target/release/.fingerprint/mimicway-*
COPY build.rs ./
COPY src/ src/
# build.rs embeds the built UI in the binary.
COPY --from=frontend /build/frontend/dist frontend/dist
RUN cargo build --release --locked && mkdir /build/data

# The binary alone, for `docker build --target binary --output type=local,dest=<directory> .`: the release workflow
# builds its Linux binaries this way. Rust and Node.js come from the images above, pinned by digest, and every path is
# the same on every machine, so a rebuild of the same commit gives the same bytes (SECURITY.md says how to compare).
FROM scratch AS binary
COPY --from=backend /build/target/release/mimicway /mimicway

# Nothing but the binary and its data directory: the binary is linked statically (musl) and carries its own TLS roots
# (rustls with webpki-roots), so the image needs no operating system, no shell and no system package, which leaves a
# vulnerability scan nothing to flag that Mimicway does not use.
FROM scratch
COPY --from=backend --chown=1000:1000 /build/data /data
COPY --from=backend /build/target/release/mimicway /app/mimicway
WORKDIR /app
USER 1000:1000
ENV DATA_PATH=/data PORT=7342 BIND_ADDRESS=0.0.0.0
EXPOSE 7342
ENTRYPOINT ["/app/mimicway"]
