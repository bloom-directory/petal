# Official Rocq 9.0.1 / OCaml 4.14.2 image, linux/amd64.
FROM rocq/rocq-prover@sha256:3ed9c46fa02e9fd7748a959abebb11b36f8818d10738dcc93edc67354c313ce2
COPY toolchain.opam.lock /tmp/petal-toolchain.opam.lock
RUN opam switch import /tmp/petal-toolchain.opam.lock --yes --jobs=6
RUN git clone https://github.com/WasmCert/WasmCert-Coq.git /home/rocq/petal-wasmcert \
 && cd /home/rocq/petal-wasmcert \
 && git checkout --detach 58a3d3f8b7af4dc8aa9351f254a9e672643b21f8 \
 && opam exec -- dune build theories/opsem.vo theories/binary_format_parser.vo theories/memory_list.vo -j 6
WORKDIR /fixture/out/wasmcert
ENTRYPOINT ["bash", "/fixture/wasmcert/check.sh"]
