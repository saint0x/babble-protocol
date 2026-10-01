use babel_object::{Resource, resource_uri::ResourceUri};
use babel_types::Hash;

fn blob_resource() -> Resource {
    let integrity = Hash::from_bytes(b"surface bytes");
    Resource {
        uri: format!("babel://blobs/{integrity}"),
        media_type: "text/html".into(),
        integrity,
    }
}

#[test]
fn resource_uri_accepts_supported_references_without_rewriting_signed_text() {
    for entry in [
        "index.html",
        "assets/surface.wasm",
        "assets/my%20surface.js",
        "assets/release..js",
        "assets/%E2%9C%93.js",
        "index.html?v=1#view",
        "https://example.com",
        "https://example.com/",
        "https://example.com/a//b/",
        "https://example.com:8443/app.js?version=1&token=a%2Fb#view",
        "HTTPS://EXAMPLE.COM/app.js",
        "https://example.com/a:b",
        "http://localhost:3000/index.html",
        "http://127.0.0.1:8080/index.html",
        "http://127.0.0.2/app.js",
        "http://[::1]:3000/index.html",
    ] {
        let parsed = ResourceUri::parse(entry).unwrap_or_else(|err| panic!("{entry}: {err}"));
        let resource = Resource {
            uri: entry.into(),
            ..blob_resource()
        };
        assert!(parsed.matches_resource(&resource), "{entry}");
    }
}

#[test]
fn resource_uri_rejects_unsafe_and_parser_normalized_references() {
    for entry in [
        "",
        " index.html",
        "index.html\n",
        "index\t.html",
        "index\u{7f}.html",
        "index\u{85}.html",
        "a\\b.js",
        "//example.com/a",
        "/root.js",
        "?v=1",
        "#view",
        "./index.html",
        "a/../index.html",
        "a//b",
        "a/",
        "../a",
        "a/%2e%2e/b",
        "a/.%2E/b",
        "a/%252e%252e/b",
        "a/%2f../b",
        "a/%5cb",
        "a/%3f/b",
        "a/%23/b",
        "a/%00b",
        "a/%0ab",
        "a/%7fb",
        "a/%",
        "a/%xz",
        "javascript:alert(1)",
        "data:text/html,hello",
        "file:///tmp/a",
        "ftp://example.com/a",
        "https:example.com/a",
        "https:///example.com/a",
        "https://",
        "https://?a",
        "https://user@example.com/a",
        "https://:pass@example.com/a",
        "https://@example.com/a",
        "https://example.com:wrong/a",
        "https://example.com:65536/a",
        "https://example.com/a/../b",
        "https://example.com/%2e%2e/b",
        "https://example.com/%252e%252e/b",
        "https://example.com/a%2fb",
        "https://example.com\\@evil.com/a",
        "https://example.com/a?x=%0d%0a",
        "http://example.com/a",
        "http://localhost.evil.com/a",
        "http://localhost:80@evil.com/a",
        "http://127.0.0.1:80@evil.com/a",
        "http://[::1]:80@evil.com/a",
        "http://[::2]/a",
    ] {
        assert!(ResourceUri::parse(entry).is_err(), "admitted {entry:?}");
    }
}

#[test]
fn resource_uri_requires_canonical_blob_address_and_matching_integrity() {
    let resource = blob_resource();
    let hash = resource.integrity.as_str();
    assert!(
        ResourceUri::parse(&resource.uri)
            .unwrap()
            .matches_resource(&resource)
    );
    for entry in [
        format!("babel://blobs/{hash}?v=1"),
        format!("babel://blobs/{hash}#view"),
        format!("babel://blobs/{hash}/"),
        format!("babel://blobs/{hash}/a"),
        format!("babel://blobs/{hash}.js"),
        format!("babel://blobs:80/{hash}"),
        format!("babel://user@blobs/{hash}"),
        format!("babel://blobs/{hash}@evil.com"),
        format!("BABEL://blobs/{hash}"),
        format!("babel://BLOBS/{hash}"),
        format!("babel://blobs/{}", hash.to_uppercase()),
        format!("babel://blobs/{}", "z".repeat(64)),
    ] {
        assert!(ResourceUri::parse(&entry).is_err(), "{entry}");
    }
    let wrong = Resource {
        integrity: Hash::from_bytes(b"other"),
        ..resource.clone()
    };
    assert!(
        !ResourceUri::parse(&resource.uri)
            .unwrap()
            .matches_resource(&wrong)
    );
    let invalid_hash = Resource {
        uri: "index.html".into(),
        integrity: Hash::new_unchecked("z".repeat(64)),
        ..resource
    };
    assert!(
        !ResourceUri::parse("index.html")
            .unwrap()
            .matches_resource(&invalid_hash)
    );
}

