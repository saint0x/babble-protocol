use babble_authoring::ObjectDraft;
use babble_object::{Resource, Surface, SurfaceRole, SurfaceTarget};
use babble_types::Hash;

#[test]
fn resource_uri_authoring_rejects_unsafe_drafts_and_deserialized_mutations() {
    let hash = Hash::from_bytes(b"surface");
    for entry in [
        "javascript:alert(1)".into(),
        "a/%2E%2E/index.html".into(),
        "a/%252e%252e/b".into(),
        "https://user@cdn.example/index.html".into(),
        "http://localhost:80@evil.com/a".into(),
        "https://cdn.example/a/../index.html".into(),
        "index.html\n".into(),
        "a\\b.html".into(),
        "a/%00.html".into(),
        format!("babble://blobs/{hash}?v=1"),
        format!("babble://blobs/{}", Hash::from_bytes(b"different")),
        format!("babble://blobs/{}", "z".repeat(64)),
    ] {
        let resource = Resource {
            uri: entry.clone(),
            media_type: "text/html".into(),
            integrity: hash.clone(),
        };
        let surface = Surface {
            bundle: None,
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: entry.clone(),
            integrity: Some(hash.clone()),
        };
        assert!(
            ObjectDraft::text("surface")
                .unwrap()
                .with_resource(resource.clone())
                .is_err(),
            "resource {entry}"
        );
        assert!(
            ObjectDraft::text("surface")
                .unwrap()
                .with_surface(surface.clone())
                .is_err(),
            "surface {entry}"
        );
        let mut draft = ObjectDraft::text("surface").unwrap();
        draft.resources.push(resource);
        assert!(draft.validate().is_err(), "mutated resource {entry}");
        draft.resources.clear();
        draft.surfaces.push(surface);
        let decoded: ObjectDraft =
            serde_json::from_value(serde_json::to_value(draft).unwrap()).unwrap();
        assert!(decoded.validate().is_err(), "decoded surface {entry}");
    }
}

#[test]
fn resource_uri_authoring_preserves_safe_entries_and_incremental_builder_order() {
    let hash = Hash::from_bytes(b"surface");
    for entry in [
        "assets/index.html".into(),
        "assets/my%20surface.html".into(),
        "https://cdn.example/index.html?v=1#view".into(),
        "http://[::1]:3000/index.html".into(),
        format!("babble://blobs/{hash}"),
        format!("http://localhost:3000/runtime/surfaces/blobs/{hash}?media_type=text/html"),
    ] {
        let surface = Surface {
            bundle: None,
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: entry.clone(),
            integrity: Some(hash.clone()),
        };
        let resource = Resource {
            uri: entry.clone(),
            media_type: "text/html".into(),
            integrity: hash.clone(),
        };
        let draft = ObjectDraft::text("surface")
            .unwrap()
            .with_surface(surface)
            .unwrap()
            .with_resource(resource)
            .unwrap();
        assert!(draft.validate().is_ok(), "{entry}");
        assert_eq!(draft.surfaces[0].entry, entry);
    }
}

#[test]
fn resource_uri_authoring_rejects_duplicate_hash_aliases_in_either_order() {
    let hash = Hash::from_bytes(b"surface");
    let primary = Resource {
        uri: "surface.js".into(),
        media_type: "text/javascript".into(),
        integrity: hash.clone(),
    };
    for alias in [
        Resource {
            uri: "https://cdn.example/surface.js".into(),
            ..primary.clone()
        },
        Resource {
            media_type: "text/html".into(),
            ..primary.clone()
        },
        primary.clone(),
    ] {
        for resources in [
            vec![primary.clone(), alias.clone()],
            vec![alias.clone(), primary.clone()],
        ] {
            let result = ObjectDraft::text("surface")
                .unwrap()
                .with_resources(resources.clone());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("duplicate draft resource")
            );
            let result = ObjectDraft::text("surface")
                .unwrap()
                .with_resource(resources[0].clone())
                .unwrap()
                .with_resource(resources[1].clone());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("duplicate draft resource")
            );
            let mut draft = ObjectDraft::text("surface").unwrap();
            draft.resources = resources;
            let decoded: ObjectDraft =
                serde_json::from_value(serde_json::to_value(draft).unwrap()).unwrap();
            assert!(
                decoded
                    .validate()
                    .unwrap_err()
                    .to_string()
                    .contains("duplicate draft resource")
            );
        }
    }
}

#[test]
fn resource_uri_authoring_accepts_existing_protocol_fixture() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/protocol/v1/fixtures.json"
    ))
    .unwrap();
    let draft: ObjectDraft =
        serde_json::from_value(fixtures["media_object_draft"].clone()).unwrap();
    draft.validate().unwrap();
    let keypair = babble_crypto::Keypair::generate();
    let identity = babble_identity::Identity::create(
        babble_identity::IdentityKind::Person,
        "fixture-test",
        &keypair,
    )
    .unwrap();
    let object = draft.build_unsigned(&identity).unwrap();
    assert_eq!(object.resources, draft.resources);
    assert_eq!(draft.required_blob_hashes().len(), 1);
    for resource in &object.resources {
        assert!(
            babble_object::resource_uri::ResourceUri::parse(&resource.uri)
                .unwrap()
                .matches_resource(resource)
        );
    }
}
