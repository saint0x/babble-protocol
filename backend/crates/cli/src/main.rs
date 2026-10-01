use artifacts::{CapturedArtifacts, MAX_INPUT_BYTES, read_bounded};
use babble_authoring::ObjectDraft;
use babble_capabilities::{CapabilityDecision, CapabilityDecisionStatus};
use babble_crypto::{Keypair, SignatureAlgorithm};
use babble_graph::{Edge, GraphIndex};
use babble_identity::{Identity, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_object::{
    CapabilityRequest, Object, ObjectKind, Provenance, Resource, Surface, SurfaceRole,
    SurfaceTarget,
};
use babble_runtime::{RuntimeAdmissionStatus, SurfaceRuntime, SurfaceSessionPlan};
use babble_store::FileStore;
use babble_types::{Canonical, Hash, IdentityId, ObjectId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

mod artifacts;
mod bundle;
#[cfg(test)]
mod tests;

fn main() {
    if let Err(err) = run(env::args().skip(1).collect()) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), CliError> {
    let command = args.first().map(String::as_str).unwrap_or_default();
    match command {
        "validate" => {
            let path = required_arg(&args, 1, "manifest")?;
            reject_extra(&args, 2)?;
            let output = validate_manifest(Path::new(path))?;
            print_json(&output)
        }
        "build" => build_command(&args),
        "preview" => preview_command(&args),
        "dev" => dev_command(&args),
        "identity" => identity_command(&args),
        "sign" => sign_command(&args),
        "publish" => publish_command(&args),
        "inspect" => inspect_command(&args),
        "graph" => graph_command(&args),
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        _ => Err(CliError::Usage(usage())),
    }
}

fn identity_command(args: &[String]) -> Result<(), CliError> {
    match args.get(1).map(String::as_str) {
        Some("new") => {
            let identity_path = Path::new(required_arg(args, 2, "identity output path")?);
            let key_path = Path::new(required_arg(args, 3, "key output path")?);
            let kind = parse_identity_kind(required_arg(args, 4, "identity kind")?)?;
            let handle = required_arg(args, 5, "handle")?;
            reject_extra(args, 6)?;
            print_json(&create_identity_files(
                identity_path,
                key_path,
                kind,
                handle,
            )?)
        }
        _ => Err(CliError::Usage(usage())),
    }
}

fn build_command(args: &[String]) -> Result<(), CliError> {
    let manifest = required_arg(args, 1, "manifest")?;
    let mut out = None;
    let mut index = 2;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                let path = required_arg(args, index + 1, "output path after --out")?;
                out = Some(PathBuf::from(path));
                index += 2;
            }
            _ => return Err(CliError::Usage(usage())),
        }
    }

    let output = build_manifest(Path::new(manifest))?;
    if let Some(path) = out {
        let bytes = serde_json::to_vec_pretty(&output.draft)?;
        fs::write(&path, bytes)
            .map_err(|err| CliError::Io(format!("write {}: {err}", path.display())))?;
        print_json(&BuildFileReport {
            draft: path,
            report: output.report,
        })
    } else {
        print_json(&output)
    }
}

fn preview_command(args: &[String]) -> Result<(), CliError> {
    let manifest = required_arg(args, 1, "manifest")?;
    reject_extra(args, 2)?;
    print_json(&preview_manifest(Path::new(manifest))?)
}

fn dev_command(args: &[String]) -> Result<(), CliError> {
    let manifest = required_arg(args, 1, "manifest")?;
    reject_extra(args, 2)?;
    let preview = preview_manifest(Path::new(manifest))?;
    print_json(&DevHostReport::from_preview(preview))
}

fn sign_command(args: &[String]) -> Result<(), CliError> {
    let identity = Path::new(required_arg(args, 1, "identity.json")?);
    let key = Path::new(required_arg(args, 2, "key.json")?);
    let manifest = Path::new(required_arg(args, 3, "manifest.json")?);
    let mut out = None;
    let mut index = 4;
    while index < args.len() {
        match args[index].as_str() {
            "--out" => {
                let path = required_arg(args, index + 1, "output path after --out")?;
                out = Some(PathBuf::from(path));
                index += 2;
            }
            _ => return Err(CliError::Usage(usage())),
        }
    }

    let output = sign_manifest(identity, key, manifest)?;
    if let Some(path) = out {
        let bytes = serde_json::to_vec_pretty(&output.object)?;
        fs::write(&path, bytes)
            .map_err(|err| CliError::Io(format!("write {}: {err}", path.display())))?;
        print_json(&SignFileReport {
            object: path,
            report: output.report,
        })
    } else {
        print_json(&output)
    }
}

