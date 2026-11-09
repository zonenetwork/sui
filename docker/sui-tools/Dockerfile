# Build application
#
# Copy in all crates, Cargo.toml and Cargo.lock unmodified,
# and build the application.
FROM rust:1.85-bullseye AS builder
ARG PROFILE=release
ARG GIT_REVISION
ENV GIT_REVISION=$GIT_REVISION
WORKDIR "$WORKDIR/sui"
# Bullseye security packages are retained in Debian's official archive.
RUN sed -i \
        -e 's|http://|https://|g' \
        -e 's|https://deb.debian.org/debian-security|https://archive.debian.org/debian-security|g' \
        /etc/apt/sources.list \
    && apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y cmake clang libpq5 libpq-dev

COPY Cargo.toml Cargo.lock ./
COPY consensus consensus
COPY crates crates
COPY sui-execution sui-execution
COPY external-crates external-crates

RUN cargo build --profile ${PROFILE} \
    --bin stress \
    --bin sui-faucet \
    --bin sui-bridge \
    --bin bridge-indexer \
    --bin deepbook-indexer \
    --bin sui-bridge-cli \
    --bin sui \
    --bin sui-cluster-test \
    --bin sui-tool

# Production Image
FROM debian:bullseye-slim AS runtime
# Bootstrap HTTPS trust before the slim image installs its CA package.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
WORKDIR "$WORKDIR/sui"

# sui-tool needs libpq at runtime and uses git when fetching move dependencies during build
# Bullseye security packages are retained in Debian's official archive.
RUN sed -i \
        -e 's|http://|https://|g' \
        -e 's|https://deb.debian.org/debian-security|https://archive.debian.org/debian-security|g' \
        /etc/apt/sources.list \
    && apt-get -o Acquire::Retries=5 update && apt-get -o Acquire::Retries=5 install -y libpq5 libpq-dev ca-certificates git

COPY --from=builder /sui/target/release/stress /usr/local/bin
COPY --from=builder /sui/target/release/sui-faucet /usr/local/bin
COPY --from=builder /sui/target/release/sui-bridge /usr/local/bin
COPY --from=builder /sui/target/release/bridge-indexer /usr/local/bin
COPY --from=builder /sui/target/release/deepbook-indexer /usr/local/bin
COPY --from=builder /sui/target/release/sui-bridge-cli /usr/local/bin
COPY --from=builder /sui/target/release/sui /usr/local/bin
COPY --from=builder /sui/target/release/sui-cluster-test /usr/local/bin
COPY --from=builder /sui/target/release/sui-tool /usr/local/bin

ARG BUILD_DATE
ARG GIT_REVISION
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_REVISION
