//! Milestone 1 evidence helper for the split-even verification fixture.
//!
//! This tool is part of the trusted computing base of every result it feeds.
//! It does not verify anything itself:
//!
//! * `extract` reads a packaged Petal archive, finds one route component,
//!   splits out its top-level core modules, and decodes the function a named
//!   core export refers to. Identity is recorded as SHA-256 of each byte string.
//! * `harness` appends Sunbeam property rules to the extracted module. It adds
//!   imports and functions only; every original function body that contains no
//!   function index is copied byte for byte, and the tool refuses to write a
//!   harness in which the target body differs from the shipped one.
//! * `run` executes concrete regression inputs on the extracted module with
//!   the wasmi interpreter. That is test evidence, not a proof.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use wasm_encoder::reencode::{self, Reencode};
use wasmparser::{
    Encoding, ExternalKind, FunctionBody, Operator, Parser, Payload, TypeRef, ValType, Validator,
    WasmFeatures,
};

type Result<T> = std::result::Result<T, String>;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let outcome = match args.first().map(String::as_str) {
        Some("extract") if args.len() == 5 => {
            extract(Path::new(&args[1]), &args[2], &args[3], Path::new(&args[4]))
        }
        Some("harness") if args.len() == 4 => {
            harness(Path::new(&args[1]), &args[2], Path::new(&args[3]))
        }
        Some("run") if args.len() == 3 => run(Path::new(&args[1]), &args[2]),
        _ => Err("usage:\n  petal-wasm-evidence extract <archive.petal.tar.gz> <component-entry> <core-export> <out-dir>\n  petal-wasm-evidence harness <module.wasm> <core-export> <out-dir>\n  petal-wasm-evidence run <module.wasm> <core-export>".into()),
    };
    match outcome {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
}

fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))
}

fn validate(bytes: &[u8]) -> Result<()> {
    Validator::new_with_features(WasmFeatures::all())
        .validate_all(bytes)
        .map(|_| ())
        .map_err(|e| format!("validation: {e}"))
}

// ---------------------------------------------------------------- extract

fn extract(archive: &Path, entry: &str, export: &str, out: &Path) -> Result<Value> {
    let archive_bytes = read(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive_bytes.as_slice()));
    let mut entries = BTreeMap::new();
    let mut component = None;
    for item in tar.entries().map_err(|e| e.to_string())? {
        let mut item = item.map_err(|e| e.to_string())?;
        let path = item
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned();
        let mut bytes = Vec::new();
        item.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if path == entry {
            if component.is_some() {
                return Err(format!("archive holds {entry} twice"));
            }
            component = Some(bytes.clone());
        }
        entries.insert(path, sha256(&bytes));
    }
    let component = component.ok_or_else(|| format!("archive has no entry {entry}"))?;
    validate(&component)?;

    // Top-level core modules only. A nested component would make "the module
    // the route runs" depend on instantiation wiring this tool does not model.
    let mut modules = Vec::new();
    let mut depth = 0usize;
    for payload in Parser::new(0).parse_all(&component) {
        match payload.map_err(|e| e.to_string())? {
            Payload::Version { encoding, .. } if depth == 0 && encoding != Encoding::Component => {
                return Err("entry is not a component".into());
            }
            Payload::ModuleSection {
                unchecked_range, ..
            } => {
                if depth == 0 {
                    modules.push(component[unchecked_range].to_vec());
                }
                depth += 1;
            }
            Payload::ComponentSection { .. } => {
                return Err("nested components are not supported by this extractor".into());
            }
            Payload::End(_) if depth > 0 => depth -= 1,
            _ => {}
        }
    }

    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    write(&out.join("component.wasm"), &component)?;
    let mut module_reports = Vec::new();
    let mut selected = None;
    for (index, module) in modules.iter().enumerate() {
        let decoded = decode_module(module)?;
        let file = format!("core-module-{index}.wasm");
        write(&out.join(&file), module)?;
        if decoded.exports.contains_key(export) {
            if selected.is_some() {
                return Err(format!("more than one core module exports {export}"));
            }
            selected = Some(index);
        }
        module_reports.push(json!({
            "index": index,
            "file": file,
            "sha256": sha256(module),
            "bytes": module.len(),
            "exports": decoded.exports.keys().collect::<Vec<_>>(),
        }));
    }
    let selected = selected.ok_or_else(|| format!("no core module exports {export}"))?;
    let module = &modules[selected];
    let target = describe_target(module, export)?;
    let module_file = out.join("target-module.wasm");
    write(&module_file, module)?;

    let report = json!({
        "schema": "bloom.petal.m1.extraction.v1",
        "extractor": extractor_identity(),
        "archive": {
            "path": archive.file_name().map(|n| n.to_string_lossy().into_owned()),
            "sha256": sha256(&archive_bytes),
            "entries": entries,
        },
        "component": {
            "entry": entry,
            "sha256": sha256(&component),
            "bytes": component.len(),
            "top_level_core_modules": module_reports,
        },
        "target_module": {
            "index": selected,
            "sha256": sha256(module),
            "bytes": module.len(),
        },
        "target_function": target,
    });
    write(
        &out.join("extraction.json"),
        serde_json::to_string_pretty(&report).unwrap().as_bytes(),
    )?;
    Ok(report)
}

fn extractor_identity() -> Value {
    json!({
        "tool": "petal-wasm-evidence",
        "decoder": "wasmparser 0.249.0",
        "encoder": "wasm-encoder 0.249.0",
    })
}

struct Decoded<'a> {
    types: Vec<wasmparser::FuncType>,
    imported_funcs: u32,
    func_types: Vec<u32>,
    exports: BTreeMap<String, (ExternalKind, u32)>,
    bodies: Vec<FunctionBody<'a>>,
    memories: Vec<wasmparser::MemoryType>,
}

fn decode_module(module: &[u8]) -> Result<Decoded<'_>> {
    validate(module)?;
    let mut decoded = Decoded {
        types: Vec::new(),
        imported_funcs: 0,
        func_types: Vec::new(),
        exports: BTreeMap::new(),
        bodies: Vec::new(),
        memories: Vec::new(),
    };
    for payload in Parser::new(0).parse_all(module) {
        match payload.map_err(|e| e.to_string())? {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for sub in group.map_err(|e| e.to_string())?.into_types() {
                        let func = sub.unwrap_func().clone();
                        decoded.types.push(func);
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.map_err(|e| e.to_string())?;
                    match import.ty {
                        TypeRef::Func(ty) => {
                            decoded.imported_funcs += 1;
                            decoded.func_types.push(ty);
                        }
                        TypeRef::Memory(memory) => decoded.memories.push(memory),
                        _ => {}
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    decoded.func_types.push(ty.map_err(|e| e.to_string())?);
                }
            }
            Payload::MemorySection(reader) => {
                for memory in reader {
                    decoded.memories.push(memory.map_err(|e| e.to_string())?);
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.map_err(|e| e.to_string())?;
                    decoded
                        .exports
                        .insert(export.name.to_owned(), (export.kind, export.index));
                }
            }
            Payload::CodeSectionEntry(body) => decoded.bodies.push(body),
            _ => {}
        }
    }
    Ok(decoded)
}

fn target_index(decoded: &Decoded, export: &str) -> Result<u32> {
    match decoded.exports.get(export) {
        Some((ExternalKind::Func, index)) if *index >= decoded.imported_funcs => Ok(*index),
        Some((ExternalKind::Func, _)) => Err(format!("{export} is an imported function")),
        Some(_) => Err(format!("{export} is not a function export")),
        None => Err(format!("module does not export {export}")),
    }
}

fn val(ty: &ValType) -> String {
    ty.to_string()
}

fn describe_target(module: &[u8], export: &str) -> Result<Value> {
    let decoded = decode_module(module)?;
    let index = target_index(&decoded, export)?;
    let body = &decoded.bodies[(index - decoded.imported_funcs) as usize];
    let type_index = decoded.func_types[index as usize];
    let ty = &decoded.types[type_index as usize];
    let range = body.range();
    let body_bytes = &module[range.clone()];

    let mut locals = Vec::new();
    for local in body.get_locals_reader().map_err(|e| e.to_string())? {
        let (count, ty) = local.map_err(|e| e.to_string())?;
        locals.push(json!({"count": count, "type": val(&ty)}));
    }
    let mut operators = Vec::new();
    let mut opcode_counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut reader = body.get_operators_reader().map_err(|e| e.to_string())?;
    while !reader.eof() {
        let offset = reader.original_position();
        let op = reader.read().map_err(|e| e.to_string())?;
        let text = format!("{op:?}");
        let name = text.split([' ', '{']).next().unwrap_or("").to_owned();
        *opcode_counts.entry(name).or_default() += 1;
        operators.push(json!({"offset": offset - range.start, "op": text}));
    }

    // Direct callers, so a reviewer can see that the route reaches this index.
    let mut callers = Vec::new();
    for (position, other) in decoded.bodies.iter().enumerate() {
        let mut reader = other.get_operators_reader().map_err(|e| e.to_string())?;
        while !reader.eof() {
            if let Operator::Call { function_index } = reader.read().map_err(|e| e.to_string())?
                && function_index == index
            {
                callers.push(position as u32 + decoded.imported_funcs);
            }
        }
    }
    let references = function_references(body)?;
    let control: Vec<_> = opcode_counts
        .keys()
        .filter(|name| {
            matches!(
                name.as_str(),
                "Loop"
                    | "Br"
                    | "BrIf"
                    | "BrTable"
                    | "Call"
                    | "CallIndirect"
                    | "ReturnCall"
                    | "ReturnCallIndirect"
                    | "Unreachable"
                    | "MemoryGrow"
                    | "Throw"
            ) || name.contains("Div")
                || name.contains("Rem")
                || name.contains("Trunc")
        })
        .cloned()
        .collect();

    Ok(json!({
        "export": export,
        "function_index": index,
        "defined_function_index": index - decoded.imported_funcs,
        "imported_function_count": decoded.imported_funcs,
        "type_index": type_index,
        "params": ty.params().iter().map(val).collect::<Vec<_>>(),
        "results": ty.results().iter().map(val).collect::<Vec<_>>(),
        "locals": locals,
        "body_offset_in_module": range.start,
        "body_bytes": body_bytes.len(),
        "body_sha256": sha256(body_bytes),
        "body_hex": hex::encode(body_bytes),
        "operators": operators,
        "opcode_counts": opcode_counts,
        "possibly_trapping_or_nonstraight_opcodes": control,
        "references_function_indices": references,
        "direct_callers": callers,
        "memories": decoded.memories.iter().map(|m| json!({
            "initial_pages": m.initial, "maximum_pages": m.maximum, "memory64": m.memory64, "shared": m.shared,
        })).collect::<Vec<_>>(),
    }))
}

fn function_references(body: &FunctionBody) -> Result<bool> {
    let mut reader = body.get_operators_reader().map_err(|e| e.to_string())?;
    while !reader.eof() {
        match reader.read().map_err(|e| e.to_string())? {
            Operator::Call { .. } | Operator::ReturnCall { .. } | Operator::RefFunc { .. } => {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------- harness

/// Imports the harness adds, in order, from module `env`. These are the
/// Certora CVT intrinsics Sunbeam recognizes by module, name and type.
const IMPORTS: &[(&str, &[ValType], &[ValType])] = &[
    ("CVT_nondet_u64", &[], &[ValType::I64]),
    ("CVT_nondet_u32", &[], &[ValType::I32]),
    ("CVT_assume", &[ValType::I32], &[]),
    ("CVT_assert", &[ValType::I32], &[]),
    ("CVT_satisfy", &[ValType::I32], &[]),
];
const NONDET_U64: u32 = 0;
const NONDET_U32: u32 = 1;
const ASSUME: u32 = 2;
const ASSERT: u32 = 3;
const SATISFY: u32 = 4;

struct HarnessEncoder<'a> {
    original: &'a [u8],
    old_types: u32,
    old_imported_funcs: u32,
    target: u32,
    max_result_address: u32,
    rules: Vec<Rule>,
    /// Indices of original bodies copied verbatim.
    verbatim: Vec<u32>,
    next_body: u32,
}

impl HarnessEncoder<'_> {
    fn shifted(&self, func: u32) -> u32 {
        if func < self.old_imported_funcs {
            func
        } else {
            func + IMPORTS.len() as u32
        }
    }
    fn import(&self, which: u32) -> u32 {
        self.old_imported_funcs + which
    }
    fn import_type(&self, which: u32) -> u32 {
        self.old_types + which
    }
    fn rule_type(&self) -> u32 {
        self.old_types + IMPORTS.len() as u32
    }
}

impl Reencode for HarnessEncoder<'_> {
    type Error = String;

    fn function_index(&mut self, func: u32) -> std::result::Result<u32, reencode::Error<String>> {
        Ok(self.shifted(func))
    }

    fn parse_type_section(
        &mut self,
        types: &mut wasm_encoder::TypeSection,
        section: wasmparser::TypeSectionReader<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        reencode::utils::parse_type_section(self, types, section)?;
        if types.len() != self.old_types {
            return Err(reencode::Error::UserError("type count changed".into()));
        }
        for (_, params, results) in IMPORTS {
            let params: Vec<_> = params.iter().map(encoder_val).collect();
            let results: Vec<_> = results.iter().map(encoder_val).collect();
            types.ty().function(params, results);
        }
        types.ty().function([], []);
        Ok(())
    }

    fn parse_import_section(
        &mut self,
        imports: &mut wasm_encoder::ImportSection,
        section: wasmparser::ImportSectionReader<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        reencode::utils::parse_import_section(self, imports, section)?;
        for (index, (name, _, _)) in IMPORTS.iter().enumerate() {
            let ty = self.import_type(index as u32);
            imports.import("env", name, wasm_encoder::EntityType::Function(ty));
        }
        Ok(())
    }

    fn parse_function_section(
        &mut self,
        functions: &mut wasm_encoder::FunctionSection,
        section: wasmparser::FunctionSectionReader<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        reencode::utils::parse_function_section(self, functions, section)?;
        for _ in &self.rules {
            functions.function(self.rule_type());
        }
        Ok(())
    }

    fn parse_export_section(
        &mut self,
        exports: &mut wasm_encoder::ExportSection,
        section: wasmparser::ExportSectionReader<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        reencode::utils::parse_export_section(self, exports, section)?;
        let first_rule = self.shifted(self.old_imported_funcs + self.next_body_count());
        for (offset, rule) in self.rules.iter().enumerate() {
            exports.export(
                rule.name,
                wasm_encoder::ExportKind::Func,
                first_rule + offset as u32,
            );
        }
        Ok(())
    }

    fn parse_function_body(
        &mut self,
        code: &mut wasm_encoder::CodeSection,
        func: FunctionBody<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        let index = self.next_body;
        self.next_body += 1;
        let refs = function_references(&func).map_err(reencode::Error::UserError)?;
        if refs {
            reencode::utils::parse_function_body(self, code, func)
        } else {
            code.raw(&self.original[func.range()]);
            self.verbatim.push(index);
            Ok(())
        }
    }

    fn parse_code_section(
        &mut self,
        code: &mut wasm_encoder::CodeSection,
        section: wasmparser::CodeSectionReader<'_>,
    ) -> std::result::Result<(), reencode::Error<String>> {
        reencode::utils::parse_code_section(self, code, section)?;
        for rule in &self.rules {
            code.function(&rule.encode(self));
        }
        Ok(())
    }
}

impl HarnessEncoder<'_> {
    fn next_body_count(&self) -> u32 {
        // Number of defined functions in the original module.
        defined_count(self.original)
    }
}

fn defined_count(module: &[u8]) -> u32 {
    let mut count = 0;
    for payload in Parser::new(0).parse_all(module) {
        if let Ok(Payload::FunctionSection(reader)) = payload {
            count = reader.count();
        }
    }
    count
}

fn encoder_val(ty: &ValType) -> wasm_encoder::ValType {
    match ty {
        ValType::I32 => wasm_encoder::ValType::I32,
        ValType::I64 => wasm_encoder::ValType::I64,
        _ => unreachable!("harness uses integer types only"),
    }
}

/// What a rule checks after the call. `p` is the result address.
#[derive(Clone, Copy)]
enum Check {
    /// SPLIT-1: first <= total and second == total - first, i.e. first + second
    /// = total over mathematical integers.
    Conservation,
    /// SPLIT-2: first <= second.
    Ordered,
    /// SPLIT-3: first <= second and second - first <= 1 (mathematical).
    Gap,
    /// SPLIT-5: a byte outside [p, p + 16) is unchanged.
    Frame,
    /// SPLIT-4 non-vacuity: a returning path exists (satisfy).
    Reaches,
    /// SPLIT-R*: total is fixed and the shares are exact.
    Concrete { total: u64, first: u64, second: u64 },
    /// Semantic probe, not a requirement: with the result slot at
    /// 0xFFFF_FFF0, outside any memory smaller than 4 GiB, Wasm traps on the
    /// first store. `satisfy` asks whether the verifier admits a normal
    /// return there. SAT means its memory model has no bounds trap.
    ProbeOutOfBoundsReturns,
}

struct Rule {
    name: &'static str,
    requirement: &'static str,
    check: Check,
}

fn rules() -> Vec<Rule> {
    vec![
        Rule {
            name: "rule_split_1_conservation",
            requirement: "SPLIT-1",
            check: Check::Conservation,
        },
        Rule {
            name: "rule_split_2_ordered",
            requirement: "SPLIT-2",
            check: Check::Ordered,
        },
        Rule {
            name: "rule_split_3_gap",
            requirement: "SPLIT-3",
            check: Check::Gap,
        },
        Rule {
            name: "rule_split_4_reaches_return",
            requirement: "SPLIT-4 (non-vacuity only)",
            check: Check::Reaches,
        },
        Rule {
            name: "rule_split_5_frame",
            requirement: "SPLIT-5",
            check: Check::Frame,
        },
        Rule {
            name: "rule_split_r1_zero",
            requirement: "SPLIT-R1",
            check: Check::Concrete {
                total: 0,
                first: 0,
                second: 0,
            },
        },
        Rule {
            name: "rule_split_r2_odd",
            requirement: "SPLIT-R2",
            check: Check::Concrete {
                total: 7,
                first: 3,
                second: 4,
            },
        },
        Rule {
            name: "rule_split_r3_max",
            requirement: "SPLIT-R3",
            check: Check::Concrete {
                total: u64::MAX,
                first: u64::MAX / 2,
                second: u64::MAX / 2 + 1,
            },
        },
        Rule {
            name: "probe_oob_result_slot_returns",
            requirement: "semantic probe (SPLIT-4 gap)",
            check: Check::ProbeOutOfBoundsReturns,
        },
    ]
}

impl Rule {
    fn encode(&self, h: &HarnessEncoder) -> wasm_encoder::Function {
        use wasm_encoder::{Instruction as I, MemArg};
        // locals: 0 p:i32, 1 total:i64, 2 first:i64, 3 second:i64, 4 q:i32, 5 before:i32
        let mut f = wasm_encoder::Function::new([
            (1, wasm_encoder::ValType::I32),
            (3, wasm_encoder::ValType::I64),
            (2, wasm_encoder::ValType::I32),
        ]);
        let (p, total, first, second, q, before) = (0, 1, 2, 3, 4, 5);
        let m8 = |offset| MemArg {
            offset,
            align: 3,
            memory_index: 0,
        };
        let m1 = MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        };

