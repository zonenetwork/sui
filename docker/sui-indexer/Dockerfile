# Build application
#
# Copy in all crates, Cargo.toml and Cargo.lock unmodified,
# and build the application.
FROM rust:1.85-bullseye AS builder
ARG PROFILE=release
ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION
WORKDIR "$WORKDIR/sui"

# sui-indexer needs postgres libpq5 and ca-certificates
# Bullseye security packages are retained in Debian's official archive.
RUN sed -i \
        -e 's|http://|https://|g' \
        -e 's|https://deb.debian.org/debian-security|https://archive.debian.org/debian-security|g' \
        /etc/apt/sources.list \
    && apt -o Acquire::Retries=5 update && apt -o Acquire::Retries=5 install -y libpq5 ca-certificates libpq-dev postgresql

RUN apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y cmake clang

COPY Cargo.toml Cargo.lock ./
COPY consensus consensus
COPY crates crates
COPY sui-execution sui-execution
COPY external-crates external-crates

RUN cargo build --profile ${PROFILE} --bin sui-indexer

# Production Image
FROM debian:bullseye-slim AS runtime
# Bootstrap HTTPS trust before the slim image installs its CA package.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
# Use jemalloc as memory allocator
# Bullseye security packages are retained in Debian's official archive.
RUN sed -i \
        -e 's|http://|https://|g' \
        -e 's|https://deb.debian.org/debian-security|https://archive.debian.org/debian-security|g' \
        /etc/apt/sources.list \
    && apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y libjemalloc-dev ca-certificates curl
ENV LD_PRELOAD /usr/lib/x86_64-linux-gnu/libjemalloc.so
WORKDIR "$WORKDIR/sui"
COPY --from=builder /sui/target/release/sui-indexer /usr/local/bin
RUN apt -o Acquire::Retries=5 update && apt -o Acquire::Retries=5 install -y libpq5 ca-certificates libpq-dev postgresql

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION
