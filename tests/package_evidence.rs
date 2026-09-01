use bloom_petal_contract::{
    CheckedPackageRequest, FileDigestEntry, PackageCheckError, PackageEvidence,
    RequestedRoutePermission, check_package_request, package_hash_from_entries,
    parse_manifest_bounds,
};

fn manifest() -> String {
    format!(
        r#"schema = {:?}
name = "example"
[caps]
allowed = ["bloom:sign", "bloom:key.derive"]
[sign]
allowed_intents = ["example.action", "example.other"]
[[key.derive]]
route = "action.tx"
operation_classes = ["example.action"]
"#,
        bloom_petal_contract::PACKAGE_SCHEMA
    )
}

fn claim(path: &str, bytes: &[u8]) -> FileDigestEntry {
    FileDigestEntry {
        path: path.into(),
        byte_len: bytes.len() as u64,
        blake3_hex: blake3::hash(bytes).to_hex().to_string(),
    }
}

fn fixture() -> (PackageEvidence, Vec<RequestedRoutePermission>) {
    let manifest_utf8 = manifest();
    // These bytes only supply static digest claims; Machine still validates WASM.
    let entries = vec![
        claim("petal.toml", manifest_utf8.as_bytes()),
        claim("README.md", b"# Example"),
        claim("AGENTS.md", b"Example agent instructions"),
        claim("petal/example/action.tx.wasm", b"static evidence fixture"),
    ];
    let evidence = PackageEvidence {
        package_hash: package_hash_from_entries(&entries).unwrap(),
        file_pages: vec![entries],
        manifest_utf8,
    };
    let routes = vec![RequestedRoutePermission {
        route_id: "r000001".into(),
        source_path: "petal/example/action.tx.wasm".into(),
        capabilities: vec!["bloom:sign".into(), "bloom:key.derive".into()],
        signing_operations: vec!["example.action".into()],
        key_derive_operations: vec!["example.action".into()],
    }];
    (evidence, routes)
}

fn rehash(evidence: &mut PackageEvidence) {
    evidence.package_hash = package_hash_from_entries(
        &evidence
            .file_pages
            .iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

fn set_manifest(evidence: &mut PackageEvidence, manifest_utf8: String) {
    evidence.manifest_utf8 = manifest_utf8;
    let entry = evidence
        .file_pages
        .iter_mut()
        .flatten()
        .find(|entry| entry.path == "petal.toml")
        .unwrap();
    *entry = claim("petal.toml", evidence.manifest_utf8.as_bytes());
    rehash(evidence);
}

#[test]
fn accepts_narrowed_permissions_and_binds_the_supplied_manifest_bytes() {
    let (mut evidence, routes) = fixture();
    let expected_hash = evidence.package_hash.clone();
    let checked = check_package_request(evidence.clone(), routes.clone()).unwrap();
    assert_eq!(checked.evidence.package_hash, expected_hash);
    assert_eq!(checked.routes, routes);
    evidence
        .manifest_utf8
        .push_str("\n# altered after hashing\n");
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Hash(_))
    ));
}

#[test]
fn hash_is_independent_of_entry_order_and_binds_paths_lengths_and_digests() {
    let (evidence, _) = fixture();
    let mut entries = evidence.file_pages[0].clone();
    entries.reverse();
    assert_eq!(
        package_hash_from_entries(&entries).unwrap(),
        evidence.package_hash
    );
    for kind in 0..3 {
        let mut changed = entries.clone();
        match kind {
            0 => changed[0].path.push('x'),
            1 => changed[0].byte_len += 1,
            _ => changed[0].blake3_hex = "00".repeat(32),
        }
        assert_ne!(
            package_hash_from_entries(&changed).unwrap(),
            evidence.package_hash
        );
    }
}

#[test]
fn package_hash_matches_the_existing_binary_preimage_format() {
    let entry = FileDigestEntry {
        path: "a".into(),
        byte_len: 258,
        blake3_hex: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".into(),
    };
    // Independent literal: domain, 32-bit LE path length, path, 64-bit LE
    // file length, then the 32 raw digest bytes (not their hexadecimal text).
    let preimage = b"bloom.petal.package.v1\0\x01\x00\x00\x00a\x02\x01\x00\x00\x00\x00\x00\x00\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\x15\x16\x17\x18\x19\x1a\x1b\x1c\x1d\x1e\x1f";
    assert_eq!(
        package_hash_from_entries(&[entry]).unwrap(),
        blake3::hash(preimage).to_hex().to_string()
    );
}

#[test]
fn a_same_length_manifest_substitution_does_not_bypass_the_digest_check() {
    let (mut evidence, routes) = fixture();
    evidence.manifest_utf8 = evidence.manifest_utf8.replace("example", "altered");
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Hash(_))
    ));
}

#[test]
fn static_package_dependencies_exclude_execution_and_service_runtimes() {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "tree",
            "--locked",
            "--offline",
            "-p",
            "bloom-petal-contract",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8(output.stdout).unwrap();
    for line in tree.lines() {
        let package = line.split_whitespace().next().unwrap();
        assert!(
            !package.starts_with("wasmtime")
                && !package.starts_with("bloom-broker")
                && !package.starts_with("bloom-signer")
                && !matches!(
                    package,
                    "bloom-petals" | "bloom-machine" | "bloom-service-runtime"
                ),
            "unexpected runtime dependency: {package}"
        );
    }
}

#[test]
fn rejects_duplicate_or_noncanonical_paths() {
    let (evidence, routes) = fixture();
    let mut duplicate = evidence.clone();
    duplicate
        .file_pages
        .push(vec![duplicate.file_pages[0][0].clone()]);
    assert!(matches!(
        check_package_request(duplicate, routes.clone()),
        Err(PackageCheckError::Path(_))
    ));
    for path in [
        "",
        "/absolute",
        "./README.md",
        "a/../README.md",
        "a//b",
        "a\\b",
        "a\0b",
        "a/",
    ] {
        let mut changed = evidence.clone();
        changed.file_pages[0][1].path = path.into();
        assert!(
            matches!(
                check_package_request(changed, routes.clone()),
                Err(PackageCheckError::Path(_))
            ),
            "{path:?}"
        );
    }
}

#[test]
fn rejects_bad_digest_encodings_and_package_hashes() {
    let (evidence, routes) = fixture();
    for digest in ["g".repeat(64), "A".repeat(64), "0".repeat(63)] {
        let mut changed = evidence.clone();
        changed.file_pages[0][0].blake3_hex = digest;
        assert!(matches!(
            check_package_request(changed, routes.clone()),
            Err(PackageCheckError::Hash(_))
        ));
    }
    let mut changed = evidence;
    changed.package_hash = "00".repeat(32);
    assert!(matches!(
        check_package_request(changed, routes),
        Err(PackageCheckError::Hash(_))
    ));
}

#[test]
fn rejects_incorrect_manifest_length_even_with_recomputed_package_hash() {
    let (mut evidence, routes) = fixture();
    evidence.file_pages[0][0].byte_len += 1;
    rehash(&mut evidence);
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Hash(_))
    ));
}

#[test]
fn rejects_missing_required_files() {
    for missing in ["petal.toml", "README.md", "AGENTS.md"] {
        let (mut evidence, routes) = fixture();
        evidence.file_pages[0].retain(|entry| entry.path != missing);
        rehash(&mut evidence);
        assert!(
            check_package_request(evidence, routes).is_err(),
            "{missing}"
        );
    }
}

#[test]
fn requires_each_source_route_exactly_once_with_its_derived_id() {
    let (evidence, routes) = fixture();
    assert!(matches!(
        check_package_request(evidence.clone(), vec![]),
        Err(PackageCheckError::Route(_))
    ));
    assert!(matches!(
        check_package_request(evidence.clone(), vec![routes[0].clone(), routes[0].clone()]),
        Err(PackageCheckError::Route(_))
    ));
    for (id, path) in [
        ("r000002", "petal/example/action.tx.wasm"),
        ("r000001", "artifacts/action.tx.wasm"),
    ] {
        let mut changed = routes.clone();
        changed[0].route_id = id.into();
        changed[0].source_path = path.into();
        assert!(matches!(
            check_package_request(evidence.clone(), changed),
            Err(PackageCheckError::Route(_))
        ));
    }
}

#[test]
fn duplicate_proposals_cannot_replace_a_missing_route_at_the_correct_count() {
    let (mut evidence, routes) = fixture();
    evidence.file_pages[0].push(claim("petal/example/z.wasm", b"second route"));
    rehash(&mut evidence);
    assert!(matches!(
        check_package_request(evidence, vec![routes[0].clone(), routes[0].clone()]),
        Err(PackageCheckError::Route(_))
    ));
}

#[test]
fn permissions_must_stay_within_the_manifest_and_route_key_declarations() {
    let (evidence, routes) = fixture();
    for kind in 0..3 {
        let mut changed = routes.clone();
        match kind {
            0 => changed[0].capabilities.push("bloom:http".into()),
            1 => changed[0]
                .signing_operations
                .push("undeclared.operation".into()),
            _ => changed[0]
                .key_derive_operations
                .push("example.other".into()),
        }
        assert!(matches!(
            check_package_request(evidence.clone(), changed),
            Err(PackageCheckError::Scope(_))
        ));
    }
}

