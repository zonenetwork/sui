# Build application
#
# Builds the `checkpoint_bucket_writer` binary from the sui-data-ingestion
# crate. This image runs as a sidecar next to the archive-writer fullnode,
# reading executed checkpoints from the node's data-ingestion-dir and
# publishing `{seq}.chk` + `epochs.json` to an S3-compatible bucket.
FROM rust:1.85-bullseye AS builder
ARG PROFILE=release
ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION
WORKDIR "$WORKDIR/sui"
RUN apt-get update && apt-get install -y cmake clang

COPY Cargo.toml Cargo.lock ./
COPY consensus consensus
COPY crates crates
COPY sui-execution sui-execution
COPY external-crates external-crates

RUN cargo build --profile ${PROFILE} --bin checkpoint_bucket_writer

# Production Image
FROM debian:bullseye-slim AS runtime
RUN apt-get update \
    && apt-get install -y ca-certificates \
    && rm -rf /var/lib/apt/lists/*
ARG PROFILE=release
WORKDIR "$WORKDIR/sui"

# Path matches archiver-compose.yaml's indexer service command.
COPY --from=builder /sui/target/release/checkpoint_bucket_writer /opt/sui/bin/checkpoint_bucket_writer
# Legacy /usr/local/bin convenience.
COPY --from=builder /sui/target/release/checkpoint_bucket_writer /usr/local/bin/checkpoint_bucket_writer

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION
