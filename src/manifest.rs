//! The package manifest grammar and its pure policy ceilings.

use crate::{PackageCheckError, paths::RouteRecord};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Static upper bounds only; source artifacts and runtime metadata are not executed here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestBounds {
    pub name: String,
    pub capabilities: Vec<String>,
    pub signing_operations: Vec<String>,
    pub key_derive: BTreeMap<String, Vec<String>>,
    pub network: Vec<NetAllowToml>,
    pub store_namespaces: Vec<String>,
    pub secret_store_namespaces: Vec<String>,
}

/// Parse the single manifest grammar without imposing full-package validation on
/// loader helpers that intentionally read only one part of the policy.
pub fn parse_manifest(text: &str) -> Result<PetalToml, toml::de::Error> {
    toml::from_str(text)
}

pub fn parse_manifest_bounds(text: &str) -> Result<ManifestBounds, PackageCheckError> {
    let manifest =
        parse_manifest(text).map_err(|error| PackageCheckError::Manifest(error.to_string()))?;
    validate_package_schema(&manifest)?;
    validate_petal_name(&manifest.name)?;
    let capabilities = manifest
        .caps
        .allowed
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let signing_operations = manifest
        .sign
        .allowed_intents
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    validate_sign_policy(&capabilities, &signing_operations)?;
    validate_store_policy(&capabilities, &manifest.store)?;
    validate_net_policy(&capabilities, &manifest.net)?;
    let key_derive = validate_key_derive_declarations(&manifest.key, &signing_operations)?;
    let secret_store_namespaces = manifest
        .store
        .secret_namespaces
        .into_iter()
        .collect::<BTreeSet<_>>();
    let store_namespaces = manifest
        .store
        .namespaces
        .into_iter()
        .chain(secret_store_namespaces.iter().cloned())
        .collect::<BTreeSet<_>>();
    Ok(ManifestBounds {
        name: manifest.name,
        capabilities: capabilities.into_iter().collect(),
        signing_operations: signing_operations.into_iter().collect(),
        key_derive,
        network: manifest.net.allow,
        store_namespaces: store_namespaces.into_iter().collect(),
        secret_store_namespaces: secret_store_namespaces.into_iter().collect(),
    })
}

pub fn validate_package_schema(manifest: &PetalToml) -> Result<(), PackageCheckError> {
    let schema = crate::PACKAGE_SCHEMA;
    if manifest.schema.as_deref() != Some(schema) {
        return Err(PackageCheckError::Manifest(format!(
            "Petal package petal.toml must set schema = {schema:?}"
        )));
    }
    Ok(())
}

