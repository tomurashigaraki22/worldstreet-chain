//! Deterministic Intertrain `.it` program packages and a fuel-metered WASM MVP.
//! The runtime exposes only the `env.host_log` host function and rejects all
//! other imports. This is intentionally narrower than general WASI/WASM.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;
use wasmi::{Caller, Config, Engine, Linker, Module, Store, TypedFunc};
use wasmparser::{Parser, Payload, Validator, WasmFeatures};

pub const IT_MAGIC: &[u8; 4] = b"ITPK";
pub const IT_VERSION: u16 = 1;
pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_WASM_BYTES: usize = 1 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgramManifest {
    pub name: String,
    pub language: String,
    pub compiler_version: String,
    pub vm_version: String,
    pub entrypoint: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub abi: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgramPackage {
    pub manifest: ProgramManifest,
    pub wasm: Vec<u8>,
    pub code_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProgramReceipt {
    pub program_id: String,
    pub operation_id: String,
    pub status: String,
    pub return_data_hex: String,
    pub gas_used: u64,
    pub gas_limit: u64,
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum ProgramError {
    #[error("package is too large")]
    PackageTooLarge,
    #[error("invalid .it magic or version")]
    InvalidHeader,
    #[error("invalid package encoding: {0}")]
    Encoding(String),
    #[error("code hash mismatch")]
    HashMismatch,
    #[error("WASM validation failed: {0}")]
    WasmValidation(String),
    #[error("unsupported WASM import: {0}")]
    UnsupportedImport(String),
    #[error("runtime error: {0}")]
    Runtime(String),
    #[error("entrypoint is not exported: {0}")]
    MissingEntrypoint(String),
    #[error("gas limit must be positive")]
    InvalidGas,
}

fn hash_wasm(wasm: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(wasm);
    hex::encode(h.finalize())
}

impl ProgramPackage {
    pub fn from_wasm(manifest: ProgramManifest, wasm: Vec<u8>) -> Result<Self, ProgramError> {
        if wasm.len() > MAX_WASM_BYTES {
            return Err(ProgramError::PackageTooLarge);
        }
        validate_wasm(&wasm, &manifest.entrypoint)?;
        Ok(Self {
            manifest,
            code_hash: hash_wasm(&wasm),
            wasm,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProgramError> {
        let manifest = serde_json::to_vec(&self.manifest)
            .map_err(|e| ProgramError::Encoding(e.to_string()))?;
        if manifest.len() > u32::MAX as usize || self.wasm.len() > u32::MAX as usize {
            return Err(ProgramError::PackageTooLarge);
        }
        let mut out = Vec::with_capacity(16 + manifest.len() + self.wasm.len());
        out.extend_from_slice(IT_MAGIC);
        out.extend_from_slice(&IT_VERSION.to_le_bytes());
        out.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
        out.extend_from_slice(&(self.wasm.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0u8; 2]);
        out.extend_from_slice(&manifest);
        out.extend_from_slice(&self.wasm);
        out.extend_from_slice(self.code_hash.as_bytes());
        if out.len() > MAX_PACKAGE_BYTES {
            return Err(ProgramError::PackageTooLarge);
        }
        Ok(out)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProgramError> {
        if bytes.len() > MAX_PACKAGE_BYTES || bytes.len() < 16 + 64 {
            return Err(ProgramError::InvalidHeader);
        }
        if &bytes[..4] != IT_MAGIC || u16::from_le_bytes([bytes[4], bytes[5]]) != IT_VERSION {
            return Err(ProgramError::InvalidHeader);
        }
        let ml = u32::from_le_bytes(bytes[6..10].try_into().unwrap()) as usize;
        let wl = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
        let start = 16usize;
        let wasm_start = start
            .checked_add(ml)
            .ok_or(ProgramError::Encoding("manifest overflow".into()))?;
        let hash_start = wasm_start
            .checked_add(wl)
            .ok_or(ProgramError::Encoding("wasm overflow".into()))?;
        if hash_start + 64 != bytes.len() {
            return Err(ProgramError::Encoding("length mismatch".into()));
        }
        let manifest: ProgramManifest = serde_json::from_slice(&bytes[start..wasm_start])
            .map_err(|e| ProgramError::Encoding(e.to_string()))?;
        let wasm = bytes[wasm_start..hash_start].to_vec();
        let code_hash = String::from_utf8(bytes[hash_start..].to_vec())
            .map_err(|e| ProgramError::Encoding(e.to_string()))?;
        let package = Self {
            manifest,
            wasm,
            code_hash,
        };
        package.verify()?;
        Ok(package)
    }

    pub fn verify(&self) -> Result<(), ProgramError> {
        if self.code_hash != hash_wasm(&self.wasm) {
            return Err(ProgramError::HashMismatch);
        }
        validate_wasm(&self.wasm, &self.manifest.entrypoint)
    }

    pub fn program_id(&self) -> String {
        format!("it1{}", &self.code_hash[..40])
    }
}

fn validate_wasm(wasm: &[u8], entrypoint: &str) -> Result<(), ProgramError> {
    if wasm.len() > MAX_WASM_BYTES {
        return Err(ProgramError::PackageTooLarge);
    }
    let mut features = WasmFeatures::default();
    features.set(WasmFeatures::REFERENCE_TYPES, false);
    features.set(WasmFeatures::THREADS, false);
    features.set(WasmFeatures::SIMD, false);
    features.set(WasmFeatures::BULK_MEMORY, false);
    let mut validator = Validator::new_with_features(features);
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.map_err(|e| ProgramError::WasmValidation(e.to_string()))? {
            Payload::ImportSection(section) => {
                for import in section {
                    let import = import.map_err(|e| ProgramError::WasmValidation(e.to_string()))?;
                    if import.module != "env" || import.name != "host_log" {
                        return Err(ProgramError::UnsupportedImport(format!(
                            "{}::{}",
                            import.module, import.name
                        )));
                    }
                }
            }
            _ => {}
        }
    }
    validator
        .validate_all(wasm)
        .map_err(|e| ProgramError::WasmValidation(e.to_string()))?;
    if entrypoint.is_empty() {
        return Err(ProgramError::MissingEntrypoint(entrypoint.into()));
    }
    Ok(())
}

#[derive(Default)]
struct HostState {
    logs: Vec<i32>,
}

/// Executes a zero-argument `i32 -> i32` entrypoint with deterministic fuel.
pub fn execute(package: &ProgramPackage, gas_limit: u64) -> Result<(Vec<u8>, u64), ProgramError> {
    if gas_limit == 0 {
        return Err(ProgramError::InvalidGas);
    }
    package.verify()?;
    let mut config = Config::default();
    config.consume_fuel(true);
    let engine = Engine::new(&config);
    let module = Module::new(&engine, &package.wasm[..])
        .map_err(|e| ProgramError::Runtime(e.to_string()))?;
    let mut store = Store::new(&engine, HostState::default());
    store
        .add_fuel(gas_limit)
        .map_err(|e| ProgramError::Runtime(e.to_string()))?;
    let mut linker = Linker::new(&engine);
    linker
        .func_wrap(
            "env",
            "host_log",
            |mut caller: Caller<'_, HostState>, value: i32| {
                caller.data_mut().logs.push(value);
            },
        )
        .map_err(|e| ProgramError::Runtime(e.to_string()))?;
    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| ProgramError::Runtime(e.to_string()))?
        .start(&mut store)
        .map_err(|e| ProgramError::Runtime(e.to_string()))?;
    let func: TypedFunc<(), i32> = instance
        .get_typed_func(&store, &package.manifest.entrypoint)
        .map_err(|_| ProgramError::MissingEntrypoint(package.manifest.entrypoint.clone()))?;
    let value = func
        .call(&mut store, ())
        .map_err(|e| ProgramError::Runtime(e.to_string()))?;
    let used = store.fuel_consumed().unwrap_or(0);
    Ok((value.to_le_bytes().to_vec(), used))
}

/// Builds the intentionally small Rust frontend supported by the MVP.
///
/// Accepted source is a deterministic function such as
/// `fn main() -> i32 { 7 }` or `fn main() -> i32 { return 7; }`.
/// This is a safe bootstrap language, not a general rustc replacement.
pub fn compile_rust_source(
    name: impl Into<String>,
    source: &str,
) -> Result<ProgramPackage, ProgramError> {
    let source = source.trim();
    if source.contains("unsafe") || source.contains("extern crate") || source.contains("std::") {
        return Err(ProgramError::WasmValidation(
            "Rust source uses a disallowed feature".into(),
        ));
    }
    if !source.contains("fn main") || !source.contains("-> i32") {
        return Err(ProgramError::WasmValidation(
            "Rust MVP source must define fn main() -> i32".into(),
        ));
    }
    let body = source.split_once('{').map(|(_, rest)| rest).unwrap_or("");
    let body = body.split('}').next().unwrap_or(body);
    let value_text = body
        .split_once("return")
        .map(|(_, rest)| rest)
        .unwrap_or(body)
        .trim()
        .trim_end_matches(';')
        .trim();
    let value: i32 = value_text.parse().map_err(|_| {
        ProgramError::WasmValidation("Rust MVP main must return a signed i32 literal".into())
    })?;
    let wat_source = format!("(module (func (export \"main\") (result i32) i32.const {value}))");
    let wasm =
        wat::parse_str(wat_source).map_err(|e| ProgramError::WasmValidation(e.to_string()))?;
    ProgramPackage::from_wasm(
        ProgramManifest {
            name: name.into(),
            language: "rust".into(),
            compiler_version: "it-rust-subset-0.1".into(),
            vm_version: "1".into(),
            entrypoint: "main".into(),
            capabilities: vec![],
            abi: BTreeMap::new(),
        },
        wasm,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_round_trip_and_execute() {
        let wasm =
            wat::parse_str("(module (func (export \"main\") (result i32) i32.const 7))").unwrap();
        let m = ProgramManifest {
            name: "demo".into(),
            language: "rust".into(),
            compiler_version: "0.1".into(),
            vm_version: "1".into(),
            entrypoint: "main".into(),
            capabilities: vec![],
            abi: BTreeMap::new(),
        };
        let p = ProgramPackage::from_wasm(m, wasm).unwrap();
        let bytes = p.encode().unwrap();
        let q = ProgramPackage::decode(&bytes).unwrap();
        let (out, gas) = execute(&q, 100_000).unwrap();
        assert_eq!(i32::from_le_bytes(out.try_into().unwrap()), 7);
        assert!(gas > 0);
    }
}