        if let Check::ProbeOutOfBoundsReturns = self.check {
            f.instruction(&I::I32Const(h.max_result_address as i32));
            f.instruction(&I::LocalSet(p));
        } else {
            // p is any address whose 16-byte result slot fits below 2^32.
            f.instruction(&I::Call(h.import(NONDET_U32)));
            f.instruction(&I::LocalSet(p));
            f.instruction(&I::LocalGet(p));
            f.instruction(&I::I32Const(h.max_result_address as i32));
            f.instruction(&I::I32LeU);
            f.instruction(&I::Call(h.import(ASSUME)));
        }

        match self.check {
            Check::Concrete { total: t, .. } => {
                f.instruction(&I::I64Const(t as i64));
                f.instruction(&I::LocalSet(total));
            }
            _ => {
                f.instruction(&I::Call(h.import(NONDET_U64)));
                f.instruction(&I::LocalSet(total));
            }
        }

        if let Check::Frame = self.check {
            // q < p or q >= p + 16, computed without wrapping.
            f.instruction(&I::Call(h.import(NONDET_U32)));
            f.instruction(&I::LocalSet(q));
            f.instruction(&I::LocalGet(q));
            f.instruction(&I::LocalGet(p));
            f.instruction(&I::I32LtU);
            f.instruction(&I::LocalGet(q));
            f.instruction(&I::I64ExtendI32U);
            f.instruction(&I::LocalGet(p));
            f.instruction(&I::I64ExtendI32U);
            f.instruction(&I::I64Const(16));
            f.instruction(&I::I64Add);
            f.instruction(&I::I64GeU);
            f.instruction(&I::I32Or);
            f.instruction(&I::Call(h.import(ASSUME)));
            f.instruction(&I::LocalGet(q));
            f.instruction(&I::I32Load8U(m1));
            f.instruction(&I::LocalSet(before));
        }

        // The call under verification: the shipped function, at its shifted index.
        f.instruction(&I::LocalGet(p));
        f.instruction(&I::LocalGet(total));
        f.instruction(&I::Call(h.shifted(h.target)));

        f.instruction(&I::LocalGet(p));
        f.instruction(&I::I64Load(m8(0)));
        f.instruction(&I::LocalSet(first));
        f.instruction(&I::LocalGet(p));
        f.instruction(&I::I64Load(m8(8)));
        f.instruction(&I::LocalSet(second));

        match self.check {
            Check::Conservation => {
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::LocalGet(total));
                f.instruction(&I::I64LeU);
                f.instruction(&I::LocalGet(second));
                f.instruction(&I::LocalGet(total));
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::I64Sub);
                f.instruction(&I::I64Eq);
                f.instruction(&I::I32And);
                f.instruction(&I::Call(h.import(ASSERT)));
            }
            Check::Ordered => {
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::LocalGet(second));
                f.instruction(&I::I64LeU);
                f.instruction(&I::Call(h.import(ASSERT)));
            }
            Check::Gap => {
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::LocalGet(second));
                f.instruction(&I::I64LeU);
                f.instruction(&I::LocalGet(second));
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::I64Sub);
                f.instruction(&I::I64Const(1));
                f.instruction(&I::I64LeU);
                f.instruction(&I::I32And);
                f.instruction(&I::Call(h.import(ASSERT)));
            }
            Check::Frame => {
                f.instruction(&I::LocalGet(q));
                f.instruction(&I::I32Load8U(m1));
                f.instruction(&I::LocalGet(before));
                f.instruction(&I::I32Eq);
                f.instruction(&I::Call(h.import(ASSERT)));
            }
            Check::Reaches | Check::ProbeOutOfBoundsReturns => {
                f.instruction(&I::I32Const(1));
                f.instruction(&I::Call(h.import(SATISFY)));
            }
            Check::Concrete {
                first: a,
                second: b,
                ..
            } => {
                f.instruction(&I::LocalGet(first));
                f.instruction(&I::I64Const(a as i64));
                f.instruction(&I::I64Eq);
                f.instruction(&I::LocalGet(second));
                f.instruction(&I::I64Const(b as i64));
                f.instruction(&I::I64Eq);
                f.instruction(&I::I32And);
                f.instruction(&I::Call(h.import(ASSERT)));
            }
        }
        f.instruction(&I::End);
        f
    }
}