/// Validate the shared manifest and runtime endpoint-binding name grammar.
pub fn validate_binding_name(binding: &str) -> Result<(), PackageCheckError> {
    if binding.is_empty()
        || !binding
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(PackageCheckError::Manifest(format!(
            "endpoint binding {binding:?} must contain only ASCII letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PetalToml {
    #[serde(default)]
    pub schema: Option<String>,
    pub name: String,
    #[serde(default)]
    pub consent: ConsentPolicy,
    #[serde(default)]
    pub caps: PetalCaps,
    #[serde(default)]
    pub net: NetPolicyToml,
    #[serde(default)]
    pub sign: SignPolicy,
    #[serde(default)]
    pub key: KeyPolicyToml,
    #[serde(default)]
    pub store: StorePolicyToml,
    #[serde(default, rename = "source")]
    pub _source: Option<SourcePolicyToml>,
    #[serde(default, rename = "build")]
    pub _build: Option<BuildPolicyToml>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ConsentPolicy {
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PetalCaps {
    #[serde(default)]
    pub allowed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct NetPolicyToml {
    #[serde(default)]
    pub allow: Vec<NetAllowToml>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetAllowToml {
    #[serde(default)]
    pub binding: Option<String>,
    pub host: String,
    #[serde(default)]
    pub methods: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SignPolicy {
    #[serde(default)]
    pub allowed_intents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct KeyPolicyToml {
    #[serde(default, rename = "derive")]
    pub derive_routes: Vec<KeyDerivePolicyToml>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyDerivePolicyToml {
    pub route: String,
    pub operation_classes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StorePolicyToml {
    #[serde(default)]
    pub namespaces: Vec<String>,
    #[serde(default)]
    pub secret_namespaces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePolicyToml {
    #[serde(rename = "kind")]
    pub _kind: String,
    #[serde(rename = "repository")]
    pub _repository: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildPolicyToml {
    #[serde(rename = "command")]
    pub _command: String,
    #[serde(default, rename = "outputs")]
    pub _outputs: Vec<String>,
}

pub fn validate_petal_name(name: &str) -> Result<(), PackageCheckError> {
    let valid = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(PackageCheckError::Manifest(format!(
            "invalid Petal name {name:?}; expected only ASCII letters, digits, '-' or '_'"
        )))
    }
}

pub fn validate_sign_policy(
    allowed_caps: &BTreeSet<String>,
    allowed_sign_intents: &BTreeSet<String>,
) -> Result<(), PackageCheckError> {
    if allowed_caps.contains("bloom:sign") && allowed_sign_intents.is_empty() {
        return Err(PackageCheckError::Manifest(
            "Petal package cap bloom:sign requires [sign].allowed_intents".into(),
        ));
    }
    for intent in allowed_sign_intents {
        validate_sign_intent(intent)?;
    }
    Ok(())
}

pub fn validate_key_derive_policy(
    policy: &KeyPolicyToml,
    routes: &[RouteRecord],
    allowed_sign_intents: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<String>>, PackageCheckError> {
    let route_patterns = routes
        .iter()
        .map(|route| route.pattern.as_str())
        .collect::<BTreeSet<_>>();
    for declaration in &policy.derive_routes {
        if !route_patterns.contains(declaration.route.as_str()) {
            return Err(PackageCheckError::Manifest(format!(
                "Petal [[key.derive]] declares unknown route {:?}",
                declaration.route
            )));
        }
    }
    validate_key_derive_declarations(policy, allowed_sign_intents)
}

fn validate_key_derive_declarations(
    policy: &KeyPolicyToml,
    allowed_sign_intents: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<String>>, PackageCheckError> {
    let mut declarations = BTreeMap::new();
    for declaration in &policy.derive_routes {
        if declaration.operation_classes.is_empty() {
            return Err(PackageCheckError::Manifest(format!(
                "Petal [[key.derive]] route {:?} operation_classes must be non-empty",
                declaration.route
            )));
        }

        let mut classes = BTreeSet::new();
        for operation_class in &declaration.operation_classes {
            validate_sign_intent(operation_class)?;
            if !allowed_sign_intents.contains(operation_class) {
                return Err(PackageCheckError::Manifest(format!(
                    "Petal [[key.derive]] operation class {operation_class:?} is not declared in [sign].allowed_intents"
                )));
            }
            if !classes.insert(operation_class.clone()) {
                return Err(PackageCheckError::Manifest(format!(
                    "Petal [[key.derive]] route {:?} has duplicate operation class {operation_class:?}",
                    declaration.route
                )));
            }
        }
        if declarations
            .insert(declaration.route.clone(), classes.into_iter().collect())
            .is_some()
        {
            return Err(PackageCheckError::Manifest(format!(
                "Petal [[key.derive]] has a duplicate declaration for route {:?}",
                declaration.route
            )));
        }
    }
    Ok(declarations)
}

pub fn validate_store_policy(
    allowed_caps: &BTreeSet<String>,
    policy: &StorePolicyToml,
) -> Result<(), PackageCheckError> {
    if allowed_caps.contains("bloom:store")
        && policy.namespaces.is_empty()
        && policy.secret_namespaces.is_empty()
    {
        return Err(PackageCheckError::Manifest(
            "Petal package cap bloom:store requires [store].namespaces or [store].secret_namespaces".into(),
        ));
    }
    for namespace in policy.namespaces.iter().chain(&policy.secret_namespaces) {
        validate_store_namespace(namespace)?;
    }
    Ok(())
}

pub fn validate_net_policy(
    allowed_caps: &BTreeSet<String>,
    policy: &NetPolicyToml,
) -> Result<(), PackageCheckError> {
    if allowed_caps.contains("bloom:http") && policy.allow.is_empty() {
        return Err(PackageCheckError::Manifest(
            "Petal package cap bloom:http requires at least one [[net.allow]] rule".into(),
        ));
    }
    for rule in &policy.allow {
        if rule.host.trim().is_empty() || rule.methods.is_empty() {
            return Err(PackageCheckError::Manifest(
                "Petal [[net.allow]] rules require a host and at least one method".into(),
            ));
        }
        if rule.host.trim() != rule.host || url::Host::parse(&rule.host).is_err() {
            return Err(PackageCheckError::Manifest(format!(
                "Petal [[net.allow]] host {:?} must be a bare DNS name or IP address without a scheme, path, or port",
                rule.host
            )));
        }
        if rule.methods.iter().any(|method| method.trim().is_empty()) {
            return Err(PackageCheckError::Manifest(
                "Petal [[net.allow]] methods must be non-empty".into(),
            ));
        }
        if let Some(binding) = rule.binding.as_deref() {
            validate_binding_name(binding)?;
        }
        if rule.paths.is_empty() || rule.paths.iter().any(|path| path.trim().is_empty()) {
            return Err(PackageCheckError::Manifest(
                "Petal [[net.allow]] paths must be explicit and non-empty; use \"/*\" for all paths"
                    .into(),
            ));
        }
    }
    Ok(())
}

fn validate_store_namespace(namespace: &str) -> Result<(), PackageCheckError> {
    if namespace.is_empty() || namespace.len() > 128 {
        return Err(PackageCheckError::Manifest(
            "Petal store namespace must be 1..128 bytes".into(),
        ));
    }
    if namespace.contains('/')
        || !namespace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err(PackageCheckError::Manifest(format!(
            "Petal store namespace {namespace:?} contains an unsupported byte"
        )));
    }
    Ok(())
}

pub fn validate_sign_intent(intent: &str) -> Result<(), PackageCheckError> {
    if intent.is_empty() || intent.len() > 128 {
        return Err(PackageCheckError::Manifest(
            "Petal sign intent must be 1..128 bytes".into(),
        ));
    }
    if !intent
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b':'))
    {
        return Err(PackageCheckError::Manifest(format!(
            "Petal sign intent {intent:?} contains an unsupported byte"
        )));
    }
    Ok(())
}
