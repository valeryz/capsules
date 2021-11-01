#!/usr/bin/env bash

set -eux

PROJECT_PATH=dfinity-lab/infra-group/capsules

docker build --tag "registry.gitlab.com/${PROJECT_PATH}/rust-minio:latest" --build-arg BASE=amd64/rust:latest --build-arg ARCH=amd64 .
docker push "registry.gitlab.com/${PROJECT_PATH}/rust-minio:latest"
docker build --tag "registry.gitlab.com/${PROJECT_PATH}/rust-minio:nightly" --build-arg BASE=rustlang/rust:nightly --build-arg ARCH=amd64 .
docker push "registry.gitlab.com/${PROJECT_PATH}/rust-minio:nightly"