fn publish_command(args: &[String]) -> Result<(), CliError> {
    let root = Path::new(required_arg(args, 1, "store root")?);
    let identity = Path::new(required_arg(args, 2, "identity.json")?);
    let key = Path::new(required_arg(args, 3, "key.json")?);
    let manifest = Path::new(required_arg(args, 4, "manifest.json")?);
    reject_extra(args, 5)?;
    print_json(&publish_manifest(root, identity, key, manifest)?)
}

fn inspect_command(args: &[String]) -> Result<(), CliError> {
    match args.get(1).map(String::as_str) {
        Some("store") => {
            let root = required_arg(args, 2, "store root")?;
            reject_extra(args, 3)?;
            print_json(&inspect_store(Path::new(root))?)
        }
        Some("object") => {
            let root = required_arg(args, 2, "store root")?;
            let object_id = required_arg(args, 3, "object id")?;
            reject_extra(args, 4)?;
            print_json(&inspect_object(Path::new(root), object_id)?)
        }
        Some("bundle") => {
            let root = required_arg(args, 2, "store root")?;
            let object_id = required_arg(args, 3, "object id")?;
            let role = parse_surface_role(required_arg(args, 4, "surface role")?)?;
            reject_extra(args, 5)?;
            print_json(&inspect_bundle(Path::new(root), object_id, role)?)
        }
        _ => Err(CliError::Usage(usage())),
    }
}

fn graph_command(args: &[String]) -> Result<(), CliError> {
    match args.get(1).map(String::as_str) {
        Some("object") => {
            let root = required_arg(args, 2, "store root")?;
            let object_id = required_arg(args, 3, "object id")?;
            reject_extra(args, 4)?;
            print_json(&graph_object(Path::new(root), object_id)?)
        }
        _ => Err(CliError::Usage(usage())),
    }
}

fn validate_manifest(path: &Path) -> Result<ManifestReport, CliError> {
    let output = build_manifest(path)?;
    Ok(output.report)
}

fn build_manifest(path: &Path) -> Result<BuildOutput, CliError> {
    let manifest = read_manifest(path)?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut artifacts = CapturedArtifacts::default();
    let draft = manifest.into_draft(base, &mut artifacts)?;
    draft.validate()?;
    let report = ManifestReport::from_draft(path, &draft)?;
    Ok(BuildOutput {
        draft,
        report,
        artifacts,
    })
}

fn read_manifest(path: &Path) -> Result<ObjectManifest, CliError> {
    let bytes = read_bounded(path, MAX_INPUT_BYTES)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| CliError::Json(format!("decode {}: {err}", path.display())))
}

fn create_identity_files(
    identity_path: &Path,
    key_path: &Path,
    kind: IdentityKind,
    handle: &str,
) -> Result<IdentityCreateReport, CliError> {
    if identity_path.exists() {
        return Err(CliError::Invalid(format!(
            "identity output already exists: {}",
            identity_path.display()
        )));
    }
    if key_path.exists() {
        return Err(CliError::Invalid(format!(
            "key output already exists: {}",
            key_path.display()
        )));
    }

    let keypair = Keypair::generate();
    let identity = Identity::create(kind, handle, &keypair)?;
    let key_file = DeveloperKeyFile::from_keypair(&identity.id, &keypair);
    write_json_file(
        identity_path,
        &DeveloperIdentityFile {
            identity: identity.clone(),
        },
    )?;
    write_json_file(key_path, &key_file)?;

    Ok(IdentityCreateReport {
        identity: identity.id,
        identity_file: identity_path.to_path_buf(),
        key_file: key_path.to_path_buf(),
    })
}

fn sign_manifest(
    identity_path: &Path,
    key_path: &Path,
    manifest_path: &Path,
) -> Result<SignOutput, CliError> {
    let identity = read_identity_file(identity_path)?;
    let keypair = read_keypair_file(key_path, &identity)?;
    let output = build_manifest(manifest_path)?;
    let object = output
        .draft
        .build_unsigned(&identity)?
        .sign(&identity, &keypair)?;
    object.verify(&identity)?;
    Ok(SignOutput {
        object,
        report: output.report,
    })
}

