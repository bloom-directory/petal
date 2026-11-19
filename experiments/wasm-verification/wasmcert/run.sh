#!/bin/sh
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
fixture=$(CDPATH= cd -- "$here/.." && pwd)
python3 "$here/generate_artifact.py"
docker run --rm --platform linux/amd64 \
  --mount "type=bind,source=$fixture,target=/fixture" \
  petal-wasmcert:58a3d3f8
