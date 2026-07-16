#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
wit-bindgen rust \
  --format \
  --generate-all \
  --pub-export-macro \
  --default-bindings-module petal::bindings \
  --world route-file \
  --out-dir "$root/crates/petal-sdk/src" \
  "$root/wit/route"