fn publish_manifest(
    root: &Path,
    identity_path: &Path,
    key_path: &Path,
    manifest_path: &Path,
) -> Result<PublishReport, CliError> {
    let identity = read_identity_file(identity_path)?;
    let keypair = read_keypair_file(key_path, &identity)?;
    let output = build_manifest(manifest_path)?;
    publish_build(root, identity, keypair, output)
}

fn publish_build(
    root: &Path,
    identity: Identity,
    keypair: Keypair,
    output: BuildOutput,
) -> Result<PublishReport, CliError> {
    let mut node = LocalNode::open(root, LocalProvider::default())?;
    ensure_signing_identity(&mut node, identity.clone(), keypair.clone())?;
    let uploaded = output.artifacts.upload(&node)?;
    let object = node.publish_draft(&identity.id, output.draft)?;

    Ok(PublishReport {
        store: root.to_path_buf(),
        identity: identity.id,
        object: object.id,
        uploaded_blobs: uploaded,
        report: output.report,
    })
}

fn preview_manifest(manifest_path: &Path) -> Result<PreviewReport, CliError> {
    let output = build_manifest(manifest_path)?;
    let keypair = Keypair::generate();
    let author = Identity::create(IdentityKind::Application, "babble-preview-host", &keypair)?;
    let object = output.draft.build_unsigned(&author)?;
    let runtime = SurfaceRuntime::babble_default();
    let surface_roles = surface_roles(&object);
    let surfaces = surface_roles
        .into_iter()
        .map(|role| runtime.prepare_surface(&object, role, &[]))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(SurfacePreviewReport::from_plan)
        .collect::<Vec<_>>();

    let summary = PreviewSummary::from_surfaces(&surfaces);
    Ok(PreviewReport {
        manifest: manifest_path.to_path_buf(),
        object_id: object.id,
        unsigned_author: author.id,
        report: output.report,
        summary,
        surfaces,
    })
}

fn surface_roles(object: &Object) -> Vec<SurfaceRole> {
    let mut roles = Vec::new();
    for surface in &object.surfaces {
        if !roles.contains(&surface.role) {
            roles.push(surface.role.clone());
        }
    }
    roles
}

fn ensure_signing_identity(
    node: &mut LocalNode<LocalProvider>,
    identity: Identity,
    keypair: Keypair,
) -> Result<(), CliError> {
    match node.identity(&identity.id) {
        Some(stored) if stored == &identity => {
            node.attach_signing_keypair(&identity.id, keypair)?;
        }
        Some(_) => {
            return Err(CliError::Invalid(format!(
                "stored identity {} does not match supplied identity file",
                identity.id
            )));
        }
        None => {
            node.import_signing_identity(identity, keypair)?;
        }
    }
    Ok(())
}

fn inspect_store(root: &Path) -> Result<StoreReport, CliError> {
    require_store_root(root)?;
    let store = FileStore::open(root)?;
    let identities = store.list_identities()?;
    let objects = store.list_objects()?;
    let edges = store.list_edges()?;
    let events = store.list_events()?;
    let judgments = store.list_judgments()?;

    Ok(StoreReport {
        root: root.to_path_buf(),
        counts: StoreCounts {
            identities: identities.len(),
            objects: objects.len(),
            edges: edges.len(),
            events: events.len(),
            judgments: judgments.len(),
            blobs: count_files(&root.join("blobs"))?,
        },
        identities: identities
            .into_iter()
            .map(|value| value.id.to_string())
            .collect(),
        objects: objects
            .into_iter()
            .map(|value| value.id.to_string())
            .collect(),
        edges: edges
            .into_iter()
            .map(|value| value.id.to_string())
            .collect(),
        events: events
            .into_iter()
            .map(|value| value.id.to_string())
            .collect(),
        judgments: judgments
            .into_iter()
            .map(|value| value.id.to_string())
            .collect(),
    })
}

fn inspect_object(root: &Path, object_id: &str) -> Result<ObjectInspectReport, CliError> {
    require_store_root(root)?;
    let id = object_id_from_arg(object_id)?;
    let store = FileStore::open(root)?;
    let object = store
        .get_object(&id)?
        .ok_or_else(|| CliError::NotFound(format!("object {id}")))?;
    let author = store
        .get_identity(&object.author)?
        .ok_or_else(|| CliError::NotFound(format!("author {}", object.author)))?;
    object.verify(&author)?;

    Ok(ObjectInspectReport {
        object,
        verified: true,
    })
}

