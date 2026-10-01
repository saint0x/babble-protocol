use babble_crypto::Keypair;
use babble_identity::{Identity, IdentityKind};
use babble_object::{
    Object, Surface, SurfaceRole, SurfaceTarget,
    bundle::{
        BUNDLE_VERSION, BundleFile, BundleFileKind, BundleManifest, MAX_BUNDLE_BYTES,
        MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_FILES, MAX_BUNDLE_MANIFEST_BYTES, MAX_BUNDLE_PATH_BYTES,
    },
};
use babble_types::{Canonical, Hash, ObjectId};
use serde_json::{Value, json};

fn file(path: &str, kind: BundleFileKind, media_type: &str) -> BundleFile {
    let integrity = Hash::from_bytes(path.as_bytes());
    BundleFile {
        path: path.into(),
        source_uri: format!("babble://blobs/{integrity}"),
        integrity,
        size_bytes: path.len() as u64,
        media_type: media_type.into(),
        kind,
    }
}

fn manifest() -> BundleManifest {
    BundleManifest {
        version: BUNDLE_VERSION,
        entry_path: "index.html".into(),
        files: vec![
            file("app.js", BundleFileKind::Script, "text/javascript"),
            file("index.html", BundleFileKind::Document, "text/html"),
        ],
    }
}

fn surface(bundle: &BundleManifest) -> Surface {
    let entry = bundle.entry_file().unwrap();
    Surface {
        role: SurfaceRole::Feed,
        target: SurfaceTarget::Web,
        entry: entry.source_uri.clone(),
        integrity: Some(entry.integrity.clone()),
        bundle: Some(bundle.clone()),
    }
}

fn identity() -> (Identity, Keypair) {
    let key = Keypair::from_ed25519_secret_hex(&"42".repeat(32)).unwrap();
    (
        Identity::create(IdentityKind::Person, "bundle-tests", &key).unwrap(),
        key,
    )
}

#[test]
fn canonical_identity_uses_versioned_encoding_and_rejects_invalid_manifests() {
    let bundle = manifest();
    let encoded = bundle.canonical_bytes().unwrap();
    assert!(encoded.starts_with(b"babble.canonical.v1\0"));
    assert_eq!(bundle.hash().unwrap(), Hash::from_bytes(&encoded));
    assert_eq!(bundle.hash().unwrap(), bundle.canonical_hash().unwrap());
    let roundtrip: BundleManifest =
        serde_json::from_slice(&serde_json::to_vec(&bundle).unwrap()).unwrap();
    assert_eq!(bundle, roundtrip);
    assert_eq!(bundle.hash().unwrap(), roundtrip.hash().unwrap());
    for version in [0, 2, u32::MAX] {
        let mut invalid = bundle.clone();
        invalid.version = version;
        assert!(invalid.validate().is_err());
        assert!(invalid.hash().is_err());
    }
}

#[test]
fn serde_rejects_missing_unknown_and_malformed_fields() {
    let value = serde_json::to_value(manifest()).unwrap();
    for field in ["version", "entry_path", "files"] {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<BundleManifest>(missing).is_err(),
            "{field}"
        );
    }
    for field in [
        "path",
        "source_uri",
        "integrity",
        "size_bytes",
        "media_type",
        "kind",
    ] {
        let mut missing = value.clone();
        missing["files"][0].as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<BundleManifest>(missing).is_err(),
            "{field}"
        );
    }
    let mut unknown = value.clone();
    unknown["future_version"] = json!(1);
    assert!(serde_json::from_value::<BundleManifest>(unknown).is_err());
    let mut unknown = value.clone();
    unknown["files"][0]["executable"] = json!(true);
    assert!(serde_json::from_value::<BundleManifest>(unknown).is_err());
    for kind in [
        json!("Script"),
        json!("worker"),
        json!({"script": {}}),
        Value::Null,
    ] {
        let mut invalid = value.clone();
        invalid["files"][0]["kind"] = kind;
        assert!(serde_json::from_value::<BundleManifest>(invalid).is_err());
    }
    for size in [json!(-1), json!(1.5), json!("1"), Value::Null] {
        let mut invalid = value.clone();
        invalid["files"][0]["size_bytes"] = size;
        assert!(serde_json::from_value::<BundleManifest>(invalid).is_err());
    }
    assert_eq!(value["files"][0]["kind"], "script");
    let schema = serde_json::to_value(schemars::schema_for!(BundleManifest)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["$defs"]["BundleFile"]["additionalProperties"], false);
}