#[test]
fn empty_permissions_do_not_inherit_package_wide_signing_authority() {
    let (evidence, mut routes) = fixture();
    routes[0].capabilities.clear();
    routes[0].signing_operations.clear();
    routes[0].key_derive_operations.clear();
    assert_eq!(
        check_package_request(evidence, routes.clone())
            .unwrap()
            .routes,
        routes
    );
}

#[test]
fn derives_route_order_from_patterns() {
    let (mut evidence, mut routes) = fixture();
    evidence.file_pages[0].push(claim("petal/example/$index.wasm", b"index"));
    routes[0].route_id = "r000002".into();
    routes.push(RequestedRoutePermission {
        route_id: "r000001".into(),
        source_path: "petal/example/$index.wasm".into(),
        capabilities: vec![],
        signing_operations: vec![],
        key_derive_operations: vec![],
    });
    rehash(&mut evidence);
    assert!(check_package_request(evidence, routes).is_ok());
}

#[test]
fn rejects_invalid_source_routes_even_with_complete_proposals() {
    for (path, original_id, added_id, expected_detail) in [
        (
            "petal/other/action.wasm",
            "r000001",
            "r000002",
            "extra petal root",
        ),
        (
            "petal/example/action.tx/child.wasm",
            "r000001",
            "r000002",
            "file route shadows descendant",
        ),
        (
            "petal/example/action.tx/$index.wasm",
            "r000001",
            "r000002",
            "conflicting Petal routes",
        ),
        (
            "petal/example/$unknown.wasm",
            "r000002",
            "r000001",
            "unsupported reserved route file",
        ),
        (
            "petal/example/[bad/child.wasm",
            "r000002",
            "r000001",
            "dynamic route segment missing ]",
        ),
    ] {
        let (mut evidence, mut routes) = fixture();
        evidence.file_pages[0].push(claim(path, b"invalid route"));
        routes[0].route_id = original_id.into();
        routes.push(RequestedRoutePermission {
            route_id: added_id.into(),
            source_path: path.into(),
            capabilities: vec![],
            signing_operations: vec![],
            key_derive_operations: vec![],
        });
        rehash(&mut evidence);
        match check_package_request(evidence, routes) {
            Err(PackageCheckError::Route(detail)) => {
                assert!(detail.contains(expected_detail), "{path}: {detail}")
            }
            result => panic!("{path}: expected {expected_detail}, got {result:?}"),
        }
    }
}

#[test]
fn shared_endpoint_binding_validator_accepts_only_the_existing_ascii_grammar() {
    use bloom_petal_contract::manifest::validate_binding_name;

    for valid in ["api", "API-01", "api_name", "0", "___"] {
        assert!(validate_binding_name(valid).is_ok(), "{valid}");
    }
    for invalid in ["", "api.example", "a/b", "with space", "åpi"] {
        assert!(
            matches!(
                validate_binding_name(invalid),
                Err(PackageCheckError::Manifest(_))
            ),
            "{invalid}"
        );
    }
}

#[test]
fn manifest_bounds_expose_static_declarations_and_reject_invalid_policy() {
    let bounds = parse_manifest_bounds(&manifest()).unwrap();
    assert_eq!(bounds.name, "example");
    assert_eq!(bounds.capabilities, ["bloom:key.derive", "bloom:sign"]);
    assert_eq!(
        bounds.signing_operations,
        ["example.action", "example.other"]
    );
    assert_eq!(bounds.key_derive["action.tx"], ["example.action"]);
    for invalid in [
        manifest().replace("bloom.petal.package.v1", "unknown.schema"),
        manifest().replace("name = \"example\"", "name = \"../bad\""),
        manifest().replace(
            "allowed_intents = [\"example.action\", \"example.other\"]",
            "allowed_intents = []",
        ),
        format!("{}\nunknown = true\n", manifest()),
        format!(
            "{}\n[[key.derive]]\nroute = \"action.tx\"\noperation_classes = [\"example.action\"]\n",
            manifest()
        ),
    ] {
        assert!(parse_manifest_bounds(&invalid).is_err(), "{invalid}");
    }
    let net_store = format!(
        "schema = {:?}\nname = \"example\"\n[caps]\nallowed = [\"bloom:http\", \"bloom:store\"]\n[[net.allow]]\nbinding = \"api\"\nhost = \"example.com\"\nmethods = [\"GET\"]\npaths = [\"/v1/*\"]\n[store]\nnamespaces = [\"public\"]\nsecret_namespaces = [\"secret\"]\n",
        bloom_petal_contract::PACKAGE_SCHEMA
    );
    let bounds = parse_manifest_bounds(&net_store).unwrap();
    assert_eq!(bounds.network[0].host, "example.com");
    assert_eq!(bounds.store_namespaces, ["public", "secret"]);
    assert_eq!(bounds.secret_store_namespaces, ["secret"]);
    assert!(
        parse_manifest_bounds(&net_store.replace("example.com", "https://example.com")).is_err()
    );
    assert!(parse_manifest_bounds(&net_store.replace("public", "../public")).is_err());
}

