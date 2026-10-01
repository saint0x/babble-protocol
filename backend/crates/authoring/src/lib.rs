use babble_capabilities::GrantDecision;
use babble_crypto::Keypair;
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_identity::Identity;
use babble_media::{MediaBlob, MediaObjectPayload, normalize_media_type};
use babble_object::resource_uri::ResourceUri;
use babble_object::{
    CapabilityRequest, Object, ObjectKind, Provenance, Resource, SchemaRegistry, Surface,
    SurfaceTarget, TextPayload, validate_capability_request,
};
use babble_types::{Hash, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectDraft {
    pub kind: ObjectKind,
    pub schema: String,
    pub payload: Value,
    pub surfaces: Vec<Surface>,
    pub resources: Vec<Resource>,
    pub capabilities: Vec<CapabilityRequest>,
    pub state: Option<Value>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EdgeDraft {
    pub source: ObjectId,
    pub target: ObjectId,
    pub relation: Relation,
    pub origin: EdgeOrigin,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityGrantDraft {
    pub object_id: ObjectId,
    pub request: CapabilityRequest,
    pub decision: GrantDecision,
    pub expires_at: Option<Timestamp>,
}

impl ObjectDraft {
    pub fn new(kind: ObjectKind, schema: impl Into<String>, payload: Value) -> Result<Self> {
        let draft = Self {
            kind,
            schema: schema.into(),
            payload,
            surfaces: Vec::new(),
            resources: Vec::new(),
            capabilities: Vec::new(),
            state: None,
            provenance: Provenance {
                parent: None,
                forked_from: None,
                remixed_from: Vec::new(),
            },
        };
        draft.validate()?;
        Ok(draft)
    }

    pub fn text(text: impl Into<String>) -> Result<Self> {
        let text = text.into();
        let text = text.trim();
        if text.is_empty() {
            return Err(babble_types::Error::Conflict(
                "text draft must not be empty".to_string(),
            ));
        }
        Self::new(
            ObjectKind::text(),
            "babble.schema.text.v1",
            serde_json::to_value(TextPayload {
                text: text.to_string(),
                metadata: BTreeMap::new(),
            })
            .map_err(|err| babble_types::Error::Canonical(err.to_string()))?,
        )
    }

    pub fn media(
        title: impl Into<String>,
        description: Option<String>,
        resources: Vec<MediaBlob>,
    ) -> Result<Self> {
        let payload = MediaObjectPayload::new(title, description, resources)?;
        let object_resources = payload.object_resources();
        Self::new(
            ObjectKind::new("babble.media"),
            "babble.schema.media.v1",
            serde_json::to_value(&payload)
                .map_err(|err| babble_types::Error::Canonical(err.to_string()))?,
        )?
        .with_resources(object_resources)
    }

    pub fn with_resource(mut self, resource: Resource) -> Result<Self> {
        validate_resource(&resource)?;
        self.resources.push(resource);
        self.validate()?;
        Ok(self)
    }

    pub fn with_resources(mut self, resources: Vec<Resource>) -> Result<Self> {
        for resource in &resources {
            validate_resource(resource)?;
        }
        self.resources = resources;
        self.validate()?;
        Ok(self)
    }

    pub fn with_surface(mut self, surface: Surface) -> Result<Self> {
        validate_surface(&surface)?;
        self.surfaces.push(surface);
        self.validate()?;
        Ok(self)
    }

    pub fn with_capability(mut self, capability: CapabilityRequest) -> Result<Self> {
        validate_capability_request(&capability)?;
        self.capabilities.push(capability);
        self.validate()?;
        Ok(self)
    }

    pub fn with_state(mut self, state: Value) -> Result<Self> {
        self.state = Some(state);
        self.validate()?;
        Ok(self)
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Result<Self> {
        validate_provenance(&provenance)?;
        self.provenance = provenance;
        self.validate()?;
        Ok(self)
    }

    pub fn required_blob_hashes(&self) -> BTreeSet<Hash> {
        let mut hashes = self
            .resources
            .iter()
            .filter(|resource| resource.uri.starts_with("babble://blobs/"))
            .map(|resource| resource.integrity.clone())
            .collect::<BTreeSet<_>>();
        hashes.extend(
            self.surfaces
                .iter()
                .filter(|surface| surface.entry.starts_with("babble://blobs/"))
                .filter_map(|surface| surface.integrity.clone()),
        );
        hashes.extend(
            self.surfaces
                .iter()
                .filter_map(|surface| surface.bundle.as_ref())
                .flat_map(|bundle| &bundle.files)
                .filter(|file| file.source_uri.starts_with("babble://blobs/"))
                .map(|file| file.integrity.clone()),
        );
        hashes
    }

    pub fn build_unsigned(&self, author: &Identity) -> Result<Object> {
        self.validate()?;
        if self.kind.as_str() == "babble.media" {
            serde_json::from_value::<MediaObjectPayload>(self.payload.clone())
                .map_err(|error| babble_types::Error::Canonical(error.to_string()))?
                .validate_object_resources(&self.resources)?;
        }
        let mut object = Object::create(
            author,
            self.kind.clone(),
            self.schema.clone(),
            self.payload.clone(),
        )?
        .with_resources(self.resources.clone())?
        .with_surfaces(self.surfaces.clone())?
        .with_capabilities(self.capabilities.clone())?
        .with_provenance(self.provenance.clone())?;
        if let Some(state) = &self.state {
            object = object.with_state(state.clone())?;
        }
        Ok(object)
    }

    pub fn validate(&self) -> Result<()> {
        validate_namespaced("Object kind", self.kind.as_str())?;
        validate_namespaced("Object schema", &self.schema)?;
        if self.payload.is_null() {
            return Err(babble_types::Error::Conflict(
                "Object draft payload must not be null".to_string(),
            ));
        }
        SchemaRegistry::babble_core().validate_draft(
            &self.kind,
            &self.schema,
            &self.payload,
            self.state.as_ref(),
            &self.capabilities,
        )?;
        if self.kind.as_str() == "babble.media" {
            serde_json::from_value::<MediaObjectPayload>(self.payload.clone())
                .map_err(|error| babble_types::Error::Canonical(error.to_string()))?
                .validate()?;
        }
        let mut resources = BTreeSet::new();
        for resource in &self.resources {
            validate_resource(resource)?;
            if !resources.insert(resource.integrity.clone()) {
                return Err(babble_types::Error::Conflict(format!(
                    "duplicate draft resource: {}",
                    resource.integrity
                )));
            }
        }
        for surface in &self.surfaces {
            validate_surface(surface)?;
        }
        let mut capability_keys = BTreeSet::new();
        for capability in &self.capabilities {
            validate_capability_request(capability)?;
            let key = format!(
                "{}@{}:{}",
                capability.id,
                capability.version,
                serde_json::to_string(&capability.scope)
                    .map_err(|err| babble_types::Error::Canonical(err.to_string()))?
            );
            if !capability_keys.insert(key) {
                return Err(babble_types::Error::Conflict(format!(
                    "duplicate draft capability request: {}@{}",
                    capability.id, capability.version
                )));
            }
        }
        validate_provenance(&self.provenance)
    }
}

impl EdgeDraft {
    pub fn new(
        source: ObjectId,
        target: ObjectId,
        relation: Relation,
        origin: EdgeOrigin,
    ) -> Result<Self> {
        source.validate()?;
        target.validate()?;
        Ok(Self {
            source,
            target,
            relation,
            origin,
            metadata: BTreeMap::new(),
        })
    }

    pub fn with_metadata(mut self, metadata: BTreeMap<String, Value>) -> Result<Self> {
        for key in metadata.keys() {
            if key.trim().is_empty() {
                return Err(babble_types::Error::Conflict(
                    "edge metadata keys must not be empty".to_string(),
                ));
            }
        }
        self.metadata = metadata;
        Ok(self)
    }

    pub fn sign(&self, author: &Identity, keypair: &Keypair) -> Result<Edge> {
        Edge::new(
            self.source.clone(),
            self.target.clone(),
            self.relation.clone(),
            self.origin.clone(),
            Some(author.id.clone()),
        )?
        .with_metadata(self.metadata.clone())?
        .sign(author, keypair)
    }
}

impl CapabilityGrantDraft {
    pub fn new(
        object_id: ObjectId,
        request: CapabilityRequest,
        decision: GrantDecision,
        expires_at: Option<Timestamp>,
    ) -> Result<Self> {
        object_id.validate()?;
        validate_capability_request(&request)?;
        Ok(Self {
            object_id,
            request,
            decision,
            expires_at,
        })
    }
}

fn validate_resource(resource: &Resource) -> Result<()> {
    ResourceUri::parse(&resource.uri)?.validate_integrity(&resource.integrity)?;
    normalize_media_type(resource.media_type.clone())?;
    Ok(())
}

fn validate_surface(surface: &Surface) -> Result<()> {
    let uri = ResourceUri::parse(&surface.entry)?;
    if let Some(integrity) = &surface.integrity {
        uri.validate_integrity(integrity)?;
    }
    if !matches!(surface.target, SurfaceTarget::Static) && surface.integrity.is_none() {
        return Err(babble_types::Error::Conflict(format!(
            "executable surface {} requires resource integrity",
            surface.entry
        )));
    }
    if let Some(bundle) = &surface.bundle {
        bundle.validate_surface(surface)?;
    }
    Ok(())
}

fn validate_provenance(provenance: &Provenance) -> Result<()> {
    if let Some(parent) = &provenance.parent {
        parent.validate()?;
    }
    if let Some(forked_from) = &provenance.forked_from {
        forked_from.validate()?;
    }
    for remixed_from in &provenance.remixed_from {
        remixed_from.validate()?;
    }
    Ok(())
}

fn validate_namespaced(label: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() || !value.contains('.') || value.contains(char::is_whitespace) {
        return Err(babble_types::Error::Conflict(format!(
            "{label} must be a non-empty namespaced identifier"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_identity::IdentityKind;
    use babble_object::{SurfaceRole, SurfaceTarget};
    use serde_json::json;

    #[test]
    fn text_draft_builds_unsigned_canonical_object_without_signing() {
        let keypair = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let draft = ObjectDraft::text("  hello Babble  ")
            .unwrap()
            .with_capability(CapabilityRequest {
                id: "babble.realtime.join".to_string(),
                version: 1,
                scope: json!({"room": "self"}),
            })
            .unwrap();

        let object = draft.build_unsigned(&author).unwrap();

        assert_eq!(object.kind.as_str(), "babble.text");
        assert_eq!(object.payload["text"], "hello Babble");
        assert_eq!(object.capabilities.len(), 1);
        assert!(object.signature.is_none());
    }

    #[test]
    fn media_draft_requires_valid_distinct_resources() {
        let blob = MediaBlob::from_bytes("Image/PNG", b"png").unwrap();
        let draft = ObjectDraft::media(
            "First image",
            Some("A resource".to_string()),
            vec![blob.clone()],
        )
        .unwrap();

        assert_eq!(draft.kind.as_str(), "babble.media");
        assert_eq!(draft.resources, vec![blob.resource()]);
        assert_eq!(
            draft.required_blob_hashes(),
            BTreeSet::from([blob.integrity.clone()])
        );
        assert!(ObjectDraft::media("duplicate", None, vec![blob.clone(), blob]).is_err());
    }

    #[test]
    fn media_album_draft_requires_matching_delivery_resources_but_allows_surface_assets() {
        let keypair = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "author", &keypair).unwrap();
        let blobs: Vec<_> = (0..3)
            .map(|i| MediaBlob::from_bytes("image/png", &[i]).unwrap())
            .collect();
        let draft = ObjectDraft::media("Album", None, blobs.clone()).unwrap();
        let mut absent = draft.clone();
        absent.resources.pop();
        assert!(absent.build_unsigned(&author).is_err());
        let mut conflicting = draft.clone();
        conflicting.resources[2].media_type = "audio/wav".into();
        assert!(conflicting.build_unsigned(&author).is_err());
        let script =
            MediaBlob::from_bytes("text/javascript", b"export function mount() {}").unwrap();
        let extended = draft.with_resource(script.resource()).unwrap();
        let object = extended.build_unsigned(&author).unwrap();
        assert_eq!(object.payload["resources"], json!(blobs));
        assert_eq!(object.resources.len(), 4);
    }

    #[test]
    fn executable_surfaces_require_integrity() {
        let rejected = ObjectDraft::text("hello").unwrap().with_surface(Surface {
            bundle: None,
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: "surface.js".to_string(),
            integrity: None,
        });

        assert!(rejected.is_err());
    }

    #[test]
    fn edge_and_grant_drafts_validate_publication_material() {
        let source = ObjectId::from_hash(&Hash::from_bytes(b"source"));
        let target = ObjectId::from_hash(&Hash::from_bytes(b"target"));
        let edge = EdgeDraft::new(
            source.clone(),
            target,
            Relation::References,
            EdgeOrigin::ApplicationAssertion,
        )
        .unwrap();
        assert_eq!(edge.source, source);

        let grant = CapabilityGrantDraft::new(
            source,
            CapabilityRequest {
                id: "babble.storage.local".to_string(),
                version: 1,
                scope: json!({"namespace": "draft-test"}),
            },
            GrantDecision::Approved,
            None,
        )
        .unwrap();
        assert_eq!(grant.request.id, "babble.storage.local");
    }
}