#[test]
fn logical_paths_are_strict_ascii_url_segments() {
    for path in [
        "",
        ".",
        "..",
        "./index.html",
        "../index.html",
        "/index.html",
        "//host/a",
        "a//b",
        "a/",
        "a/./b",
        "a/../b",
        "a\\b",
        "a?x",
        "a#x",
        "a%20b",
        "a/%2e%2e/b",
        "a/%252e/b",
        "a:b",
        "a b",
        "a\tb",
        "a\nb",
        "a\0b",
        "a\u{7f}",
        "caf\u{e9}.html",
        "a+z",
        "a;b",
        "a@b",
    ] {
        let mut bundle = manifest();
        bundle.files[0].path = path.into();
        bundle.files.sort_by(|a, b| a.path.cmp(&b.path));
        assert!(bundle.validate().is_err(), "admitted {path:?}");
        let mut bundle = manifest();
        bundle.entry_path = path.into();
        assert!(bundle.validate().is_err(), "admitted entry {path:?}");
    }
    for path in [
        "assets/release..v1.js",
        "A-Z_09/~module.min.js",
        ".well-known/app.js",
    ] {
        let mut bundle = manifest();
        bundle.files[0].path = path.into();
        bundle.files.sort_by(|a, b| a.path.cmp(&b.path));
        bundle.validate().unwrap();
    }
    for byte in 0u8..=127 {
        if byte.is_ascii_alphanumeric() || b".-_~".contains(&byte) {
            continue;
        }
        if byte == b'/' {
            continue;
        }
        let mut bundle = manifest();
        bundle.files[0].path = format!("a{}b", char::from(byte));
        bundle.files.sort_by(|a, b| a.path.cmp(&b.path));
        assert!(bundle.validate().is_err(), "admitted byte {byte}");
    }
}

#[test]
fn path_lengths_are_bounded_at_segment_and_full_path_limits() {
    let mut bundle = manifest();
    let path = format!(
        "{}/{}/{}/{}/a",
        "a".repeat(255),
        "b".repeat(255),
        "c".repeat(255),
        "d".repeat(254)
    );
    assert_eq!(path.len(), MAX_BUNDLE_PATH_BYTES);
    bundle.files[0].path = path;
    bundle.validate().unwrap();
    bundle.files[0].path.push('a');
    assert!(bundle.validate().is_err());
    bundle.files[0].path = "a".repeat(255);
    bundle.validate().unwrap();
    bundle.files[0].path.push('a');
    assert!(bundle.validate().is_err());
}

