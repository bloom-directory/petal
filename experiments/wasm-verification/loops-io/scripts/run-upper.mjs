// Run the upper WASI Petal: stdin -> stdout. Usage: node scripts/run-upper.mjs < in
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
const wasi = new WASI({ version: "preview1", stdin: 0, stdout: 1, stderr: 2, returnOnExit: true });
const url = new URL("../wasi-petal/target/wasm32-wasip1/release/upper-wasi-petal.wasm", import.meta.url);
const module = await WebAssembly.compile(await readFile(url));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
process.exitCode = wasi.start(instance);