fn graph_object(root: &Path, object_id: &str) -> Result<GraphObjectReport, CliError> {
    require_store_root(root)?;
    let id = object_id_from_arg(object_id)?;
    let store = FileStore::open(root)?;
    store
        .get_object(&id)?
        .ok_or_else(|| CliError::NotFound(format!("object {id}")))?;

    let mut index = GraphIndex::default();
    for edge in store.list_edges()? {
        index.insert(edge);
    }
    let incoming = index.incoming(&id).into_iter().cloned().collect::<Vec<_>>();
    let outgoing = index.outgoing(&id).into_iter().cloned().collect::<Vec<_>>();

    Ok(GraphObjectReport {
        object: id,
        counts: EdgeDirectionCounts {
            incoming: incoming.len(),
            outgoing: outgoing.len(),
        },
        incoming_by_relation: relation_counts(&incoming)?,
        outgoing_by_relation: relation_counts(&outgoing)?,
        incoming,
        outgoing,
    })
}

fn inspect_bundle(
    root: &Path,
    object_id: &str,
    role: SurfaceRole,
) -> Result<BundleInspectReport, CliError> {
    require_store_root(root)?;
    let id = object_id_from_arg(object_id)?;
    let node = LocalNode::open(root, LocalProvider::default())?;
    let verified = node.verify_surface_bundle(&id, role)?;
    Ok(BundleInspectReport {
        object_id: verified.object_id().clone(),
        role: verified.role().clone(),
        manifest_hash: verified.manifest_hash().clone(),
        entry_path: verified.entry_path().to_string(),
        verified: true,
        files: verified
            .files()
            .map(|file| VerifiedFileReport {
                path: file.descriptor().path.clone(),
                integrity: file.descriptor().integrity.clone(),
                size_bytes: file.bytes().len() as u64,
            })
            .collect(),
    })
}

fn parse_surface_role(value: &str) -> Result<SurfaceRole, CliError> {
    match value {
        "Preview" | "preview" => Ok(SurfaceRole::Preview),
        "Feed" | "feed" => Ok(SurfaceRole::Feed),
        "Expanded" | "expanded" => Ok(SurfaceRole::Expanded),
        "Fullscreen" | "fullscreen" => Ok(SurfaceRole::Fullscreen),
        "Background" | "background" => Ok(SurfaceRole::Background),
        _ => Err(CliError::Invalid(format!("unknown surface role {value}"))),
    }
}

fn object_id_from_arg(value: &str) -> Result<ObjectId, CliError> {
    let id = ObjectId::new_unchecked(value.to_string());
    id.validate()?;
    Ok(id)
}

fn parse_identity_kind(value: &str) -> Result<IdentityKind, CliError> {
    match value {
        "Person" | "person" => Ok(IdentityKind::Person),
        "Pseudonym" | "pseudonym" => Ok(IdentityKind::Pseudonym),
        "Organization" | "organization" => Ok(IdentityKind::Organization),
        "Service" | "service" => Ok(IdentityKind::Service),
        "Application" | "application" => Ok(IdentityKind::Application),
        "Agent" | "agent" => Ok(IdentityKind::Agent),
        _ => Err(CliError::Invalid(format!(
            "unknown identity kind {value}; expected Person, Pseudonym, Organization, Service, Application, or Agent"
        ))),
    }
}

fn read_identity_file(path: &Path) -> Result<Identity, CliError> {
    let file: DeveloperIdentityFile = read_json_file(path)?;
    file.identity.verify()?;
    Ok(file.identity)
}

fn read_keypair_file(path: &Path, identity: &Identity) -> Result<Keypair, CliError> {
    let file: DeveloperKeyFile = read_json_file(path)?;
    if file.algorithm != SignatureAlgorithm::Ed25519 {
        return Err(CliError::Invalid("unsupported key algorithm".to_string()));
    }
    if file.identity != identity.id {
        return Err(CliError::Invalid(format!(
            "key identity {} does not match {}",
            file.identity, identity.id
        )));
    }
    let keypair = Keypair::from_ed25519_secret_hex(&file.secret_key_hex)?;
    if keypair.public_key() != identity.public_key {
        return Err(CliError::Invalid(format!(
            "key public key does not match identity {}",
            identity.id
        )));
    }
    Ok(keypair)
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, CliError> {
    let bytes = read_bounded(path, MAX_INPUT_BYTES)?;
    serde_json::from_slice(&bytes)
        .map_err(|err| CliError::Json(format!("decode {}: {err}", path.display())))
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| CliError::Io(format!("create {}: {err}", parent.display())))?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).map_err(|err| CliError::Io(format!("write {}: {err}", path.display())))
}

