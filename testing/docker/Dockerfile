ARG BASE
FROM ${BASE}

ARG ARCH

RUN rustup component add clippy rustfmt
RUN curl -L --fail -o /usr/bin/minio "https://dl.min.io/server/minio/release/linux-${ARCH}/minio" && chmod +x /usr/bin/minio
