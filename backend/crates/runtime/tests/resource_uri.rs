use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_object::{Object, Resource, Surface, SurfaceRole, SurfaceTarget};
use babel_runtime::{RuntimeAdmissionStatus, SurfaceRuntime};
use babel_types::Hash;

#[test]
fn resource_uri_runtime_admits_exact_references_and_seed_gateway_urls() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "uri-test", &keypair).unwrap();
    let hash = Hash::from_bytes(b"surface");
    let blob = format!("babel://blobs/{hash}");
    let cases = [
        (blob.clone(), blob.clone()),
        (
            blob.clone(),
            format!("http://localhost:3000/runtime/surfaces/blobs/{hash}?media_type=text/html"),
        ),
        (
            blob.clone(),
            format!("https://gateway.example/runtime/surfaces/blobs/{hash}?media_type=text%2Fhtml"),
        ),
        (
            blob.clone(),
            format!("http://[::1]:8080/media/blobs/{hash}?media_type=text/html"),
        ),
        ("assets/index.html".into(), "assets/index.html".into()),
        (
            "https://cdn.example/index.html?v=1#view".into(),
            "https://cdn.example/index.html?v=1#view".into(),
        ),
    ];
    for (uri, entry) in cases {
        let object = Object::text(&identity, "surface")
            .unwrap()
            .with_resources(vec![Resource {
                uri,
                media_type: "text/html".into(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: entry.clone(),
                integrity: Some(hash.clone()),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(
            plan.admission,
            RuntimeAdmissionStatus::Ready,
            "{entry}: {:?}",
            plan.blocked_reasons
        );
        assert_eq!(plan.surface.entry, entry);
    }
}

#[test]
fn resource_uri_runtime_blocks_hash_decoys_credentials_and_traversal() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "uri-test", &keypair).unwrap();
    let hash = Hash::from_bytes(b"surface");
    let blob = format!("babel://blobs/{hash}");
    for entry in [
        format!("https://{hash}.example.com/evil.js"),
        format!("https://example.com/evil.js?hash={hash}"),
        format!("https://example.com/evil.js#{hash}"),
        format!("https://example.com/{hash}.js"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}/evil.js"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}?media_type=text/plain"),
        format!(
            "https://example.com/runtime/surfaces/blobs/{hash}?media_type=text/html&media_type=text/html"
        ),
        format!("http://localhost:80@evil.com/runtime/surfaces/blobs/{hash}"),
        format!("https://user@gateway.example/runtime/surfaces/blobs/{hash}"),
        format!("https://gateway.example/a/../runtime/surfaces/blobs/{hash}"),
        format!("https://gateway.example/%2e%2e/runtime/surfaces/blobs/{hash}"),
        format!("https://gateway.example/%252e%252e/runtime/surfaces/blobs/{hash}"),
        format!("{blob}\n"),
        format!(" {blob}"),
        format!("{blob} "),
        format!("https://gateway.example/runtime/surfaces/blobs/{hash}"),
        format!("https://gateway.example/media/blobs/{hash}"),
    ] {
        let object = Object::text(&identity, "surface")
            .unwrap()
            .with_resources(vec![Resource {
                uri: blob.clone(),
                media_type: "text/html".into(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: entry.clone(),
                integrity: Some(hash.clone()),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked, "{entry}");
        assert!(!plan.blocked_reasons.is_empty());
    }
}

#[test]
fn resource_uri_runtime_checks_exact_matches_and_static_entries_too() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "uri-test", &keypair).unwrap();
    let hash = Hash::from_bytes(b"surface");
    let wrong = Hash::from_bytes(b"other");
    for entry in [
        "javascript:alert(1)".into(),
        "a/%2e%2e/index.html".into(),
        "http://localhost:80@evil.com/index.html".into(),
        " index.html".into(),
        format!("babel://blobs/{wrong}"),
        format!("babel://blobs/{}", "z".repeat(64)),
    ] {
        for target in [
            SurfaceTarget::Static,
            SurfaceTarget::Web,
            SurfaceTarget::Wasm,
        ] {
            let media_type = if target == SurfaceTarget::Wasm {
                "application/wasm"
            } else {
                "text/html"
            };
            let object = Object::text(&identity, "surface")
                .unwrap()
                .with_resources(vec![Resource {
                    uri: entry.clone(),
                    media_type: media_type.into(),
                    integrity: hash.clone(),
                }])
                .unwrap()
                .with_surfaces(vec![Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: target.clone(),
                    entry: entry.clone(),
                    integrity: Some(hash.clone()),
                }])
                .unwrap()
                .sign(&identity, &keypair)
                .unwrap();
            let plan = SurfaceRuntime::babel_default()
                .prepare_surface(&object, SurfaceRole::Feed, &[])
                .unwrap();
            assert_eq!(
                plan.admission,
                RuntimeAdmissionStatus::Blocked,
                "{target:?} {entry}"
            );
        }
    }
}