#[test]
fn wire_lengths_are_canonical_unsigned_decimal_strings_and_unknown_fields_fail() {
    let entry = claim("README.md", b"abc");
    let encoded = serde_json::to_value(&entry).unwrap();
    assert_eq!(encoded["byte_len"], "3");
    for invalid in [
        serde_json::json!(3),
        serde_json::json!("03"),
        serde_json::json!("+3"),
        serde_json::json!("-1"),
        serde_json::json!(" 3"),
        serde_json::json!("18446744073709551616"),
    ] {
        let mut changed = encoded.clone();
        changed["byte_len"] = invalid;
        assert!(serde_json::from_value::<FileDigestEntry>(changed).is_err());
    }
    for length in [0, u64::MAX] {
        let mut changed = entry.clone();
        changed.byte_len = length;
        assert_eq!(
            serde_json::from_str::<FileDigestEntry>(&serde_json::to_string(&changed).unwrap())
                .unwrap(),
            changed
        );
    }
    let (evidence, routes) = fixture();
    let request = CheckedPackageRequest { evidence, routes };
    for pointer in ["", "/evidence", "/evidence/file_pages/0/0", "/routes/0"] {
        let mut changed = serde_json::to_value(&request).unwrap();
        changed
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), serde_json::json!(true));
        assert!(
            serde_json::from_value::<CheckedPackageRequest>(changed).is_err(),
            "{pointer}"
        );
    }
}

fn many_files(count: usize, path_len: usize) -> (PackageEvidence, Vec<RequestedRoutePermission>) {
    let (mut evidence, routes) = fixture();
    let mut entries = evidence.file_pages.remove(0);
    for i in entries.len()..count {
        let path = format!("doc{i:04}{}", "x".repeat(path_len.saturating_sub(7)));
        entries.push(claim(&path, b""));
    }
    evidence.file_pages = entries.chunks(256).map(<[_]>::to_vec).collect();
    rehash(&mut evidence);
    (evidence, routes)
}

#[test]
fn file_page_and_entry_budgets_allow_boundaries_and_reject_one_over() {
    let (evidence, routes) = many_files(4096, 7);
    assert!(check_package_request(evidence.clone(), routes.clone()).is_ok());
    let mut too_many_pages = evidence.clone();
    too_many_pages.file_pages.push(vec![]);
    assert!(matches!(
        check_package_request(too_many_pages, routes.clone()),
        Err(PackageCheckError::Limit(_))
    ));
    let mut too_many_entries = evidence;
    too_many_entries.file_pages[0].push(claim("extra", b""));
    assert!(matches!(
        check_package_request(too_many_entries, routes),
        Err(PackageCheckError::Limit(_))
    ));
}

#[test]
fn route_budget_allows_256_and_rejects_257() {
    let (mut evidence, mut routes) = fixture();
    for i in 1..257 {
        let path = format!("petal/example/z{i:03}.wasm");
        evidence.file_pages[0].push(claim(&path, b"route"));
        routes.push(RequestedRoutePermission {
            route_id: format!("r{:06}", i + 1),
            source_path: path,
            capabilities: vec![],
            signing_operations: vec![],
            key_derive_operations: vec![],
        });
        if i == 255 {
            let mut allowed = evidence.clone();
            allowed.file_pages = allowed.file_pages[0]
                .chunks(256)
                .map(<[_]>::to_vec)
                .collect();
            rehash(&mut allowed);
            assert!(check_package_request(allowed, routes.clone()).is_ok());
        }
    }
    evidence.file_pages = evidence.file_pages[0]
        .chunks(256)
        .map(<[_]>::to_vec)
        .collect();
    rehash(&mut evidence);
    assert!(matches!(
        check_package_request(evidence.clone(), routes[..256].to_vec()),
        Err(PackageCheckError::Limit(_))
    ));
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Limit(_))
    ));
}

