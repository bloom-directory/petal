#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
docker build --platform linux/amd64 -t petal-wasmcert:58a3d3f8 "$here"