#[test]
fn resource_uri_runtime_alias_admission_is_independent_of_resource_order() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "uri-alias-test", &keypair).unwrap();
    let hash = Hash::from_bytes(b"surface");
    let blob = format!("babel://blobs/{hash}");
    for (target, media_type) in [
        (SurfaceTarget::Web, "text/html"),
        (SurfaceTarget::Wasm, "application/wasm"),
    ] {
        for (uri, entry) in [
            (
                "https://cdn.example/surface".to_string(),
                "https://cdn.example/surface".to_string(),
            ),
            ("surface.js".to_string(), "surface.js".to_string()),
            (blob.clone(), blob.clone()),
            (
                blob.clone(),
                format!(
                    "https://gateway.example/runtime/surfaces/blobs/{hash}?media_type={media_type}"
                ),
            ),
        ] {
            let resources = [
                Resource {
                    uri: "https://other.example/surface".into(),
                    media_type: media_type.into(),
                    integrity: hash.clone(),
                },
                Resource {
                    uri: uri.clone(),
                    media_type: "image/png".into(),
                    integrity: hash.clone(),
                },
                Resource {
                    uri,
                    media_type: media_type.into(),
                    integrity: hash.clone(),
                },
            ];
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                let object = Object::text(&identity, "surface")
                    .unwrap()
                    .with_resources(order.map(|index| resources[index].clone()).to_vec())
                    .unwrap()
                    .with_surfaces(vec![Surface {
                        bundle: None,
                        role: SurfaceRole::Feed,
                        target: target.clone(),
                        entry: entry.clone(),
                        integrity: Some(hash.clone()),
                    }])
                    .unwrap()
                    .sign(&identity, &keypair)
                    .unwrap();
                let plan = SurfaceRuntime::babel_default()
                    .prepare_surface(&object, SurfaceRole::Feed, &[])
                    .unwrap();
                assert_eq!(
                    plan.admission,
                    RuntimeAdmissionStatus::Ready,
                    "{target:?} {entry} {order:?}: {:?}",
                    plan.blocked_reasons
                );
            }
        }
    }
}

#[test]
fn resource_uri_runtime_cannot_combine_uri_hash_and_mime_from_different_candidates() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "uri-alias-test", &keypair).unwrap();
    let hash = Hash::from_bytes(b"surface");
    for (target, media_type) in [
        (SurfaceTarget::Web, "text/html"),
        (SurfaceTarget::Wasm, "application/wasm"),
    ] {
        let resources = [
            Resource {
                uri: "surface.js".into(),
                media_type: "image/png".into(),
                integrity: hash.clone(),
            },
            Resource {
                uri: "other.js".into(),
                media_type: media_type.into(),
                integrity: hash.clone(),
            },
            Resource {
                uri: "surface.js".into(),
                media_type: media_type.into(),
                integrity: Hash::from_bytes(b"wrong bytes"),
            },
        ];
        for order in [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ] {
            let object = Object::text(&identity, "surface")
                .unwrap()
                .with_resources(order.map(|index| resources[index].clone()).to_vec())
                .unwrap()
                .with_surfaces(vec![Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: target.clone(),
                    entry: "surface.js".into(),
                    integrity: Some(hash.clone()),
                }])
                .unwrap()
                .sign(&identity, &keypair)
                .unwrap();
            let plan = SurfaceRuntime::babel_default()
                .prepare_surface(&object, SurfaceRole::Feed, &[])
                .unwrap();
            assert_eq!(
                plan.admission,
                RuntimeAdmissionStatus::Blocked,
                "{target:?} {order:?}"
            );
            assert!(
                plan.blocked_reasons
                    .iter()
                    .any(|reason| reason.contains("media type"))
            );
        }
    }
}