#[test]
fn each_nested_permission_list_is_bounded_at_256_items() {
    for kind in 0..3 {
        let (mut evidence, mut routes) = fixture();
        let items = (0..256)
            .map(|i| format!("operation.{i}"))
            .collect::<Vec<_>>();
        let list = format!("{:?}", items);
        let text = format!(
            "schema = {:?}\nname = \"example\"\n[caps]\nallowed = {list}\n[sign]\nallowed_intents = {list}\n[[key.derive]]\nroute = \"action.tx\"\noperation_classes = {list}\n",
            bloom_petal_contract::PACKAGE_SCHEMA
        );
        set_manifest(&mut evidence, text);
        routes[0].capabilities.clear();
        routes[0].signing_operations.clear();
        routes[0].key_derive_operations.clear();
        let target = match kind {
            0 => &mut routes[0].capabilities,
            1 => &mut routes[0].signing_operations,
            _ => &mut routes[0].key_derive_operations,
        };
        *target = items;
        assert!(check_package_request(evidence.clone(), routes.clone()).is_ok());
        let target = match kind {
            0 => &mut routes[0].capabilities,
            1 => &mut routes[0].signing_operations,
            _ => &mut routes[0].key_derive_operations,
        };
        target.push("one.over".into());
        assert!(matches!(
            check_package_request(evidence, routes),
            Err(PackageCheckError::Limit(_))
        ));
    }
}

#[test]
fn manifest_budget_allows_64_kib_and_rejects_one_more_byte() {
    let (mut evidence, routes) = fixture();
    let text = format!(
        "{}\n#{}",
        manifest(),
        "x".repeat(65536 - manifest().len() - 2)
    );
    set_manifest(&mut evidence, text);
    assert!(check_package_request(evidence.clone(), routes.clone()).is_ok());
    evidence.manifest_utf8.push('x');
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Limit(_))
    ));
}

#[test]
fn path_budget_preserves_stricter_ustar_semantics_and_rejects_over_512_bytes() {
    let (mut evidence, routes) = fixture();
    evidence.file_pages[0].push(claim(
        &format!("{}/{}", "x".repeat(155), "y".repeat(100)),
        b"",
    ));
    rehash(&mut evidence);
    assert!(check_package_request(evidence.clone(), routes.clone()).is_ok());
    evidence.file_pages[0].last_mut().unwrap().path =
        format!("{}/{}", "x".repeat(156), "y".repeat(100));
    assert!(matches!(
        check_package_request(evidence.clone(), routes.clone()),
        Err(PackageCheckError::Path(_))
    ));
    evidence.file_pages[0].last_mut().unwrap().path = "x".repeat(513);
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Limit(_))
    ));
}

#[test]
fn evidence_bounds_do_not_limit_the_size_of_a_claimed_wasm_artifact() {
    let (mut evidence, routes) = fixture();
    evidence.file_pages[0][3].byte_len = u64::MAX;
    rehash(&mut evidence);
    assert!(check_package_request(evidence, routes).is_ok());
}

#[test]
fn canonical_encoded_request_budget_allows_exactly_768_kib() {
    let (mut evidence, routes) = many_files(3700, 90);
    let encoded_len = |evidence: &PackageEvidence| {
        serde_jcs::to_vec(&CheckedPackageRequest {
            evidence: evidence.clone(),
            routes: routes.clone(),
        })
        .unwrap()
        .len()
    };
    let base_len = encoded_len(&evidence);
    let padding = 768 * 1024 - base_len;
    assert!(
        padding < 60_000,
        "fixture must leave room within the manifest budget: {padding}"
    );
    let mut text = manifest();
    text.push_str("\n#");
    text.push_str(&"x".repeat(padding - 3)); // newline is escaped in JSON
    set_manifest(&mut evidence, text);
    // Decimal length gains digits as the comment grows; tune against the wire encoding.
    while encoded_len(&evidence) > 768 * 1024 {
        let mut text = evidence.manifest_utf8.clone();
        text.pop();
        set_manifest(&mut evidence, text);
    }
    assert_eq!(encoded_len(&evidence), 768 * 1024);
    assert!(check_package_request(evidence.clone(), routes.clone()).is_ok());
    let mut text = evidence.manifest_utf8.clone();
    text.push('x');
    set_manifest(&mut evidence, text);
    assert_eq!(encoded_len(&evidence), 768 * 1024 + 1);
    assert!(matches!(
        check_package_request(evidence, routes),
        Err(PackageCheckError::Limit(_))
    ));
}