fn require_store_root(root: &Path) -> Result<(), CliError> {
    if !root.exists() {
        return Err(CliError::NotFound(format!("store root {}", root.display())));
    }
    if !root.is_dir() {
        return Err(CliError::Invalid(format!(
            "store root is not a directory: {}",
            root.display()
        )));
    }
    Ok(())
}

fn count_files(path: &Path) -> Result<usize, CliError> {
    if !path.exists() {
        return Ok(0);
    }
    let entries = fs::read_dir(path)
        .map_err(|err| CliError::Io(format!("read dir {}: {err}", path.display())))?;
    let mut count = 0;
    for entry in entries {
        let entry =
            entry.map_err(|err| CliError::Io(format!("read dir {}: {err}", path.display())))?;
        if entry
            .file_type()
            .map_err(|err| CliError::Io(format!("stat {}: {err}", entry.path().display())))?
            .is_file()
        {
            count += 1;
        }
    }
    Ok(count)
}

fn relation_counts(edges: &[Edge]) -> Result<BTreeMap<String, usize>, CliError> {
    let mut counts = BTreeMap::new();
    for edge in edges {
        let relation = match serde_json::to_value(&edge.relation)? {
            Value::String(value) => value,
            value => value.to_string(),
        };
        *counts.entry(relation).or_insert(0) += 1;
    }
    Ok(counts)
}

fn required_arg<'a>(args: &'a [String], index: usize, label: &str) -> Result<&'a str, CliError> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| CliError::Usage(format!("missing {label}\n\n{}", usage())))
}

