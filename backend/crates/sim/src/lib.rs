use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_network::{Envelope, GossipEngine, Message, NetworkAction, NetworkLimits};
use babel_node::LocalNode;
use babel_object::Object;
use babel_types::{EventId, IdentityId, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveryStatus {
    Delivered,
    DroppedPartition,
    Rejected,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SimulationStats {
    pub delivered: u64,
    pub dropped_partition: u64,
    pub rejected: u64,
    pub imported_events: u64,
    pub duplicate_events: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeliveryReport {
    pub status: DeliveryStatus,
    pub actions: Vec<NetworkAction>,
}

pub struct SimNode {
    pub identity: Identity,
    keypair: Keypair,
    pub node: LocalNode<LocalProvider>,
}

pub struct NetworkSimulator {
    engine: GossipEngine,
    nodes: Vec<SimNode>,
    blocked_links: BTreeSet<(usize, usize)>,
    stats: SimulationStats,
}

impl NetworkSimulator {
    pub fn new(limits: NetworkLimits) -> Self {
        Self {
            engine: GossipEngine::new(limits),
            nodes: Vec::new(),
            blocked_links: BTreeSet::new(),
            stats: SimulationStats::default(),
        }
    }

    pub fn add_node(&mut self, root: impl Into<PathBuf>, handle: &str) -> Result<usize> {
        let mut node = LocalNode::open(root, LocalProvider::default())?;
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, handle, &keypair)?;
        node.import_signing_identity(identity.clone(), keypair.clone())?;
        self.nodes.push(SimNode {
            identity,
            keypair,
            node,
        });
        Ok(self.nodes.len() - 1)
    }

    pub fn identity(&self, index: usize) -> Result<&Identity> {
        Ok(&self.require_node(index)?.identity)
    }

    pub fn node(&self, index: usize) -> Result<&LocalNode<LocalProvider>> {
        Ok(&self.require_node(index)?.node)
    }

    pub fn publish_text(&mut self, index: usize, text: &str) -> Result<Object> {
        let node = self.require_node_mut(index)?;
        node.node.publish_text(&node.identity.id, text)
    }

    pub fn events(&self, index: usize) -> Result<Vec<EventId>> {
        Ok(self
            .require_node(index)?
            .node
            .store()
            .list_events()?
            .into_iter()
            .map(|event| event.id)
            .collect())
    }

    pub fn partition(&mut self, left: usize, right: usize) -> Result<()> {
        self.require_node(left)?;
        self.require_node(right)?;
        self.blocked_links.insert(link_key(left, right));
        Ok(())
    }

    pub fn heal(&mut self, left: usize, right: usize) -> Result<()> {
        self.require_node(left)?;
        self.require_node(right)?;
        self.blocked_links.remove(&link_key(left, right));
        Ok(())
    }

    pub fn sync_pair(&mut self, source: usize, target: usize) -> Result<DeliveryReport> {
        let events = self.events(source)?;
        self.ensure_authenticated_pair(source, target)?;
        self.deliver(
            source,
            target,
            Message::Inventory {
                events: events.clone(),
            },
        )
    }

    pub fn sync_all(&mut self) -> Result<Vec<DeliveryReport>> {
        let mut reports = Vec::new();
        for source in 0..self.nodes.len() {
            for target in 0..self.nodes.len() {
                if source == target {
                    continue;
                }
                reports.push(self.sync_pair(source, target)?);
            }
        }
        Ok(reports)
    }

    pub fn deliver(
        &mut self,
        source: usize,
        target: usize,
        message: Message,
    ) -> Result<DeliveryReport> {
        self.require_node(source)?;
        self.require_node(target)?;
        if self.blocked_links.contains(&link_key(source, target)) {
            self.stats.dropped_partition += 1;
            return Ok(DeliveryReport {
                status: DeliveryStatus::DroppedPartition,
                actions: Vec::new(),
            });
        }

        let envelope = self.envelope(source, message)?;
        self.deliver_envelope(source, target, envelope)
    }

    pub fn deliver_tampered_hash(
        &mut self,
        source: usize,
        target: usize,
        message: Message,
    ) -> Result<DeliveryReport> {
        self.require_node(source)?;
        self.require_node(target)?;
        let mut envelope = self.envelope(source, message)?;
        envelope.payload_hash = babel_types::Hash::from_bytes(b"tampered network message");
        self.deliver_envelope(source, target, envelope)
    }

    pub fn stats(&self) -> &SimulationStats {
        &self.stats
    }

    fn deliver_envelope(
        &mut self,
        source: usize,
        target: usize,
        envelope: Envelope,
    ) -> Result<DeliveryReport> {
        if self.blocked_links.contains(&link_key(source, target)) {
            self.stats.dropped_partition += 1;
            return Ok(DeliveryReport {
                status: DeliveryStatus::DroppedPartition,
                actions: Vec::new(),
            });
        }

        let mut actions = Vec::new();
        let first = {
            let engine = self.engine.clone();
            let target_node = &mut self.require_node_mut(target)?.node;
            engine.receive(target_node, envelope)
        };
        let Ok(action) = first else {
            self.stats.rejected += 1;
            return Ok(DeliveryReport {
                status: DeliveryStatus::Rejected,
                actions,
            });
        };
        self.record_action(&action);
        actions.push(action.clone());

        match action {
            NetworkAction::RequestEvents { events } => {
                let response_envelope = self.envelope(target, Message::RequestEvents { events })?;
                let response = {
                    let engine = self.engine.clone();
                    let source_node = &mut self.require_node_mut(source)?.node;
                    engine.receive(source_node, response_envelope)?
                };
                self.record_action(&response);
                actions.push(response.clone());

                if let NetworkAction::SendBundle { bundle } = response {
                    let bundle_envelope = self.envelope(source, Message::EventBundle { bundle })?;
                    let imported = {
                        let engine = self.engine.clone();
                        let target_node = &mut self.require_node_mut(target)?.node;
                        engine.receive(target_node, bundle_envelope)?
                    };
                    self.record_action(&imported);
                    actions.push(imported);
                }
            }
            NetworkAction::RequestObjects { objects } => {
                let response_envelope =
                    self.envelope(target, Message::RequestObjects { objects })?;
                let response = {
                    let engine = self.engine.clone();
                    let source_node = &mut self.require_node_mut(source)?.node;
                    engine.receive(source_node, response_envelope)?
                };
                self.record_action(&response);
                actions.push(response.clone());

                if let NetworkAction::SendBundle { bundle } = response {
                    let bundle_envelope =
                        self.envelope(source, Message::ObjectBundle { bundle })?;
                    let imported = {
                        let engine = self.engine.clone();
                        let target_node = &mut self.require_node_mut(target)?.node;
                        engine.receive(target_node, bundle_envelope)?
                    };
                    self.record_action(&imported);
                    actions.push(imported);
                }
            }
            NetworkAction::None
            | NetworkAction::PeerAccepted { .. }
            | NetworkAction::SendBundle { .. }
            | NetworkAction::Imported { .. } => {}
        }

        self.stats.delivered += 1;
        Ok(DeliveryReport {
            status: DeliveryStatus::Delivered,
            actions,
        })
    }

    fn record_action(&mut self, action: &NetworkAction) {
        if let NetworkAction::Imported { report } = action {
            self.stats.imported_events += report.events as u64;
            self.stats.duplicate_events += report.duplicate_events as u64;
        }
    }

    fn ensure_authenticated_pair(&mut self, left: usize, right: usize) -> Result<()> {
        if self.blocked_links.contains(&link_key(left, right)) {
            return Ok(());
        }
        self.send_hello(left, right)?;
        self.send_hello(right, left)
    }

    fn send_hello(&mut self, source: usize, target: usize) -> Result<()> {
        if self
            .require_node(target)?
            .node
            .identity(&self.require_node(source)?.identity.id)
            .is_some()
        {
            return Ok(());
        }
        let envelope = self.envelope(
            source,
            Message::Hello {
                identity: self.require_node(source)?.identity.clone(),
            },
        )?;
        let engine = self.engine.clone();
        let target_node = &mut self.require_node_mut(target)?.node;
        let action = engine.receive(target_node, envelope)?;
        self.record_action(&action);
        Ok(())
    }

    fn envelope(&self, source: usize, message: Message) -> Result<Envelope> {
        let node = self.require_node(source)?;
        Envelope::signed(&node.identity, &node.keypair, message)
    }

    fn require_node(&self, index: usize) -> Result<&SimNode> {
        self.nodes
            .get(index)
            .ok_or_else(|| babel_types::Error::NotFound(format!("sim node {index}")))
    }

    fn require_node_mut(&mut self, index: usize) -> Result<&mut SimNode> {
        self.nodes
            .get_mut(index)
            .ok_or_else(|| babel_types::Error::NotFound(format!("sim node {index}")))
    }
}

fn link_key(left: usize, right: usize) -> (usize, usize) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

pub fn event_sets_by_node(
    simulator: &NetworkSimulator,
) -> Result<BTreeMap<IdentityId, BTreeSet<EventId>>> {
    simulator
        .nodes
        .iter()
        .map(|node| {
            Ok((
                node.identity.id.clone(),
                node.node
                    .store()
                    .list_events()?
                    .into_iter()
                    .map(|event| event.id)
                    .collect(),
            ))
        })
        .collect()
}