fn harness(module_path: &Path, export: &str, out: &Path) -> Result<Value> {
    let original = read(module_path)?;
    let decoded = decode_module(&original)?;
    let target = target_index(&decoded, export)?;
    let target_body = &decoded.bodies[(target - decoded.imported_funcs) as usize];
    if function_references(target_body)? {
        return Err("target body references function indices; it cannot be copied verbatim".into());
    }
    let original_body = original[target_body.range()].to_vec();

    let mut encoder = HarnessEncoder {
        original: &original,
        old_types: decoded.types.len() as u32,
        old_imported_funcs: decoded.imported_funcs,
        target,
        max_result_address: u32::MAX - 15,
        rules: rules(),
        verbatim: Vec::new(),
        next_body: 0,
    };
    let mut module = wasm_encoder::Module::new();
    encoder
        .parse_core_module(&mut module, Parser::new(0), &original)
        .map_err(|e| format!("reencode: {e:?}"))?;
    let bytes = module.finish();
    validate(&bytes)?;

    // Check, with the same decoder, that the harness calls the shipped body.
    let harnessed = decode_module(&bytes)?;
    let shifted = encoder.shifted(target);
    let harness_body =
        &bytes[harnessed.bodies[(shifted - harnessed.imported_funcs) as usize].range()];
    if harness_body != original_body.as_slice() {
        return Err("target body differs between shipped module and harness".into());
    }
    if harnessed.func_types[shifted as usize] != decoded.func_types[target as usize] {
        return Err("target type index changed".into());
    }
    let harness_export = match harnessed.exports.get(export) {
        Some((ExternalKind::Func, index)) => *index,
        _ => return Err("harness lost the target export".into()),
    };
    if harness_export != shifted {
        return Err("harness export does not point at the shifted target".into());
    }

    fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let file: PathBuf = out.join("sunbeam-harness.wasm");
    write(&file, &bytes)?;
    let report = json!({
        "schema": "bloom.petal.m1.sunbeam-harness.v1",
        "extractor": extractor_identity(),
        "shipped_module_sha256": sha256(&original),
        "harness_sha256": sha256(&bytes),
        "harness_bytes": bytes.len(),
        "target_export": export,
        "target_index_in_shipped_module": target,
        "target_index_in_harness": shifted,
        "first_rule_function_index": encoder.shifted(decoded.imported_funcs + decoded.bodies.len() as u32),
        "target_body_sha256": sha256(&original_body),
        "target_body_identical": true,
        "original_defined_functions": decoded.bodies.len(),
        "original_bodies_copied_verbatim": encoder.verbatim.len(),
        "original_bodies_reencoded_for_index_shift": decoded.bodies.len() - encoder.verbatim.len(),
        "added_imports": IMPORTS.iter().map(|(n, _, _)| format!("env.{n}")).collect::<Vec<_>>(),
        "result_address_assumption": format!("p <= {} (unsigned)", u32::MAX - 15),
        "rules": encoder.rules.iter().map(|r| json!({"name": r.name, "requirement": r.requirement})).collect::<Vec<_>>(),
    });
    write(
        &out.join("sunbeam-harness.json"),
        serde_json::to_string_pretty(&report).unwrap().as_bytes(),
    )?;
    Ok(report)
}

// ---------------------------------------------------------------- run

fn ordered_gap_le_one(first: u64, second: u64) -> bool {
    // Short circuit before subtraction: exactly 0 <= second - first <= 1.
    first <= second && second - first <= 1
}

#[test]
fn concrete_gap_reporting_rejects_reversed_and_extreme_shares() {
    assert!(!ordered_gap_le_one(1, 0));
    assert!(!ordered_gap_le_one(u64::MAX, 0));
    assert!(!ordered_gap_le_one(0, u64::MAX));
    assert!(ordered_gap_le_one(u64::MAX - 1, u64::MAX));
    assert!(ordered_gap_le_one(0, 0));
}