#[test]
fn resource_uri_gateway_aliases_match_complete_path_and_declared_media_type() {
    let resource = blob_resource();
    let hash = resource.integrity.as_str();
    for origin in [
        "https://gateway.example",
        "http://localhost:3000",
        "http://127.0.0.1:8080",
        "http://[::1]:3000",
    ] {
        for route in ["/runtime/surfaces/blobs/", "/media/blobs/"] {
            for query in ["?media_type=text/html", "?media_type=text%2Fhtml"] {
                let entry = format!("{origin}{route}{hash}{query}");
                assert!(
                    ResourceUri::parse(&entry)
                        .unwrap()
                        .matches_resource(&resource),
                    "{entry}"
                );
            }
        }
    }
}

#[test]
fn resource_uri_hash_decoys_do_not_establish_blob_identity() {
    let resource = blob_resource();
    let hash = resource.integrity.as_str();
    for entry in [
        format!("https://{hash}.example.com/evil.js"),
        format!("https://example.com/evil.js?hash={hash}"),
        format!("https://example.com/evil.js#{hash}"),
        format!("https://example.com/{hash}.js"),
        format!("https://example.com/{hash}/evil.js"),
        format!("https://example.com/arbitrary/{hash}"),
        format!("https://example.com/prefix/runtime/surfaces/blobs/{hash}"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}/evil.js"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}.js"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}?redirect=evil"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}?media_type=text/plain"),
        format!(
            "https://example.com/runtime/surfaces/blobs/{hash}?media_type=text/html&media_type=text/html"
        ),
        format!(
            "https://example.com/runtime/surfaces/blobs/{hash}?media_type=text/html&redirect=evil"
        ),
        format!("https://example.com/runtime/surfaces/blobs/{hash}#view"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}#"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}?"),
        format!("https://example.com/runtime/surfaces/blobs/{hash}"),
        format!("https://example.com/media/blobs/{hash}"),
        format!("assets/{hash}.js"),
        format!("runtime/surfaces/blobs/{hash}"),
    ] {
        assert!(
            !ResourceUri::parse(&entry)
                .unwrap()
                .matches_resource(&resource),
            "{entry}"
        );
    }
}

#[test]
fn resource_uri_external_declarations_require_exact_reference() {
    let hash = Hash::from_bytes(b"surface bytes");
    let resource = Resource {
        uri: format!("https://signed.example/app/{hash}.js?v=1#view"),
        integrity: hash.clone(),
        media_type: "text/javascript".into(),
    };
    assert!(
        ResourceUri::parse(&resource.uri)
            .unwrap()
            .matches_resource(&resource)
    );
    for entry in [
        format!("https://other.example/app/{hash}.js?v=1#view"),
        format!("https://signed.example/app/{hash}.js?v=2#view"),
        format!("https://signed.example/app/{hash}.js?v=1#other"),
        format!("https://signed.example/runtime/surfaces/blobs/{hash}"),
        format!("babel://blobs/{hash}"),
    ] {
        assert!(
            !ResourceUri::parse(&entry)
                .unwrap()
                .matches_resource(&resource),
            "{entry}"
        );
    }
}

#[test]
fn resource_uri_mutations_cannot_change_the_admitted_blob_hash() {
    let resource = blob_resource();
    let hash = resource.integrity.as_str();
    for offset in 0..hash.len() {
        for replacement in b"0123456789abcdef" {
            if *replacement == hash.as_bytes()[offset] {
                continue;
            }
            let mut mutated = hash.as_bytes().to_vec();
            mutated[offset] = *replacement;
            let mutated = String::from_utf8(mutated).unwrap();
            for entry in [
                format!("babel://blobs/{mutated}"),
                format!(
                    "https://gateway.example/runtime/surfaces/blobs/{mutated}?media_type=text/html"
                ),
                format!("https://gateway.example/media/blobs/{mutated}?media_type=text/html"),
            ] {
                assert!(
                    !ResourceUri::parse(&entry)
                        .unwrap()
                        .matches_resource(&resource),
                    "{entry}"
                );
            }
        }
    }
}

#[test]
fn resource_uri_rejects_controls_at_every_url_boundary() {
    let resource = blob_resource();
    let hash = resource.integrity.as_str();
    for byte in (0u8..=31).chain([127]) {
        for control in [
            char::from(byte).to_string(),
            format!("%{byte:02x}"),
            format!("%{byte:02X}"),
        ] {
            for entry in [
                format!("{control}https://gateway.example/index.html"),
                format!("https://gate{control}way.example/index.html"),
                format!("https://gateway.example/in{control}dex.html"),
                format!("https://gateway.example/index.html?x={control}"),
                format!(
                    "https://gateway.example/runtime/surfaces/blobs/{hash}?media_type=text/html{control}"
                ),
                format!("assets/in{control}dex.html"),
            ] {
                assert!(ResourceUri::parse(&entry).is_err(), "{entry:?}");
            }
        }
    }
}
