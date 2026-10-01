use babble_object::Resource;
use babble_types::{Hash, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaBlob {
    pub uri: String,
    pub media_type: String,
    pub integrity: Hash,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaObjectPayload {
    pub title: String,
    pub description: Option<String>,
    pub primary_resource: MediaBlob,
    pub resources: Vec<MediaBlob>,
}

impl MediaBlob {
    pub fn from_bytes(media_type: impl Into<String>, bytes: &[u8]) -> Result<Self> {
        let integrity = Hash::from_bytes(bytes);
        Self::from_hash(media_type, integrity, bytes.len() as u64)
    }

    pub fn from_hash(
        media_type: impl Into<String>,
        integrity: Hash,
        size_bytes: u64,
    ) -> Result<Self> {
        integrity.validate()?;
        let media_type = normalize_media_type(media_type.into())?;
        Ok(Self {
            uri: format!("babble://blobs/{}", integrity.as_str()),
            media_type,
            integrity,
            size_bytes,
        })
    }

    pub fn resource(&self) -> Resource {
        Resource {
            uri: self.uri.clone(),
            media_type: self.media_type.clone(),
            integrity: self.integrity.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        let canonical = Self::from_hash(&self.media_type, self.integrity.clone(), self.size_bytes)?;
        if self.size_bytes == 0 || self != &canonical {
            return Err(babble_types::Error::Conflict(
                "media resource metadata must be canonical with a positive size".into(),
            ));
        }
        Ok(())
    }
}

impl MediaObjectPayload {
    pub fn new(
        title: impl Into<String>,
        description: Option<String>,
        resources: Vec<MediaBlob>,
    ) -> Result<Self> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(babble_types::Error::Conflict(
                "media Object title must not be empty".to_string(),
            ));
        }
        if resources.is_empty() {
            return Err(babble_types::Error::Conflict(
                "media Object requires at least one resource".to_string(),
            ));
        }
        let payload = Self {
            title: title.trim().to_string(),
            description: description.and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }),
            primary_resource: resources[0].clone(),
            resources,
        };
        payload.validate()?;
        Ok(payload)
    }

    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty() || self.resources.is_empty() {
            return Err(babble_types::Error::Conflict(
                "media Object requires a title and at least one resource".into(),
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        for resource in &self.resources {
            resource.validate()?;
            if !unique.insert(&resource.integrity) {
                return Err(babble_types::Error::Conflict(format!(
                    "duplicate media resource: {}",
                    resource.integrity
                )));
            }
        }
        if !self.resources.contains(&self.primary_resource) {
            return Err(babble_types::Error::Conflict(
                "media primary_resource must exactly match an album resource".into(),
            ));
        }
        Ok(())
    }

    pub fn validate_object_resources(&self, resources: &[Resource]) -> Result<()> {
        self.validate()?;
        let mut by_hash = std::collections::BTreeMap::new();
        for resource in resources {
            if by_hash.insert(&resource.integrity, resource).is_some() {
                return Err(babble_types::Error::Conflict(format!(
                    "duplicate media Object resource: {}",
                    resource.integrity
                )));
            }
        }
        // Additional resources may support executable surfaces; album members
        // must still have an unambiguous, matching signed delivery descriptor.
        for blob in &self.resources {
            if by_hash.get(&blob.integrity).copied() != Some(&blob.resource()) {
                return Err(babble_types::Error::Conflict(format!(
                    "media payload does not match Object resource: {}",
                    blob.integrity
                )));
            }
        }
        Ok(())
    }

    pub fn object_resources(&self) -> Vec<Resource> {
        self.resources.iter().map(MediaBlob::resource).collect()
    }
}

pub fn normalize_media_type(value: String) -> Result<String> {
    let normalized = value.trim().to_ascii_lowercase();
    let Some((kind, subtype)) = normalized.split_once('/') else {
        return Err(invalid_media_type(value));
    };
    let valid = token(kind) && token(subtype);
    if valid {
        Ok(normalized)
    } else {
        Err(invalid_media_type(value))
    }
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

fn invalid_media_type(value: String) -> babble_types::Error {
    babble_types::Error::Conflict(format!("invalid media type: {value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_blob_is_content_addressed_and_resource_compatible() {
        let blob = MediaBlob::from_bytes("Image/PNG", b"png bytes").unwrap();

        assert_eq!(blob.media_type, "image/png");
        assert_eq!(blob.size_bytes, 9);
        assert_eq!(blob.uri, format!("babble://blobs/{}", blob.integrity));
        assert_eq!(blob.resource().integrity, blob.integrity);
    }

    #[test]
    fn media_payload_rejects_empty_or_duplicate_resources() {
        let blob = MediaBlob::from_bytes("text/plain", b"hello").unwrap();

        assert!(MediaObjectPayload::new("", None, vec![blob.clone()]).is_err());
        assert!(MediaObjectPayload::new("title", None, Vec::new()).is_err());
        assert!(MediaObjectPayload::new("title", None, vec![blob.clone(), blob]).is_err());
    }

    #[test]
    fn media_blob_from_hash_validates_descriptor_metadata() {
        let hash = Hash::from_bytes(b"external blob");
        let blob = MediaBlob::from_hash("Image/JPEG", hash, 13).unwrap();

        assert_eq!(blob.media_type, "image/jpeg");
        assert!(MediaBlob::from_hash("not-a-media-type", blob.integrity, 13).is_err());
    }

    #[test]
    fn media_album_preserves_all_permutations_and_explicit_primary_without_a_count_limit() {
        let blobs: Vec<_> = (0..13)
            .map(|i| MediaBlob::from_bytes("image/png", &[i]).unwrap())
            .collect();
        for reverse in [false, true] {
            for offset in 0..blobs.len() {
                let mut ordered = blobs.clone();
                ordered.rotate_left(offset);
                if reverse {
                    ordered.reverse();
                }
                let mut payload =
                    MediaObjectPayload::new(" Album ", Some(" caption ".into()), ordered.clone())
                        .unwrap();
                assert_eq!(payload.resources, ordered);
                assert_eq!(payload.primary_resource, ordered[0]);
                assert_eq!(
                    payload.object_resources(),
                    ordered.iter().map(MediaBlob::resource).collect::<Vec<_>>()
                );
                for primary in &ordered {
                    payload.primary_resource = primary.clone();
                    payload
                        .validate_object_resources(&payload.object_resources())
                        .unwrap();
                }
            }
        }
    }

    #[test]
    fn media_album_rejects_noncanonical_descriptors_and_conflicting_primary() {
        let blob = MediaBlob::from_bytes("image/png", b"image").unwrap();
        for field in ["uri", "mime", "size"] {
            let mut invalid = blob.clone();
            match field {
                "uri" => invalid.uri = "https://example.test/image".into(),
                "mime" => invalid.media_type = "Image/PNG".into(),
                "size" => invalid.size_bytes = 0,
                _ => unreachable!(),
            }
            assert!(MediaObjectPayload::new("Album", None, vec![invalid]).is_err());
        }
        let mut payload = MediaObjectPayload::new("Album", None, vec![blob.clone()]).unwrap();
        payload.primary_resource.size_bytes += 1;
        assert!(payload.validate().is_err());
        payload.primary_resource = blob.clone();
        let mut duplicate = blob;
        duplicate.media_type = "audio/wav".into();
        payload.resources.push(duplicate);
        assert!(payload.validate().is_err());
    }
}