fn run(module_path: &Path, export: &str) -> Result<Value> {
    let bytes = read(module_path)?;
    let engine = wasmi::Engine::default();
    let module = wasmi::Module::new(&engine, &bytes[..]).map_err(|e| e.to_string())?;
    let mut store = wasmi::Store::new(&engine, ());
    let mut linker = wasmi::Linker::<()>::new(&engine);
    // Every host import traps: the target must not need the host.
    for import in module.imports() {
        if let wasmi::ExternType::Func(ty) = import.ty() {
            let name = format!("{}.{}", import.module(), import.name());
            linker
                .func_new(
                    import.module(),
                    import.name(),
                    ty.clone(),
                    move |_, _, _| Err(wasmi::Error::new(format!("host import {name} called"))),
                )
                .map_err(|e| e.to_string())?;
        }
    }
    let instance = linker
        .instantiate_and_start(&mut store, &module)
        .map_err(|e| e.to_string())?;
    let func = instance
        .get_typed_func::<(i32, i64), ()>(&store, export)
        .map_err(|e| e.to_string())?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("module exports no memory")?;

    let cases: &[(u64, u64, u64)] = &[
        (0, 0, 0),
        (1, 0, 1),
        (7, 3, 4),
        (10, 5, 5),
        (u64::MAX - 1, u64::MAX / 2, u64::MAX / 2),
        (u64::MAX, u64::MAX / 2, u64::MAX / 2 + 1),
    ];
    let size = memory.data_size(&store);
    // Three result addresses: low, unaligned, and the last in-bounds slot.
    let addresses = [16usize, 1_001, size - 16];
    let mut results = Vec::new();
    let mut all_pass = true;
    for &(total, want_first, want_second) in cases {
        for &p in &addresses {
            let before = memory.data(&store).to_vec();
            let outcome = func.call(&mut store, (p as i32, total as i64));
            let after = memory.data(&store).to_vec();
            let entry = match outcome {
                Err(error) => {
                    all_pass = false;
                    json!({"total": total.to_string(), "p": p, "trap": error.to_string(), "pass": false})
                }
                Ok(()) => {
                    let first = u64::from_le_bytes(after[p..p + 8].try_into().unwrap());
                    let second = u64::from_le_bytes(after[p + 8..p + 16].try_into().unwrap());
                    let outside_changed = before
                        .iter()
                        .zip(&after)
                        .enumerate()
                        .any(|(i, (a, b))| a != b && !(p..p + 16).contains(&i));
                    let sum_ok = u128::from(first) + u128::from(second) == u128::from(total);
                    let ordered = first <= second;
                    let gap = ordered_gap_le_one(first, second);
                    let exact = first == want_first && second == want_second;
                    let pass = sum_ok && ordered && gap && exact && !outside_changed;
                    all_pass &= pass;
                    json!({
                        "total": total.to_string(), "p": p,
                        "first": first.to_string(), "second": second.to_string(),
                        "conservation": sum_ok, "ordered": ordered, "gap_le_1": gap,
                        "expected_exact": exact, "wrote_outside_result_slot": outside_changed,
                        "pass": pass,
                    })
                }
            };
            results.push(entry);
        }
    }
    // An out-of-bounds result slot must trap in real Wasm semantics.
    let oob = func.call(&mut store, ((size - 8) as i32, 7));
    Ok(json!({
        "schema": "bloom.petal.m1.concrete-run.v1",
        "evidence_type": "test (concrete execution), not proof",
        "engine": "wasmi 2.0.1 (interpreter); every host import traps",
        "module_sha256": sha256(&bytes),
        "export": export,
        "memory_bytes": size,
        "cases": results,
        "out_of_bounds_result_slot_traps": oob.is_err(),
        "out_of_bounds_trap": oob.err().map(|e| e.to_string()),
        "all_pass": all_pass,
    }))
}
