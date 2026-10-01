use crate::{SandboxPolicy, SurfaceRuntime, SurfaceSessionId, SurfaceSessionPlan};
use babble_capabilities::CapabilityGrant;
use babble_object::{
    Object, SchemaRegistry, Surface, SurfaceRole, SurfaceTarget, bundle::BundleFileKind,
};
use babble_store::VerifiedBundle;
use babble_types::{Canonical, Error, Hash, ObjectId, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const BUNDLE_POLICY_VERSION: u32 = 1;

/// Descriptive policy result, never an authority to start execution without a receipt.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleVerification {
    pub policy_version: u32,
    pub manifest_hash: Hash,
}

/// Transport descriptor attached by the gateway after a session has been admitted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifiedSurfaceMount {
    pub version: u32,
    pub session_id: SurfaceSessionId,
    pub object_id: ObjectId,
    pub role: SurfaceRole,
    pub manifest_hash: Hash,
    pub origin: String,
    pub entry_url: String,
}

impl SurfaceRuntime {
    /// Receipts prove immutable byte readiness. Capability admission is evaluated afresh.
    /// The host must resolve and verify the Object's authoritative signing identity.
    pub fn prepare_verified_surface(
        &self,
        object: &Object,
        role: SurfaceRole,
        grants: &[CapabilityGrant],
        receipt: &VerifiedBundle,
    ) -> Result<SurfaceSessionPlan> {
        self.prepare_surface_with_bundle(object, role, grants, Some(receipt))
    }
}

pub(super) fn validate_receipt(
    object: &Object,
    surface: &Surface,
    receipt: &VerifiedBundle,
) -> Result<BundleVerification> {
    SchemaRegistry::babble_core().validate_object(object)?;
    if object.signature.is_none() {
        return Err(Error::UnsignedObject);
    }
    // Object IDs exclude the ID and signature themselves. Recompute that commitment:
    // a caller must not reuse a verified ID after mutating capabilities or other fields.
    let mut commitment =
        serde_json::to_value(object).map_err(|error| Error::Canonical(error.to_string()))?;
    let fields = commitment
        .as_object_mut()
        .ok_or_else(|| Error::Canonical("Object commitment must be a map".into()))?;
    fields.remove("id");
    fields.remove("signature");
    if ObjectId::from_hash(&commitment.canonical_hash()?) != object.id {
        return Err(Error::Signature);
    }
    let manifest = surface.bundle.as_ref().ok_or_else(|| {
        Error::Conflict("verified admission requires a signed bundle manifest".into())
    })?;
    manifest.validate_surface(surface)?;
    if manifest
        .files
        .iter()
        .any(|file| file.kind == BundleFileKind::Wasm)
    {
        return Err(Error::Conflict(
            "verified bundle policy does not support browser WASM execution".into(),
        ));
    }
    let manifest_hash = manifest.hash()?;
    if receipt.object_id() != &object.id
        || receipt.role() != &surface.role
        || receipt.manifest_hash() != &manifest_hash
        || receipt.entry_path() != manifest.entry_path
    {
        return Err(Error::Conflict(
            "verified bundle receipt does not match Object, role and manifest".into(),
        ));
    }
    Ok(BundleVerification {
        policy_version: BUNDLE_POLICY_VERSION,
        manifest_hash,
    })
}

pub(super) fn verified_sandbox() -> SandboxPolicy {
    let mut policy = SandboxPolicy::for_target(&SurfaceTarget::Web);
    policy.iframe_sandbox = Some("allow-scripts allow-same-origin".into());
    // The gateway must replace frame-ancestors with its exact host origin.
    policy.csp = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; media-src 'self'; connect-src 'self'; worker-src 'none'; frame-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; sandbox allow-scripts allow-same-origin".into();
    policy
}
