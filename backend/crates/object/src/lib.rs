use babel_crypto::Signature;
use babel_graph::Edge;
use babel_identity::Identity;
use babel_types::{Canonical, Hash, ObjectId, Protocol, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub mod bundle;
pub mod resource_uri;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ObjectKind(String);

impl ObjectKind {
    pub fn new(kind: impl Into<String>) -> Self {
        Self(kind.into())
    }

    pub fn text() -> Self {
        Self::new("babel.text")
    }

    pub fn claim() -> Self {
        Self::new("babel.claim")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum SurfaceTarget {
    Static,
    Wasm,
    Web,
    WebGpu,
    NativeTrusted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum SurfaceRole {
    Preview,
    Feed,
    Expanded,
    Fullscreen,
    Background,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Surface {
    pub role: SurfaceRole,
    pub target: SurfaceTarget,
    pub entry: String,
    pub integrity: Option<Hash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<bundle::BundleManifest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Resource {
    pub uri: String,
    pub media_type: String,
    pub integrity: Hash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityRequest {
    pub id: String,
    pub version: u32,
    pub scope: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub parent: Option<ObjectId>,
    pub forked_from: Option<ObjectId>,
    pub remixed_from: Vec<ObjectId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Object {
    pub protocol: Protocol,
    pub id: ObjectId,
    pub author: babel_types::IdentityId,
    pub created_at: Timestamp,
    pub kind: ObjectKind,
    pub schema: String,
    pub payload: Value,
    pub surfaces: Vec<Surface>,
    pub resources: Vec<Resource>,
    pub capabilities: Vec<CapabilityRequest>,
    pub relations: Vec<Edge>,
    pub state: Option<Value>,
    pub provenance: Provenance,
    pub signature: Option<Signature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct ObjectCommitment {
    pub protocol: Protocol,
    pub author: babel_types::IdentityId,
    pub created_at: Timestamp,
    pub kind: ObjectKind,
    pub schema: String,
    pub payload: Value,
    pub surfaces: Vec<Surface>,
    pub resources: Vec<Resource>,
    pub capabilities: Vec<CapabilityRequest>,
    pub relations: Vec<Edge>,
    pub state: Option<Value>,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextPayload {
    pub text: String,
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SchemaRegistry {
    pub protocol: Protocol,
    pub core_schemas: Vec<String>,
    pub core_capabilities: Vec<String>,
}

impl SchemaRegistry {
    pub fn babel_core() -> Self {
        Self {
            protocol: Protocol::default(),
            core_schemas: vec![
                "babel.schema.text.v1".to_string(),
                "babel.schema.media.v1".to_string(),
            ],
            core_capabilities: CORE_CAPABILITIES
                .iter()
                .map(|capability| capability.to_string())
                .collect(),
        }
    }

    pub fn validate_object(&self, object: &Object) -> Result<()> {
        for surface in &object.surfaces {
            if let Some(bundle) = &surface.bundle {
                bundle.validate_surface(surface)?;
            }
        }
        validate_object_contract(
            &object.kind,
            &object.schema,
            &object.payload,
            object.state.as_ref(),
            &object.capabilities,
        )
    }

    pub fn validate_draft(
        &self,
        kind: &ObjectKind,
        schema: &str,
        payload: &Value,
        state: Option<&Value>,
        capabilities: &[CapabilityRequest],
    ) -> Result<()> {
        validate_object_contract(kind, schema, payload, state, capabilities)
    }
}

pub fn validate_object_contract(
    kind: &ObjectKind,
    schema: &str,
    payload: &Value,
    state: Option<&Value>,
    capabilities: &[CapabilityRequest],
) -> Result<()> {
    validate_namespaced("Object kind", kind.as_str())?;
    validate_namespaced("Object schema", schema)?;
    validate_core_schema_payload(kind, schema, payload)?;
    if let Some(state) = state {
        validate_state(schema, state)?;
    }
    for capability in capabilities {
        validate_capability_request(capability)?;
    }
    Ok(())
}

pub fn validate_capability_request(request: &CapabilityRequest) -> Result<()> {
    validate_capability_id(&request.id)?;
    if request.version == 0 {
        return Err(babel_types::Error::Conflict(format!(
            "capability version must be positive: {}",
            request.id
        )));
    }
    if request.id.starts_with("babel.") {
        validate_core_capability_scope(request)?;
    } else if !request.scope.is_object() {
        return Err(babel_types::Error::Conflict(format!(
            "capability {} scope must be a JSON object",
            request.id
        )));
    }
    Ok(())
}

impl Object {
    pub fn create(
        author: &Identity,
        kind: ObjectKind,
        schema: impl Into<String>,
        payload: Value,
    ) -> Result<Self> {
        let schema = schema.into();
        validate_object_contract(&kind, &schema, &payload, None, &[])?;
        let commitment = ObjectCommitment {
            protocol: Protocol::default(),
            author: author.id.clone(),
            created_at: Timestamp::now(),
            kind,
            schema,
            payload,
            surfaces: Vec::new(),
            resources: Vec::new(),
            capabilities: Vec::new(),
            relations: Vec::new(),
            state: None,
            provenance: Provenance {
                parent: None,
                forked_from: None,
                remixed_from: Vec::new(),
            },
        };
        let id = ObjectId::from_hash(&commitment.canonical_hash()?);
        Ok(Self {
            protocol: commitment.protocol,
            id,
            author: commitment.author,
            created_at: commitment.created_at,
            kind: commitment.kind,
            schema: commitment.schema,
            payload: commitment.payload,
            surfaces: commitment.surfaces,
            resources: commitment.resources,
            capabilities: commitment.capabilities,
            relations: commitment.relations,
            state: commitment.state,
            provenance: commitment.provenance,
            signature: None,
        })
    }

    pub fn text(author: &Identity, text: impl Into<String>) -> Result<Self> {
        Self::create(
            author,
            ObjectKind::text(),
            "babel.schema.text.v1",
            serde_json::to_value(TextPayload {
                text: text.into(),
                metadata: BTreeMap::new(),
            })
            .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
        )
    }

    pub fn with_surfaces(mut self, surfaces: Vec<Surface>) -> Result<Self> {
        self.surfaces = surfaces;
        self.refresh_unsigned_id()
    }

    pub fn with_resources(mut self, resources: Vec<Resource>) -> Result<Self> {
        self.resources = resources;
        self.refresh_unsigned_id()
    }

    pub fn with_capabilities(mut self, capabilities: Vec<CapabilityRequest>) -> Result<Self> {
        for capability in &capabilities {
            validate_capability_request(capability)?;
        }
        self.capabilities = capabilities;
        self.refresh_unsigned_id()
    }

    pub fn with_state(mut self, state: Value) -> Result<Self> {
        validate_state(&self.schema, &state)?;
        self.state = Some(state);
        self.refresh_unsigned_id()
    }

    pub fn with_provenance(mut self, provenance: Provenance) -> Result<Self> {
        self.provenance = provenance;
        self.refresh_unsigned_id()
    }

    pub fn with_relations(mut self, relations: Vec<Edge>) -> Result<Self> {
        self.relations = relations;
        self.refresh_unsigned_id()
    }

    pub fn sign(mut self, author: &Identity, keypair: &babel_crypto::Keypair) -> Result<Self> {
        if self.author != author.id {
            return Err(babel_types::Error::Signature);
        }
        SchemaRegistry::babel_core().validate_object(&self)?;
        self.signature = Some(keypair.sign(&self.commitment().canonical_bytes()?));
        Ok(self)
    }

    pub fn verify(&self, author: &Identity) -> Result<()> {
        self.id.validate()?;
        SchemaRegistry::babel_core().validate_object(self)?;
        if self.author != author.id {
            return Err(babel_types::Error::Signature);
        }
        let expected_id = ObjectId::from_hash(&self.commitment().canonical_hash()?);
        if expected_id != self.id {
            return Err(babel_types::Error::Signature);
        }
        let signature = self
            .signature
            .as_ref()
            .ok_or(babel_types::Error::UnsignedObject)?;
        author
            .public_key
            .verify(&self.commitment().canonical_bytes()?, signature)
    }

    fn commitment(&self) -> ObjectCommitment {
        ObjectCommitment {
            protocol: self.protocol.clone(),
            author: self.author.clone(),
            created_at: self.created_at,
            kind: self.kind.clone(),
            schema: self.schema.clone(),
            payload: self.payload.clone(),
            surfaces: self.surfaces.clone(),
            resources: self.resources.clone(),
            capabilities: self.capabilities.clone(),
            relations: self.relations.clone(),
            state: self.state.clone(),
            provenance: self.provenance.clone(),
        }
    }

    fn refresh_unsigned_id(mut self) -> Result<Self> {
        SchemaRegistry::babel_core().validate_object(&self)?;
        self.signature = None;
        self.id = ObjectId::from_hash(&self.commitment().canonical_hash()?);
        Ok(self)
    }
}

const CORE_CAPABILITIES: &[&str] = &[
    "babel.identity.current",
    "babel.social.follow",
    "babel.social.unfollow",
    "babel.social.share",
    "babel.social.reply",
    "babel.storage.local",
    "babel.storage.object",
    "babel.realtime.join",
    "babel.realtime.send",
    "babel.realtime.leave",
    "babel.payments.checkout",
    "babel.ai.judge",
    "babel.ai.generate",
    "babel.ai.embed",
    "babel.ai.transcribe",
    "babel.media.camera",
    "babel.media.microphone",
    "babel.graphics.webgpu",
    "babel.notifications.request",
    "babel.clipboard.write",
    "babel.fullscreen.enter",
    "babel.network.fetch",
    "babel.location",
    "babel.files",
];

fn validate_core_schema_payload(kind: &ObjectKind, schema: &str, payload: &Value) -> Result<()> {
    match schema {
        "babel.schema.text.v1" => {
            if kind.as_str() != "babel.text" && kind.as_str() != "babel.claim" {
                return Err(babel_types::Error::Conflict(format!(
                    "schema {schema} is not valid for Object kind {}",
                    kind.as_str()
                )));
            }
            let text: TextPayload = serde_json::from_value(payload.clone()).map_err(|err| {
                babel_types::Error::Conflict(format!("invalid text payload: {err}"))
            })?;
            if text.text.trim().is_empty() {
                return Err(babel_types::Error::Conflict(
                    "text payload must not be empty".to_string(),
                ));
            }
            for key in text.metadata.keys() {
                if key.trim().is_empty() {
                    return Err(babel_types::Error::Conflict(
                        "text metadata keys must not be empty".to_string(),
                    ));
                }
            }
        }
        "babel.schema.media.v1" => {
            if kind.as_str() != "babel.media" {
                return Err(babel_types::Error::Conflict(format!(
                    "schema {schema} is not valid for Object kind {}",
                    kind.as_str()
                )));
            }
            validate_media_payload(payload)?;
        }
        _ if schema.starts_with("babel.schema.") => {
            return Err(babel_types::Error::Conflict(format!(
                "unsupported core Object schema: {schema}"
            )));
        }
        _ => {}
    }
    Ok(())
}

fn validate_media_payload(payload: &Value) -> Result<()> {
    let object = object(payload, "media payload")?;
    let title = string_field(object, "title", "media payload")?;
    if title.trim().is_empty() {
        return Err(babel_types::Error::Conflict(
            "media payload title must not be empty".to_string(),
        ));
    }
    if let Some(description) = object.get("description").filter(|value| !value.is_null()) {
        string_value(description, "media payload description")?;
    }
    let primary = object
        .get("primary_resource")
        .ok_or_else(|| missing("media payload", "primary_resource"))?;
    let resources = object
        .get("resources")
        .and_then(Value::as_array)
        .ok_or_else(|| type_error("media payload resources", "array"))?;
    if resources.is_empty() {
        return Err(babel_types::Error::Conflict(
            "media payload requires at least one resource".to_string(),
        ));
    }
    let primary_resource = media_resource_key(primary, "primary_resource")?;
    let mut unique = std::collections::BTreeSet::new();
    let mut saw_primary = false;
    for resource in resources {
        let key = media_resource_key(resource, "media resource")?;
        saw_primary |= key == primary_resource;
        if !unique.insert(key.clone()) {
            return Err(babel_types::Error::Conflict(format!(
                "duplicate media resource: {}",
                key.integrity
            )));
        }
    }
    if !saw_primary {
        return Err(babel_types::Error::Conflict(
            "media payload primary_resource must be present in resources".to_string(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct MediaResourceKey {
    integrity: String,
}

fn media_resource_key(value: &Value, label: &str) -> Result<MediaResourceKey> {
    let resource = object(value, label)?;
    let uri = string_field(resource, "uri", label)?;
    let media_type = string_field(resource, "media_type", label)?;
    let integrity = string_field(resource, "integrity", label)?;
    let size_bytes = resource
        .get("size_bytes")
        .and_then(Value::as_u64)
        .ok_or_else(|| type_error(&format!("{label} size_bytes"), "positive integer"))?;
    if size_bytes == 0 {
        return Err(babel_types::Error::Conflict(format!(
            "{label} size_bytes must be greater than zero"
        )));
    }
    if !uri.starts_with("babel://blobs/") {
        return Err(babel_types::Error::Conflict(format!(
            "{label} URI must use babel://blobs/"
        )));
    }
    Hash::new_unchecked(integrity.to_string()).validate()?;
    if uri != format!("babel://blobs/{integrity}") {
        return Err(babel_types::Error::Conflict(format!(
            "{label} URI does not match integrity"
        )));
    }
    validate_media_type(media_type)?;
    Ok(MediaResourceKey {
        integrity: integrity.to_string(),
    })
}

fn validate_state(schema: &str, state: &Value) -> Result<()> {
    if state.is_null() {
        return Err(babel_types::Error::Conflict(format!(
            "state for {schema} must not be null"
        )));
    }
    Ok(())
}

fn validate_core_capability_scope(request: &CapabilityRequest) -> Result<()> {
    let scope = object(&request.scope, &format!("{} scope", request.id))?;
    match request.id.as_str() {
        "babel.network.fetch" => validate_string_array(scope, "origins", |origin| {
            origin.starts_with("https://")
                || origin.starts_with("http://localhost")
                || origin.starts_with("http://127.0.0.1")
                || origin.starts_with("http://[::1]")
        })?,
        "babel.realtime.join" | "babel.realtime.send" | "babel.realtime.leave" => {
            non_empty_string_field(scope, "room", &request.id)?;
        }
        "babel.storage.local" => {
            non_empty_string_field(scope, "namespace", &request.id)?;
        }
        "babel.storage.object" => {
            if let Some(value) = scope.get("object_id") {
                let id = babel_types::ObjectId::new_unchecked(string_value(
                    value,
                    "babel.storage.object object_id",
                )?);
                id.validate()?;
            } else {
                non_empty_string_field(scope, "namespace", &request.id)?;
            }
        }
        "babel.ai.judge" => {
            validate_namespaced(
                "Judgment definition",
                &non_empty_string_field(scope, "definition", &request.id)?,
            )?;
        }
        "babel.ai.generate" => {
            validate_string_array(scope, "tasks", valid_ai_generate_task)?;
            validate_string_array(scope, "output_modalities", valid_ai_modality)?;
            if let Some(value) = scope.get("models") {
                validate_optional_string_array(value, "AI generate models", valid_ai_model)?;
            }
            if let Some(value) = scope.get("max_input_bytes") {
                positive_u64_value(value, "AI generate max_input_bytes")?;
            }
            if let Some(value) = scope.get("max_output_tokens") {
                positive_u64_value(value, "AI generate max_output_tokens")?;
            }
        }
        "babel.ai.embed" => {
            validate_string_array(scope, "input_modalities", valid_ai_embed_modality)?;
            if let Some(value) = scope.get("models") {
                validate_optional_string_array(value, "AI embed models", valid_ai_model)?;
            }
            if let Some(value) = scope.get("max_input_bytes") {
                positive_u64_value(value, "AI embed max_input_bytes")?;
            }
            if let Some(value) = scope.get("dimensions") {
                positive_u64_value(value, "AI embed dimensions")?;
            }
        }
        "babel.ai.transcribe" => {
            validate_string_array(scope, "media_types", |media_type| {
                media_type.starts_with("audio/") || media_type.starts_with("video/")
            })?;
            if let Some(value) = scope.get("models") {
                validate_optional_string_array(value, "AI transcribe models", valid_ai_model)?;
            }
            if let Some(value) = scope.get("languages") {
                validate_optional_string_array(value, "AI transcribe languages", valid_language)?;
            }
            if let Some(value) = scope.get("max_duration_ms") {
                positive_u64_value(value, "AI transcribe max_duration_ms")?;
            }
        }
        "babel.payments.checkout" => {
            validate_string_array(scope, "currencies", valid_currency)?;
            if let Some(value) = scope.get("max_amount_minor") {
                positive_u64_value(value, "payment max_amount_minor")?;
            }
            if let Some(value) = scope.get("merchant_id") {
                bounded_token_value(value, "payment merchant_id", 128)?;
            }
        }
        "babel.notifications.request" => {
            validate_string_array(scope, "categories", valid_category)?;
            if let Some(value) = scope.get("purpose") {
                bounded_text_value(value, "notification purpose", 200)?;
            }
        }
        "babel.media.camera" => {
            validate_string_array(scope, "modes", valid_camera_mode)?;
            validate_string_array(scope, "media_types", |media_type| {
                media_type.starts_with("image/") || media_type.starts_with("video/")
            })?;
            if let Some(value) = scope.get("max_duration_ms") {
                positive_u64_value(value, "camera max_duration_ms")?;
            }
            if let Some(value) = scope.get("facing_modes") {
                let facing_modes = value
                    .as_array()
                    .ok_or_else(|| type_error("camera facing_modes", "array"))?;
                if facing_modes.is_empty() {
                    return Err(babel_types::Error::Conflict(
                        "camera facing_modes must not be empty".to_string(),
                    ));
                }
                for facing_mode in facing_modes {
                    if !valid_camera_facing_mode(&string_value(facing_mode, "camera facing_mode")?)
                    {
                        return Err(babel_types::Error::Conflict(
                            "invalid camera facing_mode".to_string(),
                        ));
                    }
                }
            }
        }
        "babel.media.microphone" => {
            validate_string_array(scope, "modes", valid_microphone_mode)?;
            validate_string_array(scope, "media_types", |media_type| {
                media_type.starts_with("audio/")
            })?;
            if let Some(value) = scope.get("max_duration_ms") {
                positive_u64_value(value, "microphone max_duration_ms")?;
            }
        }
        "babel.social.follow"
        | "babel.social.unfollow"
        | "babel.social.share"
        | "babel.social.reply" => {
            if let Some(value) = scope.get("object_id") {
                let id = babel_types::ObjectId::new_unchecked(string_value(
                    value,
                    "social capability object_id",
                )?);
                id.validate()?;
            }
        }
        id if CORE_CAPABILITIES.contains(&id) => {}
        id => {
            return Err(babel_types::Error::Conflict(format!(
                "unknown core capability: {id}"
            )));
        }
    }
    Ok(())
}

fn validate_string_array(
    object: &Map<String, Value>,
    field: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<()> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| type_error(field, "array"))?;
    if values.is_empty() {
        return Err(babel_types::Error::Conflict(format!(
            "{field} must not be empty"
        )));
    }
    for value in values {
        let value = string_value(value, field)?;
        if value.trim().is_empty() || !predicate(&value) {
            return Err(babel_types::Error::Conflict(format!(
                "invalid {field} entry: {value}"
            )));
        }
    }
    Ok(())
}

fn validate_optional_string_array(
    value: &Value,
    label: &str,
    predicate: impl Fn(&str) -> bool,
) -> Result<()> {
    let values = value.as_array().ok_or_else(|| type_error(label, "array"))?;
    if values.is_empty() {
        return Err(babel_types::Error::Conflict(format!(
            "{label} must not be empty"
        )));
    }
    for value in values {
        let value = string_value(value, label)?;
        if value.trim().is_empty() || !predicate(&value) {
            return Err(babel_types::Error::Conflict(format!(
                "invalid {label} entry: {value}"
            )));
        }
    }
    Ok(())
}

fn non_empty_string_field(object: &Map<String, Value>, field: &str, label: &str) -> Result<String> {
    let value = string_field(object, field, label)?;
    if value.trim().is_empty() {
        return Err(babel_types::Error::Conflict(format!(
            "{label} {field} must not be empty"
        )));
    }
    Ok(value.to_string())
}

fn string_field<'a>(object: &'a Map<String, Value>, field: &str, label: &str) -> Result<&'a str> {
    object
        .get(field)
        .ok_or_else(|| missing(label, field))
        .and_then(|value| value.as_str().ok_or_else(|| type_error(field, "string")))
}

fn string_value(value: &Value, label: &str) -> Result<String> {
    value
        .as_str()
        .map(ToString::to_string)
        .ok_or_else(|| type_error(label, "string"))
}

fn positive_u64_value(value: &Value, label: &str) -> Result<u64> {
    let value = value.as_u64().ok_or_else(|| type_error(label, "u64"))?;
    if value == 0 {
        return Err(babel_types::Error::Conflict(format!(
            "{label} must be positive"
        )));
    }
    Ok(value)
}

fn bounded_token_value(value: &Value, label: &str, max_len: usize) -> Result<String> {
    let value = string_value(value, label)?;
    if value.trim().is_empty() || value.len() > max_len || !token(&value) {
        return Err(babel_types::Error::Conflict(format!(
            "invalid {label}: {value}"
        )));
    }
    Ok(value)
}

fn bounded_text_value(value: &Value, label: &str, max_len: usize) -> Result<String> {
    let value = string_value(value, label)?;
    if value.trim().is_empty() || value.len() > max_len {
        return Err(babel_types::Error::Conflict(format!(
            "invalid {label}: {value}"
        )));
    }
    Ok(value)
}

fn object<'a>(value: &'a Value, label: &str) -> Result<&'a Map<String, Value>> {
    value.as_object().ok_or_else(|| type_error(label, "object"))
}

fn validate_media_type(value: &str) -> Result<()> {
    let Some((kind, subtype)) = value.split_once('/') else {
        return Err(type_error("media type", "type/subtype"));
    };
    let valid = token(kind) && token(subtype);
    if valid {
        Ok(())
    } else {
        Err(type_error("media type", "type/subtype"))
    }
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

fn valid_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn valid_category(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_camera_mode(value: &str) -> bool {
    matches!(value, "photo" | "video" | "stream")
}

fn valid_microphone_mode(value: &str) -> bool {
    matches!(value, "audio_clip" | "stream")
}

fn valid_camera_facing_mode(value: &str) -> bool {
    matches!(value, "any" | "user" | "environment")
}

fn valid_ai_generate_task(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio" | "code" | "json")
}

fn valid_ai_modality(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio" | "json")
}

fn valid_ai_embed_modality(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio")
}

fn valid_ai_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
}

fn valid_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn validate_capability_id(value: &str) -> Result<()> {
    let parts = value.split('.').collect::<Vec<_>>();
    let valid = parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(babel_types::Error::Conflict(format!(
            "invalid capability id: {value}"
        )))
    }
}

fn validate_namespaced(label: &str, value: &str) -> Result<()> {
    let value = value.trim();
    if value.is_empty() || !value.contains('.') || value.contains(char::is_whitespace) {
        return Err(babel_types::Error::Conflict(format!(
            "{label} must be a non-empty namespaced identifier"
        )));
    }
    Ok(())
}

fn missing(label: &str, field: &str) -> babel_types::Error {
    babel_types::Error::Conflict(format!("{label} missing required field {field}"))
}

fn type_error(label: &str, expected: &str) -> babel_types::Error {
    babel_types::Error::Conflict(format!("{label} must be {expected}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_validates_core_text_schema_and_payload() {
        let valid = validate_object_contract(
            &ObjectKind::text(),
            "babel.schema.text.v1",
            &json!({"text": "hello", "metadata": {"topic": "babel"}}),
            None,
            &[],
        );
        assert!(valid.is_ok());

        let empty = validate_object_contract(
            &ObjectKind::text(),
            "babel.schema.text.v1",
            &json!({"text": " ", "metadata": {}}),
            None,
            &[],
        );
        assert!(empty.is_err());

        let wrong_kind = validate_object_contract(
            &ObjectKind::new("babel.media"),
            "babel.schema.text.v1",
            &json!({"text": "hello", "metadata": {}}),
            None,
            &[],
        );
        assert!(wrong_kind.is_err());
    }

    #[test]
    fn registry_validates_core_media_payload() {
        let integrity = Hash::from_bytes(b"media");
        let resource = json!({
            "uri": format!("babel://blobs/{integrity}"),
            "media_type": "image/png",
            "integrity": integrity,
            "size_bytes": 5
        });
        let valid = validate_object_contract(
            &ObjectKind::new("babel.media"),
            "babel.schema.media.v1",
            &json!({
                "title": "Image",
                "description": null,
                "primary_resource": resource,
                "resources": [resource]
            }),
            None,
            &[],
        );
        assert!(valid.is_ok());

        let missing_primary = validate_object_contract(
            &ObjectKind::new("babel.media"),
            "babel.schema.media.v1",
            &json!({
                "title": "Image",
                "primary_resource": resource,
                "resources": []
            }),
            None,
            &[],
        );
        assert!(missing_primary.is_err());
    }

    #[test]
    fn registry_validates_core_capability_scopes() {
        validate_capability_request(&CapabilityRequest {
            id: "babel.network.fetch".to_string(),
            version: 1,
            scope: json!({"origins": ["https://example.com"]}),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.network.fetch".to_string(),
            version: 1,
            scope: json!({"origins": ["http://127.0.0.1:4317"]}),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.realtime.join".to_string(),
            version: 1,
            scope: json!({"room": "object-chat"}),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.payments.checkout".to_string(),
            version: 1,
            scope: json!({"currencies": ["USD"], "max_amount_minor": 5000}),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.notifications.request".to_string(),
            version: 1,
            scope: json!({"categories": ["game.turn", "creator_update"]}),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.media.camera".to_string(),
            version: 1,
            scope: json!({
                "modes": ["photo", "video"],
                "media_types": ["image/jpeg", "video/webm"],
                "max_duration_ms": 30_000,
                "facing_modes": ["user", "environment"]
            }),
        })
        .unwrap();
        validate_capability_request(&CapabilityRequest {
            id: "babel.media.microphone".to_string(),
            version: 1,
            scope: json!({
                "modes": ["audio_clip"],
                "media_types": ["audio/webm"],
                "max_duration_ms": 30_000
            }),
        })
        .unwrap();

        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.network.fetch".to_string(),
                version: 1,
                scope: json!({"origins": ["file:///tmp/secret"]}),
            })
            .is_err()
        );
        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.realtime.join".to_string(),
                version: 1,
                scope: json!({}),
            })
            .is_err()
        );
        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.payments.checkout".to_string(),
                version: 1,
                scope: json!({"currencies": ["usd"]}),
            })
            .is_err()
        );
        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.notifications.request".to_string(),
                version: 1,
                scope: json!({"categories": []}),
            })
            .is_err()
        );
        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.media.camera".to_string(),
                version: 1,
                scope: json!({"modes": ["screen"], "media_types": ["image/jpeg"]}),
            })
            .is_err()
        );
        assert!(
            validate_capability_request(&CapabilityRequest {
                id: "babel.media.microphone".to_string(),
                version: 1,
                scope: json!({"modes": ["audio_clip"], "media_types": ["video/webm"]}),
            })
            .is_err()
        );
    }
}