#[test]
fn sources_require_absolute_admitted_uris_and_canonical_matching_hashes() {
    for source in [
        "app.js",
        "./app.js",
        "/app.js",
        "//example.com/app.js",
        "data:text/javascript,x",
        "file:///tmp/app.js",
        "https:example.com/app.js",
        "https:///example.com/app.js",
        "http://example.com/app.js",
        "http://localhost.evil.com/app.js",
        "http://127.0.0.1:80@evil.com/app.js",
        "http://[::2]/app.js",
        "http://[::ffff:127.0.0.1]/app.js",
        "https://u:p@example.com/app.js",
        "https://example.com/%2e%2e/app.js",
        "https://example.com/a%2fb",
    ] {
        let mut bundle = manifest();
        bundle.files[0].source_uri = source.into();
        assert!(bundle.validate().is_err(), "admitted {source}");
    }
    for source in [
        "https://example.com/app.js?v=1#view",
        "HTTPS://EXAMPLE.COM/app.js",
        "http://localhost:8080/app.js",
        "http://127.0.0.2/app.js",
        "http://[::1]/app.js",
    ] {
        let mut bundle = manifest();
        bundle.files[0].source_uri = source.into();
        bundle.validate().unwrap();
    }
    for hash in [
        "".into(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "g".repeat(64),
    ] {
        let mut bundle = manifest();
        bundle.files[0].source_uri = "https://example.com/app.js".into();
        bundle.files[0].integrity = Hash::new_unchecked(hash);
        assert!(bundle.validate().is_err());
    }
    let mut bundle = manifest();
    let original = bundle.files[0].integrity.to_string();
    for offset in 0..64 {
        let mut hash = original.as_bytes().to_vec();
        hash[offset] = if hash[offset] == b'0' { b'1' } else { b'0' };
        bundle.files[0].integrity = Hash::new_unchecked(String::from_utf8(hash).unwrap());
        assert!(
            bundle.validate().is_err(),
            "admitted mismatched digest at {offset}"
        );
    }
}

#[test]
fn mime_kind_pairs_are_explicit_and_canonical() {
    let allowed = [
        (BundleFileKind::Document, "text/html"),
        (BundleFileKind::Script, "text/javascript"),
        (BundleFileKind::Script, "application/javascript"),
        (BundleFileKind::Stylesheet, "text/css"),
        (BundleFileKind::Wasm, "application/wasm"),
        (BundleFileKind::Asset, "application/json"),
        (BundleFileKind::Asset, "image/png"),
        (BundleFileKind::Asset, "font/woff2"),
    ];
    let kinds = [
        BundleFileKind::Document,
        BundleFileKind::Script,
        BundleFileKind::Stylesheet,
        BundleFileKind::Wasm,
        BundleFileKind::Asset,
    ];
    for (expected, mime) in allowed {
        for kind in kinds {
            let mut bundle = manifest();
            bundle.files[0].kind = kind;
            bundle.files[0].media_type = mime.into();
            assert_eq!(
                bundle.validate().is_ok(),
                kind == expected,
                "{kind:?} {mime}"
            );
        }
        for noncanonical in [
            mime.to_uppercase(),
            format!("{mime};charset=utf-8"),
            format!(" {mime}"),
            format!("{mime}\r\nX: y"),
        ] {
            let mut bundle = manifest();
            bundle.files[0].kind = expected;
            bundle.files[0].media_type = noncanonical;
            assert!(bundle.validate().is_err());
        }
    }
    for mime in [
        "image/unknown",
        "application/pdf",
        "text/xml",
        "text/ecmascript",
        "",
        "*/*",
    ] {
        let mut bundle = manifest();
        bundle.files[0].kind = BundleFileKind::Asset;
        bundle.files[0].media_type = mime.into();
        assert!(bundle.validate().is_err(), "admitted {mime}");
    }
}

#[test]
fn inventory_requires_sorted_unique_paths_and_compatible_hash_metadata() {
    let mut bundle = manifest();
    bundle.files.reverse();
    assert!(bundle.validate().is_err());
    let mut bundle = manifest();
    bundle.files.insert(0, bundle.files[0].clone());
    assert!(bundle.validate().is_err());
    let mut bundle = manifest();
    let mut alias = bundle.files[0].clone();
    alias.path = "copy.js".into();
    alias.source_uri = "https://example.com/copy.js".into();
    bundle.files.insert(1, alias);
    bundle.validate().unwrap();
    let mut conflict = bundle.clone();
    conflict.files[1].size_bytes += 1;
    assert!(conflict.validate().is_err());
    let mut conflict = bundle.clone();
    conflict.files[1].media_type = "application/javascript".into();
    assert!(conflict.validate().is_err());
    let mut conflict = bundle;
    conflict.files[1].kind = BundleFileKind::Asset;
    conflict.files[1].media_type = "text/plain".into();
    assert!(conflict.validate().is_err());
}

#[test]
fn file_count_size_and_aggregate_limits_include_aliases_and_prevent_overflow() {
    let mut bundle = manifest();
    bundle.files = (0..MAX_BUNDLE_FILES - 1)
        .map(|n| {
            file(
                &format!("asset-{n:03}"),
                BundleFileKind::Asset,
                "text/plain",
            )
        })
        .chain([bundle.files[1].clone()])
        .collect();
    bundle.validate().unwrap();
    bundle
        .files
        .insert(0, file("aaa", BundleFileKind::Asset, "text/plain"));
    assert!(bundle.validate().is_err());
    bundle.files.clear();
    assert!(bundle.validate().is_err());

    let mut bundle = manifest();
    bundle.files[0].size_bytes = 0;
    bundle.validate().unwrap();
    bundle.files[0].size_bytes = MAX_BUNDLE_FILE_BYTES;
    bundle.validate().unwrap();
    for size in [MAX_BUNDLE_FILE_BYTES + 1, u64::MAX] {
        bundle.files[0].size_bytes = size;
        assert!(bundle.validate().is_err());
    }
    let entry = bundle.files[1].clone();
    bundle.files = (0..4)
        .map(|n| BundleFile {
            path: format!("entry-{n}"),
            size_bytes: MAX_BUNDLE_FILE_BYTES,
            ..entry.clone()
        })
        .collect();
    bundle.entry_path = bundle.files[0].path.clone();
    assert_eq!(
        bundle.files.iter().map(|file| file.size_bytes).sum::<u64>(),
        MAX_BUNDLE_BYTES
    );
    bundle.validate().unwrap();
    bundle
        .files
        .push(file("z", BundleFileKind::Asset, "text/plain"));
    assert!(bundle.validate().is_err());
}

#[test]
fn canonical_manifest_limit_is_measured_in_canonical_bytes() {
    let mut bundle = manifest();
    bundle.files[0].source_uri = "https://example.com/".into();
    let remaining = MAX_BUNDLE_MANIFEST_BYTES - bundle.canonical_bytes().unwrap().len();
    bundle.files[0].source_uri.push_str(&"a".repeat(remaining));
    assert_eq!(
        bundle.canonical_bytes().unwrap().len(),
        MAX_BUNDLE_MANIFEST_BYTES
    );
    bundle.validate().unwrap();
    bundle.files[0].source_uri.push('a');
    assert!(bundle.validate().is_err());
    bundle.files[0].source_uri = "a".repeat(MAX_BUNDLE_MANIFEST_BYTES + 1);
    assert!(bundle.validate().is_err());
}

#[test]
fn surface_profiles_and_entry_mapping_fail_closed() {
    let bundle = manifest();
    for target in [SurfaceTarget::Web, SurfaceTarget::WebGpu] {
        let mut surface = surface(&bundle);
        surface.target = target;
        bundle.validate_surface(&surface).unwrap();
    }
    for target in [
        SurfaceTarget::Static,
        SurfaceTarget::Wasm,
        SurfaceTarget::NativeTrusted,
    ] {
        let mut surface = surface(&bundle);
        surface.target = target;
        assert!(
            bundle
                .validate_surface(&surface)
                .unwrap_err()
                .to_string()
                .contains("unsupported bundle surface profile")
        );
    }
    let mut changed = surface(&bundle);
    changed.integrity = None;
    assert!(bundle.validate_surface(&changed).is_err());
    changed.integrity = Some(bundle.files[0].integrity.clone());
    assert!(bundle.validate_surface(&changed).is_err());
    let mut changed = surface(&bundle);
    changed.entry = format!(
        "https://gateway.example/media/blobs/{}?media_type=text/html",
        bundle.files[1].integrity
    );
    assert!(bundle.validate_surface(&changed).is_err());
    let mut missing = bundle.clone();
    missing.entry_path = "missing.html".into();
    assert!(missing.validate().is_err());
    let mut non_html = bundle;
    non_html.entry_path = "app.js".into();
    assert!(non_html.validate().is_err());
}

#[test]
fn signed_object_commits_every_manifest_field_without_outer_resources() {
    let (author, key) = identity();
    let bundle = manifest();
    let signed = Object::text(&author, "signed bundle")
        .unwrap()
        .with_surfaces(vec![surface(&bundle)])
        .unwrap()
        .sign(&author, &key)
        .unwrap();
    assert!(signed.resources.is_empty());
    signed.verify(&author).unwrap();
    let mut mutations = Vec::new();
    let original = serde_json::to_value(&bundle).unwrap();
    for (field, value) in [
        ("path", json!("another.js")),
        ("source_uri", json!("https://example.com/other.js")),
        ("integrity", json!(Hash::from_bytes(b"changed"))),
        ("size_bytes", json!(20)),
        ("media_type", json!("application/javascript")),
        ("kind", json!("asset")),
    ] {
        let mut changed = original.clone();
        changed["files"][0][field] = value;
        mutations.push(changed);
    }
    let mut changed = original.clone();
    changed["version"] = json!(2);
    mutations.push(changed);
    let mut changed = original;
    changed["entry_path"] = json!("app.js");
    mutations.push(changed);
    for mutation in mutations {
        let changed: BundleManifest = serde_json::from_value(mutation).unwrap();
        assert_ne!(
            bundle.canonical_hash().unwrap(),
            changed.canonical_hash().unwrap()
        );
        let mut tampered = signed.clone();
        tampered.surfaces[0].bundle = Some(changed);
        assert!(tampered.verify(&author).is_err());
    }
    let mut removed = signed;
    removed.surfaces[0].bundle = None;
    assert!(removed.verify(&author).is_err());
}

#[test]
fn absent_bundle_preserves_legacy_canonical_bytes_and_signatures() {
    let (author, key) = identity();
    let legacy = json!({
        "role": "Feed", "target": "Web", "entry": "https://example.com/index.html",
        "integrity": Hash::from_bytes(b"legacy")
    });
    let parsed: Surface = serde_json::from_value(legacy.clone()).unwrap();
    assert!(parsed.bundle.is_none());
    assert_eq!(serde_json::to_value(&parsed).unwrap(), legacy);
    assert_eq!(
        parsed.canonical_bytes().unwrap(),
        legacy.canonical_bytes().unwrap()
    );

    // Construct the old commitment as JSON, independently of Surface's new serializer.
    let object = Object::text(&author, "legacy object").unwrap();
    let mut wire = serde_json::to_value(object).unwrap();
    wire["surfaces"] = json!([legacy]);
    let mut commitment = wire.clone();
    commitment.as_object_mut().unwrap().remove("id");
    commitment.as_object_mut().unwrap().remove("signature");
    wire["id"] = json!(ObjectId::from_hash(&commitment.canonical_hash().unwrap()));
    wire["signature"] = json!(key.sign(&commitment.canonical_bytes().unwrap()));
    let signed: Object = serde_json::from_value(wire.clone()).unwrap();
    signed.verify(&author).unwrap();
    assert_eq!(serde_json::to_value(&signed).unwrap(), wire);
    wire["surfaces"][0]["bundle"] = Value::Null;
    let explicit_null: Object = serde_json::from_value(wire).unwrap();
    explicit_null.verify(&author).unwrap();
    assert_eq!(
        explicit_null.canonical_bytes().unwrap(),
        signed.canonical_bytes().unwrap()
    );
}
