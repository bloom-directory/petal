//! Canonical contract identifiers and WIT sources for Bloom Petals.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;

pub const ROUTE_PACKAGE: &str = "bloom:route@0.1.0";
pub const ROUTE_WORLD: &str = "route-file";
pub const SIGNING_INTERFACE: &str = "bloom:sign/signing@0.1.0";
pub const PACKAGE_SCHEMA: &str = "bloom.petal.package.v1";
pub const ROUTE_INDEX_SCHEMA: &str = "bloom.petal.route-index.v1";
pub const BUILD_MANIFEST_SCHEMA: &str = "bloom.petal.build-manifest.v1";
pub const PACKAGE_DIGEST_PREFIX: &[u8] = b"bloom.petal.package.v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostInterface {
    RouteTypes,
    HttpFetch,
    StoreKv,
    SignSigning,
    TxOutbox,
    ChainRead,
    VfsReadwrite,
    EnvRuntime,
}

impl HostInterface {
    pub const fn capabilities(self) -> &'static [&'static str] {
        match self {
            Self::RouteTypes | Self::EnvRuntime => &[],
            Self::HttpFetch => &["bloom:http"],
            Self::StoreKv => &["bloom:store"],
            Self::SignSigning => &["bloom:sign"],
            Self::TxOutbox => &["bloom:tx.outbox"],
            Self::ChainRead => &["bloom:chain"],
            Self::VfsReadwrite => &["bloom:vfs.read", "bloom:vfs.write"],
        }
    }
}

pub const WIT_FILES: &[(&str, &[u8])] = &[
    ("route.wit", include_bytes!("../wit/route/route.wit")),
    (
        "deps/chain/chain.wit",
        include_bytes!("../wit/route/deps/chain/chain.wit"),
    ),
    (
        "deps/env/env.wit",
        include_bytes!("../wit/route/deps/env/env.wit"),
    ),
    (
        "deps/http/http.wit",
        include_bytes!("../wit/route/deps/http/http.wit"),
    ),
    (
        "deps/sign-v0.1/sign.wit",
        include_bytes!("../wit/route/deps/sign-v0.1/sign.wit"),
    ),
    (
        "deps/store/store.wit",
        include_bytes!("../wit/route/deps/store/store.wit"),
    ),
    (
        "deps/tx/outbox.wit",
        include_bytes!("../wit/route/deps/tx/outbox.wit"),
    ),
    (
        "deps/vfs/vfs.wit",
        include_bytes!("../wit/route/deps/vfs/vfs.wit"),
    ),
];

pub const RUST_TEMPLATE_FILES: &[(&str, &str)] = &[
    (
        "petal.toml",
        include_str!("../templates/rust-route-petal/petal.toml"),
    ),
    (
        "petal-build.toml",
        include_str!("../templates/rust-route-petal/petal-build.toml"),
    ),
    (
        "README.md",
        include_str!("../templates/rust-route-petal/README.md"),
    ),
    (
        "AGENTS.md",
        include_str!("../templates/rust-route-petal/AGENTS.md"),
    ),
    (
        "route/Cargo.toml",
        include_str!("../templates/rust-route-petal/route/Cargo.toml"),
    ),
    (
        "route/src/lib.rs",
        include_str!("../templates/rust-route-petal/route/src/lib.rs"),
    ),
    (
        "route/files/status.json.rs",
        include_str!("../templates/rust-route-petal/route/files/status.json.rs"),
    ),
];

pub fn wit_digest() -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in WIT_FILES {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    format!("{:x}", hasher.finalize())
}

pub fn write_wit_tree(destination: impl AsRef<Path>) -> io::Result<()> {
    let destination = destination.as_ref();
    for (relative, bytes) in WIT_FILES {
        let path = destination.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
    }
    Ok(())
}

pub fn host_interface(import: &str) -> Option<HostInterface> {
    match import {
        "bloom:route/types@0.1.0" => Some(HostInterface::RouteTypes),
        "bloom:http/fetch@0.1.0" => Some(HostInterface::HttpFetch),
        "bloom:store/kv@0.1.0" => Some(HostInterface::StoreKv),
        "bloom:sign/signing@0.1.0" => Some(HostInterface::SignSigning),
        "bloom:tx/outbox@0.1.0" => Some(HostInterface::TxOutbox),
        "bloom:chain/read@0.1.0" => Some(HostInterface::ChainRead),
        "bloom:vfs/readwrite@0.1.0" => Some(HostInterface::VfsReadwrite),
        "bloom:env/runtime@0.1.0" => Some(HostInterface::EnvRuntime),
        _ => None,
    }
}

pub fn capabilities_for_import(import: &str) -> Option<&'static [&'static str]> {
    host_interface(import).map(HostInterface::capabilities)
}

pub fn capability_for_import(import: &str) -> Option<&'static str> {
    capabilities_for_import(import).and_then(|caps| caps.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wit_digest_is_stable_and_complete() {
        assert_eq!(wit_digest().len(), 64);
        assert_eq!(WIT_FILES.len(), 8);
        assert!(WIT_FILES.iter().all(|(_, bytes)| !bytes.is_empty()));
    }

    #[test]
    fn writes_the_embedded_tree() {
        let tmp = tempfile::tempdir().unwrap();
        write_wit_tree(tmp.path()).unwrap();
        for (path, bytes) in WIT_FILES {
            assert_eq!(fs::read(tmp.path().join(path)).unwrap(), *bytes);
        }
    }

    #[test]
    fn maps_contract_imports_to_capabilities() {
        assert_eq!(
            capability_for_import("bloom:sign/signing@0.1.0"),
            Some("bloom:sign")
        );
        assert_eq!(capability_for_import("bloom:env/runtime@0.1.0"), None);
        assert_eq!(
            capabilities_for_import("bloom:vfs/readwrite@0.1.0"),
            Some(&["bloom:vfs.read", "bloom:vfs.write"][..])
        );
        assert_eq!(host_interface("bloom:sign/signing@9.9.9"), None);
        assert_eq!(capability_for_import("unknown:host/api@1.0.0"), None);
    }
}