fn reject_extra(args: &[String], expected_len: usize) -> Result<(), CliError> {
    if args.len() == expected_len {
        Ok(())
    } else {
        Err(CliError::Usage(usage()))
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), CliError> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn usage() -> String {
    [
        "babble developer workflow",
        "",
        "Usage:",
        "  babble validate <manifest.json>",
        "  babble build <manifest.json> [--out draft.json]",
        "  babble preview <manifest.json>",
        "  babble dev <manifest.json>",
        "  babble identity new <identity.json> <key.json> <kind> <handle>",
        "  babble sign <identity.json> <key.json> <manifest.json> [--out object.json]",
        "  babble publish <store-root> <identity.json> <key.json> <manifest.json>",
        "  babble inspect store <store-root>",
        "  babble inspect object <store-root> <object-id>",
        "  babble inspect bundle <store-root> <object-id> <role>",
        "  babble graph object <store-root> <object-id>",
        "",
        "Bundle inputs list already-built local files; compilation and dependency discovery are not performed.",
    ]
    .join("\n")
}

#[derive(Clone, Debug, Deserialize)]
struct ObjectManifest {
    kind: String,
    schema: String,
    payload: Value,
    #[serde(default)]
    surfaces: Vec<SurfaceManifest>,
    #[serde(default)]
    resources: Vec<ResourceManifest>,
    #[serde(default)]
    capabilities: Vec<CapabilityRequest>,
    #[serde(default)]
    state: Option<Value>,
    #[serde(default = "default_provenance")]
    provenance: Provenance,
}

impl ObjectManifest {
    fn into_draft(
        self,
        base: &Path,
        artifacts: &mut CapturedArtifacts,
    ) -> Result<ObjectDraft, CliError> {
        let mut draft = ObjectDraft::new(ObjectKind::new(self.kind), self.schema, self.payload)?;
        for resource in self.resources {
            draft = draft.with_resource(resource.into_resource(base, artifacts)?)?;
        }
        for surface in self.surfaces {
            draft = draft.with_surface(surface.into_surface(base, artifacts)?)?;
        }
        for capability in self.capabilities {
            draft = draft.with_capability(capability)?;
        }
        if let Some(state) = self.state {
            draft = draft.with_state(state)?;
        }
        draft.with_provenance(self.provenance).map_err(Into::into)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceManifest {
    uri: Option<String>,
    media_type: String,
    integrity: Option<Hash>,
    path: Option<PathBuf>,
}

impl ResourceManifest {
    fn into_resource(
        self,
        base: &Path,
        artifacts: &mut CapturedArtifacts,
    ) -> Result<Resource, CliError> {
        let path_hash = self
            .path
            .as_ref()
            .map(|path| artifacts.capture(&base.join(path)).map(|(hash, _)| hash))
            .transpose()?;
        let integrity = merge_integrity(self.integrity, path_hash)?;
        let uri = self
            .uri
            .unwrap_or_else(|| format!("babble://blobs/{}", integrity.as_str()));
        Ok(Resource {
            uri,
            media_type: self.media_type,
            integrity,
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfaceManifest {
    role: SurfaceRole,
    target: SurfaceTarget,
    entry: Option<String>,
    integrity: Option<Hash>,
    path: Option<PathBuf>,
    bundle: Option<bundle::LocalBundle>,
}

impl SurfaceManifest {
    fn into_surface(
        self,
        base: &Path,
        artifacts: &mut CapturedArtifacts,
    ) -> Result<Surface, CliError> {
        if let Some(bundle) = self.bundle {
            if self.path.is_some() || self.entry.is_some() || self.integrity.is_some() {
                return Err(CliError::Invalid(
                    "bundle cannot be combined with surface entry, path, or integrity".into(),
                ));
            }
            let bundle = bundle.capture(base, artifacts)?;
            let entry = bundle.entry_file()?;
            return Ok(Surface {
                role: self.role,
                target: self.target,
                entry: entry.source_uri.clone(),
                integrity: Some(entry.integrity.clone()),
                bundle: Some(bundle),
            });
        }
        let path_hash = self
            .path
            .as_ref()
            .map(|path| artifacts.capture(&base.join(path)).map(|(hash, _)| hash))
            .transpose()?;
        let target = self.target;
        let integrity = if matches!(target, SurfaceTarget::Static)
            && self.integrity.is_none()
            && path_hash.is_none()
        {
            None
        } else {
            Some(merge_integrity(self.integrity, path_hash)?)
        };
        let entry = match (&integrity, self.path.as_ref()) {
            (Some(hash), Some(_)) => format!("babble://blobs/{}", hash.as_str()),
            _ => self
                .entry
                .ok_or_else(|| CliError::Invalid("surface requires entry or path".into()))?,
        };
        Ok(Surface {
            role: self.role,
            target,
            entry,
            integrity,
            bundle: None,
        })
    }
}

fn merge_integrity(declared: Option<Hash>, computed: Option<Hash>) -> Result<Hash, CliError> {
    match (declared, computed) {
        (Some(declared), Some(computed)) if declared != computed => Err(CliError::Invalid(
            format!("integrity mismatch: declared {declared}, computed {computed}"),
        )),
        (Some(hash), _) | (_, Some(hash)) => {
            hash.validate()?;
            Ok(hash)
        }
        (None, None) => Err(CliError::Invalid(
            "resource or executable surface requires integrity or path".to_string(),
        )),
    }
}

fn default_provenance() -> Provenance {
    Provenance {
        parent: None,
        forked_from: None,
        remixed_from: Vec::new(),
    }
}

#[derive(Clone, Debug, Serialize)]
struct ManifestReport {
    manifest: PathBuf,
    draft_hash: Hash,
    kind: String,
    schema: String,
    resources: Vec<ResourceReport>,
    surfaces: Vec<SurfaceReport>,
    capabilities: Vec<CapabilityReport>,
    required_blobs: Vec<Hash>,
}

impl ManifestReport {
    fn from_draft(path: &Path, draft: &ObjectDraft) -> Result<Self, CliError> {
        Ok(Self {
            manifest: path.to_path_buf(),
            draft_hash: draft.canonical_hash()?,
            kind: draft.kind.as_str().to_string(),
            schema: draft.schema.clone(),
            resources: draft
                .resources
                .iter()
                .map(|resource| ResourceReport {
                    uri: resource.uri.clone(),
                    media_type: resource.media_type.clone(),
                    integrity: resource.integrity.clone(),
                })
                .collect(),
            surfaces: draft
                .surfaces
                .iter()
                .map(|surface| SurfaceReport {
                    role: format!("{:?}", surface.role),
                    target: format!("{:?}", surface.target),
                    entry: surface.entry.clone(),
                    integrity: surface.integrity.clone(),
                })
                .collect(),
            capabilities: draft
                .capabilities
                .iter()
                .map(|capability| CapabilityReport {
                    id: capability.id.clone(),
                    version: capability.version,
                    scope: capability.scope.clone(),
                })
                .collect(),
            required_blobs: draft.required_blob_hashes().into_iter().collect(),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
struct ResourceReport {
    uri: String,
    media_type: String,
    integrity: Hash,
}

#[derive(Clone, Debug, Serialize)]
struct SurfaceReport {
    role: String,
    target: String,
    entry: String,
    integrity: Option<Hash>,
}

#[derive(Clone, Debug, Serialize)]
struct CapabilityReport {
    id: String,
    version: u32,
    scope: Value,
}

#[derive(Clone, Debug, Serialize)]
struct PreviewReport {
    manifest: PathBuf,
    object_id: ObjectId,
    unsigned_author: IdentityId,
    report: ManifestReport,
    summary: PreviewSummary,
    surfaces: Vec<SurfacePreviewReport>,
}

#[derive(Clone, Debug, Serialize)]
struct PreviewSummary {
    surface_count: usize,
    ready: usize,
    needs_permission: usize,
    blocked: usize,
    executable_surfaces: usize,
    capability_requests: usize,
    blocked_reasons: Vec<String>,
}

impl PreviewSummary {
    fn from_surfaces(surfaces: &[SurfacePreviewReport]) -> Self {
        let ready = surfaces
            .iter()
            .filter(|surface| surface.admission == RuntimeAdmissionStatus::Ready)
            .count();
        let needs_permission = surfaces
            .iter()
            .filter(|surface| surface.admission == RuntimeAdmissionStatus::NeedsPermission)
            .count();
        let blocked = surfaces
            .iter()
            .filter(|surface| surface.admission == RuntimeAdmissionStatus::Blocked)
            .count();
        let mut blocked_reasons = surfaces
            .iter()
            .flat_map(|surface| surface.blocked_reasons.iter().cloned())
            .collect::<Vec<_>>();
        blocked_reasons.sort();
        blocked_reasons.dedup();
        Self {
            surface_count: surfaces.len(),
            ready,
            needs_permission,
            blocked,
            executable_surfaces: surfaces.iter().filter(|surface| surface.executable).count(),
            capability_requests: surfaces
                .iter()
                .map(|surface| surface.capability_decisions().len())
                .max()
                .unwrap_or_default(),
            blocked_reasons,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct SurfacePreviewReport {
    role: SurfaceRole,
    target: SurfaceTarget,
    entry: String,
    executable: bool,
    admission: RuntimeAdmissionStatus,
    blocked_reasons: Vec<String>,
    plan: SurfaceSessionPlan,
}

impl SurfacePreviewReport {
    fn from_plan(plan: SurfaceSessionPlan) -> Self {
        let executable = !matches!(plan.surface.target, SurfaceTarget::Static);
        Self {
            role: plan.surface.role.clone(),
            target: plan.surface.target.clone(),
            entry: plan.surface.entry.clone(),
            executable,
            admission: plan.admission.clone(),
            blocked_reasons: plan.blocked_reasons.clone(),
            plan,
        }
    }

    fn capability_decisions(&self) -> &[CapabilityDecision] {
        &self.plan.capability_decisions
    }
}

#[derive(Clone, Debug, Serialize)]
struct DevHostReport {
    mode: &'static str,
    emulated_identity: IdentityId,
    object: ObjectId,
    preview: PreviewReport,
    diagnostics: DevDiagnostics,
}

impl DevHostReport {
    fn from_preview(preview: PreviewReport) -> Self {
        let diagnostics = DevDiagnostics::from_preview(&preview);
        Self {
            mode: "local-dev-host",
            emulated_identity: preview.unsigned_author.clone(),
            object: preview.object_id.clone(),
            preview,
            diagnostics,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct DevDiagnostics {
    admitted_surface_count: usize,
    permission_prompt_count: usize,
    denied_or_unavailable_capability_count: usize,
    total_memory_bytes: u64,
    total_network_bytes_per_minute: u64,
    csp_policies: Vec<String>,
    notes: Vec<String>,
}

impl DevDiagnostics {
    fn from_preview(preview: &PreviewReport) -> Self {
        let mut csp_policies = preview
            .surfaces
            .iter()
            .map(|surface| surface.plan.sandbox.csp.clone())
            .collect::<Vec<_>>();
        csp_policies.sort();
        csp_policies.dedup();
        let denied_or_unavailable_capability_count = preview
            .surfaces
            .iter()
            .flat_map(SurfacePreviewReport::capability_decisions)
            .filter(|decision| {
                matches!(
                    decision.status,
                    CapabilityDecisionStatus::Denied
                        | CapabilityDecisionStatus::Unavailable
                        | CapabilityDecisionStatus::VersionUnsupported
                        | CapabilityDecisionStatus::Revoked
                )
            })
            .count();
        let permission_prompt_count = preview
            .surfaces
            .iter()
            .flat_map(SurfacePreviewReport::capability_decisions)
            .filter(|decision| decision.status == CapabilityDecisionStatus::RequiresUser)
            .count();
        let mut notes = Vec::new();
        if preview.summary.blocked > 0 {
            notes.push("blocked surfaces cannot be started by the local runtime".to_string());
        }
        if permission_prompt_count > 0 {
            notes.push(
                "permission-gated capabilities require trusted host approval before activation"
                    .to_string(),
            );
        }
        if preview.summary.surface_count == 0 {
            notes.push("object has no declared surfaces; it can publish as data but not preview as executable media".to_string());
        }
        Self {
            admitted_surface_count: preview.summary.ready,
            permission_prompt_count,
            denied_or_unavailable_capability_count,
            total_memory_bytes: preview
                .surfaces
                .iter()
                .map(|surface| surface.plan.budget.memory_bytes)
                .sum(),
            total_network_bytes_per_minute: preview
                .surfaces
                .iter()
                .map(|surface| surface.plan.budget.network_bytes_per_minute)
                .sum(),
            csp_policies,
            notes,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct BuildOutput {
    draft: ObjectDraft,
    report: ManifestReport,
    #[serde(skip)]
    artifacts: CapturedArtifacts,
}

#[derive(Clone, Debug, Serialize)]
struct BuildFileReport {
    draft: PathBuf,
    report: ManifestReport,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeveloperIdentityFile {
    identity: Identity,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeveloperKeyFile {
    identity: IdentityId,
    algorithm: SignatureAlgorithm,
    public_key: babble_crypto::PublicKey,
    secret_key_hex: String,
}

impl DeveloperKeyFile {
    fn from_keypair(identity: &IdentityId, keypair: &Keypair) -> Self {
        Self {
            identity: identity.clone(),
            algorithm: SignatureAlgorithm::Ed25519,
            public_key: keypair.public_key(),
            secret_key_hex: keypair.ed25519_secret_hex(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct IdentityCreateReport {
    identity: IdentityId,
    identity_file: PathBuf,
    key_file: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
struct SignOutput {
    object: Object,
    report: ManifestReport,
}

#[derive(Clone, Debug, Serialize)]
struct SignFileReport {
    object: PathBuf,
    report: ManifestReport,
}

#[derive(Clone, Debug, Serialize)]
struct PublishReport {
    store: PathBuf,
    identity: IdentityId,
    object: ObjectId,
    uploaded_blobs: Vec<BlobUploadReport>,
    report: ManifestReport,
}

#[derive(Clone, Debug, Serialize)]
struct BlobUploadReport {
    hash: Hash,
    path: PathBuf,
    bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct StoreReport {
    root: PathBuf,
    counts: StoreCounts,
    identities: Vec<String>,
    objects: Vec<String>,
    edges: Vec<String>,
    events: Vec<String>,
    judgments: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct StoreCounts {
    identities: usize,
    objects: usize,
    edges: usize,
    events: usize,
    judgments: usize,
    blobs: usize,
}

#[derive(Clone, Debug, Serialize)]
struct ObjectInspectReport {
    object: babble_object::Object,
    verified: bool,
}

#[derive(Clone, Debug, Serialize)]
struct BundleInspectReport {
    object_id: ObjectId,
    role: SurfaceRole,
    manifest_hash: Hash,
    entry_path: String,
    verified: bool,
    files: Vec<VerifiedFileReport>,
}

#[derive(Clone, Debug, Serialize)]
struct VerifiedFileReport {
    path: String,
    integrity: Hash,
    size_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct GraphObjectReport {
    object: ObjectId,
    counts: EdgeDirectionCounts,
    incoming_by_relation: BTreeMap<String, usize>,
    outgoing_by_relation: BTreeMap<String, usize>,
    incoming: Vec<Edge>,
    outgoing: Vec<Edge>,
}

#[derive(Clone, Debug, Serialize)]
struct EdgeDirectionCounts {
    incoming: usize,
    outgoing: usize,
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Json(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Core(#[from] babble_types::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}
