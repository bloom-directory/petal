#!/bin/bash
set -euxo pipefail
export OPAMROOT=$HOME/.petal-rocq/opam OPAMYES=1
opam init --bare --disable-sandboxing -n default https://opam.ocaml.org
opam switch create petal ocaml-variants.4.14.2+options ocaml-option-flambda --jobs=12 || true
eval $(opam env --switch=petal --set-switch)
opam repo add rocq-released https://rocq-prover.org/opam/released || true
opam install --jobs=12 dune.3.21.1 rocq-core.9.0.1 rocq-stdlib.9.0.0 coq.9.0.1 coq-compcert.3.17 coq-flocq.4.2.2 \
  rocq-mathcomp-ssreflect.2.4.0 coq-mathcomp-ssreflect.2.4.0 rocq-parseque.0.3.1 coq-ext-lib.0.13.1 wasm.2.0.2 \
  menhir.20260209 coq-menhirlib.20260209 rocq-elpi.3.5.0 rocq-hierarchy-builder.1.10.3 rocq-bignums.9.0.0+rocq9.0 \
  ppx_deriving.6.0.3 ppx_optcomp.v0.16.0 atdgen.4.2.0 cmdliner.2.1.1 num.1.5-1
[ -d $HOME/.petal-rocq/WasmCert-Coq ] || git clone https://github.com/WasmCert/WasmCert-Coq.git $HOME/.petal-rocq/WasmCert-Coq
cd $HOME/.petal-rocq/WasmCert-Coq
git checkout --detach 58a3d3f8b7af4dc8aa9351f254a9e672643b21f8
time opam exec -- dune build theories/opsem.vo theories/binary_format_parser.vo theories/memory_list.vo -j 12
echo SETUP_DONE
