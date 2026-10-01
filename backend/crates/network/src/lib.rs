use babble_crypto::{Keypair, Signature};
use babble_identity::Identity;
use babble_judgment::JudgmentProvider;
use babble_node::{ImportBundle, ImportReport, LocalNode};
use babble_types::{Canonical, EventId, Hash, IdentityId, ObjectId, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const NETWORK_PROTOCOL: &str = "babble.network.v1";
pub const NETWORK_PROTOCOL_VERSION: u32 = 1;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct PeerId(pub IdentityId);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetworkLimits {
    pub max_inventory_ids: usize,
    pub max_request_ids: usize,
    pub max_bundle_events: usize,
    pub max_bundle_objects: usize,
}

impl Default for NetworkLimits {
    fn default() -> Self {
        Self {
            max_inventory_ids: 256,
            max_request_ids: 128,
            max_bundle_events: 128,
            max_bundle_objects: 128,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Envelope {
    pub protocol: String,
    pub version: u32,
    pub supported_versions: Vec<u32>,
    pub sender: PeerId,
    pub payload_hash: Hash,
    pub message: Message,
    pub signature: Signature,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct EnvelopeCommitment {
    pub protocol: String,
    pub version: u32,
    pub supported_versions: Vec<u32>,
    pub sender: PeerId,
    pub payload_hash: Hash,
    pub message: Message,
}

impl Envelope {
    pub fn signed(sender: &Identity, keypair: &Keypair, message: Message) -> Result<Self> {
        Self::signed_with_versions(sender, keypair, vec![NETWORK_PROTOCOL_VERSION], message)
    }

    pub fn signed_with_versions(
        sender: &Identity,
        keypair: &Keypair,
        supported_versions: Vec<u32>,
        message: Message,
    ) -> Result<Self> {
        if keypair.public_key() != sender.public_key {
            return Err(babble_types::Error::Signature);
        }
        if supported_versions.is_empty() || !supported_versions.contains(&NETWORK_PROTOCOL_VERSION)
        {
            return Err(babble_types::Error::Conflict(
                "network envelope must advertise the active protocol version".to_string(),
            ));
        }
        let payload_hash = message.canonical_hash()?;
        let commitment = EnvelopeCommitment {
            protocol: NETWORK_PROTOCOL.to_string(),
            version: NETWORK_PROTOCOL_VERSION,
            supported_versions: supported_versions.clone(),
            sender: PeerId(sender.id.clone()),
            payload_hash: payload_hash.clone(),
            message: message.clone(),
        };
        Ok(Self {
            protocol: NETWORK_PROTOCOL.to_string(),
            version: NETWORK_PROTOCOL_VERSION,
            supported_versions,
            sender: PeerId(sender.id.clone()),
            payload_hash,
            message,
            signature: keypair.sign(&commitment.canonical_bytes()?),
        })
    }

    pub fn verify(&self, sender: &Identity) -> Result<()> {
        if self.protocol != NETWORK_PROTOCOL {
            return Err(babble_types::Error::Conflict(format!(
                "unsupported network protocol: {}",
                self.protocol
            )));
        }
        if self.version != NETWORK_PROTOCOL_VERSION {
            return Err(babble_types::Error::Conflict(format!(
                "unsupported network protocol version: {}",
                self.version
            )));
        }
        if !self.supported_versions.contains(&NETWORK_PROTOCOL_VERSION) {
            return Err(babble_types::Error::Conflict(
                "network peer does not support the active protocol version".to_string(),
            ));
        }
        if sender.id != self.sender.0 {
            return Err(babble_types::Error::Signature);
        }
        let expected = self.message.canonical_hash()?;
        if expected != self.payload_hash {
            return Err(babble_types::Error::Conflict(
                "network payload hash mismatch".to_string(),
            ));
        }
        sender
            .public_key
            .verify(&self.commitment().canonical_bytes()?, &self.signature)?;
        Ok(())
    }

    fn commitment(&self) -> EnvelopeCommitment {
        EnvelopeCommitment {
            protocol: self.protocol.clone(),
            version: self.version,
            supported_versions: self.supported_versions.clone(),
            sender: self.sender.clone(),
            payload_hash: self.payload_hash.clone(),
            message: self.message.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum Message {
    Hello { identity: Identity },
    Inventory { events: Vec<EventId> },
    RequestEvents { events: Vec<EventId> },
    EventBundle { bundle: ImportBundle },
    ObjectInventory { objects: Vec<ObjectId> },
    RequestObjects { objects: Vec<ObjectId> },
    ObjectBundle { bundle: ImportBundle },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum NetworkAction {
    None,
    PeerAccepted { identity: Identity },
    RequestEvents { events: Vec<EventId> },
    SendBundle { bundle: ImportBundle },
    RequestObjects { objects: Vec<ObjectId> },
    Imported { report: ImportReport },
}

#[derive(Clone, Debug)]
pub struct GossipEngine {
    limits: NetworkLimits,
}

impl GossipEngine {
    pub fn new(limits: NetworkLimits) -> Self {
        Self { limits }
    }

    pub fn receive<P>(&self, node: &mut LocalNode<P>, envelope: Envelope) -> Result<NetworkAction>
    where
        P: JudgmentProvider,
    {
        let sender = self.sender_identity(node, &envelope)?;
        envelope.verify(&sender)?;
        match envelope.message {
            Message::Hello { identity } => {
                identity.verify()?;
                if identity.id != envelope.sender.0 {
                    return Err(babble_types::Error::Signature);
                }
                let is_new = node.identity(&identity.id).is_none();
                if is_new {
                    node.import_bundle(ImportBundle {
                        identities: vec![identity.clone()],
                        objects: Vec::new(),
                        edges: Vec::new(),
                        events: Vec::new(),
                    })?;
                }
                Ok(NetworkAction::PeerAccepted { identity })
            }
            Message::Inventory { events } => {
                self.ensure_len("inventory", events.len(), self.limits.max_inventory_ids)?;
                let missing = events
                    .into_iter()
                    .filter(|event_id| node.event(event_id).is_none())
                    .take(self.limits.max_request_ids)
                    .collect::<Vec<_>>();
                if missing.is_empty() {
                    Ok(NetworkAction::None)
                } else {
                    Ok(NetworkAction::RequestEvents { events: missing })
                }
            }
            Message::RequestEvents { events } => {
                self.ensure_len("request", events.len(), self.limits.max_request_ids)?;
                let requested = events.into_iter().collect::<BTreeSet<_>>();
                let bundle = node.event_bundle(&requested)?;
                self.ensure_len("bundle", bundle.events.len(), self.limits.max_bundle_events)?;
                Ok(NetworkAction::SendBundle { bundle })
            }
            Message::EventBundle { bundle } => {
                self.ensure_bundle_limits(&bundle)?;
                let report = node.import_bundle(bundle)?;
                Ok(NetworkAction::Imported { report })
            }
            Message::ObjectInventory { objects } => {
                self.ensure_len(
                    "object inventory",
                    objects.len(),
                    self.limits.max_inventory_ids,
                )?;
                let missing = objects
                    .into_iter()
                    .filter(|object_id| node.object(object_id).is_none())
                    .take(self.limits.max_request_ids)
                    .collect::<Vec<_>>();
                if missing.is_empty() {
                    Ok(NetworkAction::None)
                } else {
                    Ok(NetworkAction::RequestObjects { objects: missing })
                }
            }
            Message::RequestObjects { objects } => {
                self.ensure_len("object request", objects.len(), self.limits.max_request_ids)?;
                let requested = objects.into_iter().collect::<BTreeSet<_>>();
                let bundle = node.object_bundle(&requested)?;
                self.ensure_bundle_limits(&bundle)?;
                Ok(NetworkAction::SendBundle { bundle })
            }
            Message::ObjectBundle { bundle } => {
                self.ensure_bundle_limits(&bundle)?;
                let report = node.import_bundle(bundle)?;
                Ok(NetworkAction::Imported { report })
            }
        }
    }

    fn sender_identity<P>(&self, node: &LocalNode<P>, envelope: &Envelope) -> Result<Identity>
    where
        P: JudgmentProvider,
    {
        match &envelope.message {
            Message::Hello { identity } => {
                identity.verify()?;
                if identity.id != envelope.sender.0 {
                    return Err(babble_types::Error::Signature);
                }
                Ok(identity.clone())
            }
            _ => node
                .signing_identity(&envelope.sender.0)
                .map_err(|_| babble_types::Error::NotFound(envelope.sender.0.to_string())),
        }
    }

    fn ensure_bundle_limits(&self, bundle: &ImportBundle) -> Result<()> {
        self.ensure_len(
            "bundle events",
            bundle.events.len(),
            self.limits.max_bundle_events,
        )?;
        self.ensure_len(
            "bundle objects",
            bundle.objects.len(),
            self.limits.max_bundle_objects,
        )
    }

    fn ensure_len(&self, label: &str, actual: usize, limit: usize) -> Result<()> {
        if actual > limit {
            return Err(babble_types::Error::Conflict(format!(
                "{label} length {actual} exceeds limit {limit}"
            )));
        }
        Ok(())
    }
}
