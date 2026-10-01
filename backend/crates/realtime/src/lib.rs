use babble_types::{
    Canonical, Hash, IdentityId, ObjectId, RealtimeMessageId, RealtimeRoomId, RealtimeSessionId,
    RealtimeSnapshotId, Result, Timestamp,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MembershipPolicy {
    Open,
    AllowList(BTreeSet<IdentityId>),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PersistencePolicy {
    Ephemeral,
    DurableMessages,
    SnapshotEvery { messages: u64 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomLimits {
    pub max_members: usize,
    pub max_payload_bytes: usize,
    pub max_messages_per_session: u64,
}

impl Default for RoomLimits {
    fn default() -> Self {
        Self {
            max_members: 128,
            max_payload_bytes: 64 * 1024,
            max_messages_per_session: 10_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomSpec {
    pub id: RealtimeRoomId,
    pub object_id: ObjectId,
    pub name: String,
    pub schema: String,
    pub membership: MembershipPolicy,
    pub persistence: PersistencePolicy,
    pub limits: RoomLimits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeSchemaRegistry {
    pub schemas: Vec<String>,
}

impl RealtimeSchemaRegistry {
    pub fn babble_core() -> Self {
        Self {
            schemas: vec![
                "babble.realtime.state.v1".to_string(),
                "babble.realtime.chat.v1".to_string(),
            ],
        }
    }

    pub fn validate_room(&self, spec: &RoomSpec) -> Result<()> {
        validate_realtime_schema(&spec.schema)
    }

    pub fn validate_payload(&self, schema: &str, payload: &RealtimePayload) -> Result<()> {
        validate_payload(schema, payload)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct RoomCommitment {
    pub object_id: ObjectId,
    pub name: String,
    pub schema: String,
    pub membership: MembershipPolicy,
    pub persistence: PersistencePolicy,
    pub limits: RoomLimits,
}

impl RoomSpec {
    pub fn new(
        object_id: ObjectId,
        name: impl Into<String>,
        schema: impl Into<String>,
        membership: MembershipPolicy,
        persistence: PersistencePolicy,
        limits: RoomLimits,
    ) -> Result<Self> {
        let commitment = RoomCommitment {
            object_id,
            name: name.into(),
            schema: schema.into(),
            membership,
            persistence,
            limits,
        };
        validate_name(&commitment.name)?;
        validate_realtime_schema(&commitment.schema)?;
        if commitment.limits.max_members == 0 || commitment.limits.max_payload_bytes == 0 {
            return Err(babble_types::Error::Conflict(
                "room limits must allow at least one member and one payload byte".to_string(),
            ));
        }
        if matches!(
            commitment.persistence,
            PersistencePolicy::SnapshotEvery { messages: 0 }
        ) {
            return Err(babble_types::Error::Conflict(
                "snapshot cadence must be at least one message".to_string(),
            ));
        }
        Ok(Self {
            id: RealtimeRoomId::from_hash(&commitment.canonical_hash()?),
            object_id: commitment.object_id,
            name: commitment.name,
            schema: commitment.schema,
            membership: commitment.membership,
            persistence: commitment.persistence,
            limits: commitment.limits,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Active,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeSession {
    pub id: RealtimeSessionId,
    pub room_id: RealtimeRoomId,
    pub object_id: ObjectId,
    pub participant: IdentityId,
    pub created_at: Timestamp,
    pub state: SessionState,
    pub sent_messages: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct SessionCommitment {
    pub room_id: RealtimeRoomId,
    pub object_id: ObjectId,
    pub participant: IdentityId,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeMessage {
    pub id: RealtimeMessageId,
    pub room_id: RealtimeRoomId,
    pub session_id: RealtimeSessionId,
    pub sender: IdentityId,
    pub sequence: u64,
    pub durable: bool,
    pub payload: RealtimePayload,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeSnapshot {
    pub id: RealtimeSnapshotId,
    pub room_id: RealtimeRoomId,
    pub object_id: ObjectId,
    pub session_id: RealtimeSessionId,
    pub sequence: u64,
    pub message_count: u64,
    pub state_hash: Hash,
    pub state: CollaborativeState,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct SnapshotCommitment {
    pub room_id: RealtimeRoomId,
    pub object_id: ObjectId,
    pub session_id: RealtimeSessionId,
    pub sequence: u64,
    pub message_count: u64,
    pub state_hash: Hash,
    pub state: CollaborativeState,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimePublication {
    pub message: RealtimeMessage,
    pub snapshot: Option<RealtimeSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct MessageCommitment {
    pub room_id: RealtimeRoomId,
    pub session_id: RealtimeSessionId,
    pub sender: IdentityId,
    pub sequence: u64,
    pub durable: bool,
    pub payload: RealtimePayload,
    pub created_at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RealtimePayload {
    Presence(Value),
    Broadcast(Value),
    State(RealtimeOperation),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeOperation {
    IncrementCounter { key: String, by: i64 },
    SetRegister { key: String, value: Value },
    AddToSet { key: String, value: String },
    RemoveFromSet { key: String, value: String },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CollaborativeState {
    pub counters: BTreeMap<String, i64>,
    pub registers: BTreeMap<String, RegisterValue>,
    pub sets: BTreeMap<String, BTreeSet<String>>,
    #[serde(default)]
    pub set_versions: BTreeMap<String, BTreeMap<String, SetVersion>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RegisterValue {
    pub value: Value,
    pub message_id: RealtimeMessageId,
    pub updated_at: Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetVersion {
    pub added_by: Option<RealtimeMessageId>,
    pub added_at: Option<Timestamp>,
    pub removed_by: Option<RealtimeMessageId>,
    pub removed_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomPresence {
    pub room_id: RealtimeRoomId,
    pub active_sessions: BTreeSet<RealtimeSessionId>,
    pub participants: BTreeSet<IdentityId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomView {
    pub spec: RoomSpec,
    pub presence: RoomPresence,
    pub state: CollaborativeState,
    pub durable_messages: Vec<RealtimeMessage>,
    pub snapshots: Vec<RealtimeSnapshot>,
}

#[derive(Clone, Debug, Default)]
pub struct RealtimeHub {
    rooms: BTreeMap<RealtimeRoomId, RoomRuntime>,
    sessions: BTreeMap<RealtimeSessionId, RealtimeSession>,
}

#[derive(Clone, Debug)]
struct RoomRuntime {
    spec: RoomSpec,
    messages: Vec<RealtimeMessage>,
    snapshots: Vec<RealtimeSnapshot>,
    state: CollaborativeState,
}

impl RealtimeHub {
    pub fn create_room(&mut self, spec: RoomSpec) -> Result<RoomSpec> {
        RealtimeSchemaRegistry::babble_core().validate_room(&spec)?;
        if let Some(existing) = self.rooms.get(&spec.id) {
            if existing.spec == spec {
                return Ok(spec);
            }
            return Err(babble_types::Error::Conflict(format!(
                "room id conflict: {}",
                spec.id
            )));
        }
        self.rooms.insert(
            spec.id.clone(),
            RoomRuntime {
                spec: spec.clone(),
                messages: Vec::new(),
                snapshots: Vec::new(),
                state: CollaborativeState::default(),
            },
        );
        Ok(spec)
    }

    pub fn room(&self, room_id: &RealtimeRoomId) -> Option<RoomView> {
        self.rooms.get(room_id).map(|runtime| RoomView {
            spec: runtime.spec.clone(),
            presence: self.presence(room_id).unwrap_or(RoomPresence {
                room_id: room_id.clone(),
                active_sessions: BTreeSet::new(),
                participants: BTreeSet::new(),
            }),
            state: runtime.state.clone(),
            durable_messages: runtime
                .messages
                .iter()
                .filter(|message| message.durable)
                .cloned()
                .collect(),
            snapshots: runtime.snapshots.clone(),
        })
    }

    pub fn session(&self, session_id: &RealtimeSessionId) -> Option<&RealtimeSession> {
        self.sessions.get(session_id)
    }

    pub fn rooms_for_object(&self, object_id: &ObjectId) -> Vec<RoomView> {
        self.rooms
            .keys()
            .filter_map(|room_id| {
                let view = self.room(room_id)?;
                if &view.spec.object_id == object_id {
                    Some(view)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn start_session(
        &mut self,
        room_id: &RealtimeRoomId,
        participant: IdentityId,
    ) -> Result<RealtimeSession> {
        let room = self
            .rooms
            .get(room_id)
            .ok_or_else(|| babble_types::Error::NotFound(room_id.to_string()))?;
        if !member_allowed(&room.spec.membership, &participant) {
            return Err(babble_types::Error::Conflict(format!(
                "participant is not allowed in room: {participant}"
            )));
        }
        let active_count = self
            .sessions
            .values()
            .filter(|session| session.room_id == *room_id && session.state == SessionState::Active)
            .count();
        if active_count >= room.spec.limits.max_members {
            return Err(babble_types::Error::Conflict(format!(
                "room member limit reached: {}",
                room.spec.id
            )));
        }
        let commitment = SessionCommitment {
            room_id: room.spec.id.clone(),
            object_id: room.spec.object_id.clone(),
            participant,
            created_at: Timestamp::now(),
        };
        let session = RealtimeSession {
            id: RealtimeSessionId::from_hash(&commitment.canonical_hash()?),
            room_id: commitment.room_id,
            object_id: commitment.object_id,
            participant: commitment.participant,
            created_at: commitment.created_at,
            state: SessionState::Active,
            sent_messages: 0,
        };
        self.sessions.insert(session.id.clone(), session.clone());
        Ok(session)
    }

    pub fn close_session(&mut self, session_id: &RealtimeSessionId) -> Result<RealtimeSession> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| babble_types::Error::NotFound(session_id.to_string()))?;
        if session.state == SessionState::Closed {
            return Err(babble_types::Error::Conflict(format!(
                "session is already closed: {session_id}"
            )));
        }
        session.state = SessionState::Closed;
        Ok(session.clone())
    }

    pub fn publish(
        &mut self,
        session_id: &RealtimeSessionId,
        payload: RealtimePayload,
        durable: bool,
    ) -> Result<RealtimePublication> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| babble_types::Error::NotFound(session_id.to_string()))?;
        if session.state != SessionState::Active {
            return Err(babble_types::Error::Conflict(format!(
                "session is closed: {session_id}"
            )));
        }
        let room = self
            .rooms
            .get_mut(&session.room_id)
            .ok_or_else(|| babble_types::Error::NotFound(session.room_id.to_string()))?;
        if session.sent_messages >= room.spec.limits.max_messages_per_session {
            return Err(babble_types::Error::Conflict(format!(
                "session message limit reached: {session_id}"
            )));
        }
        let payload_bytes = payload.canonical_bytes()?;
        if payload_bytes.len() > room.spec.limits.max_payload_bytes {
            return Err(babble_types::Error::Conflict(format!(
                "payload exceeds room limit: {} > {}",
                payload_bytes.len(),
                room.spec.limits.max_payload_bytes
            )));
        }
        RealtimeSchemaRegistry::babble_core().validate_payload(&room.spec.schema, &payload)?;
        let should_persist = match room.spec.persistence {
            PersistencePolicy::Ephemeral => false,
            PersistencePolicy::DurableMessages => durable,
            PersistencePolicy::SnapshotEvery { .. } => durable,
        };
        let commitment = MessageCommitment {
            room_id: room.spec.id.clone(),
            session_id: session.id.clone(),
            sender: session.participant.clone(),
            sequence: session.sent_messages + 1,
            durable: should_persist,
            payload,
            created_at: Timestamp::now(),
        };
        let message = RealtimeMessage {
            id: RealtimeMessageId::from_hash(&commitment.canonical_hash()?),
            room_id: commitment.room_id,
            session_id: commitment.session_id,
            sender: commitment.sender,
            sequence: commitment.sequence,
            durable: commitment.durable,
            payload: commitment.payload,
            created_at: commitment.created_at,
        };
        apply_message(&mut room.state, &message);
        room.messages.push(message.clone());
        session.sent_messages = message.sequence;
        let snapshot = maybe_snapshot(room, &message)?;
        Ok(RealtimePublication { message, snapshot })
    }

    pub fn apply_session(&mut self, session: RealtimeSession) -> Result<()> {
        self.rooms
            .get(&session.room_id)
            .ok_or_else(|| babble_types::Error::NotFound(session.room_id.to_string()))?;
        if let Some(existing) = self.sessions.get(&session.id) {
            if existing == &session {
                return Ok(());
            }
            let same_session_identity = existing.room_id == session.room_id
                && existing.object_id == session.object_id
                && existing.participant == session.participant
                && existing.created_at == session.created_at;
            let valid_close = same_session_identity
                && existing.state == SessionState::Active
                && session.state == SessionState::Closed
                && existing.sent_messages <= session.sent_messages;
            if valid_close {
                self.sessions.insert(session.id.clone(), session);
                return Ok(());
            }
            return Err(babble_types::Error::Conflict(format!(
                "session id conflict: {}",
                session.id
            )));
        }
        self.sessions.insert(session.id.clone(), session);
        Ok(())
    }

    pub fn apply_message(&mut self, message: RealtimeMessage) -> Result<()> {
        let room = self
            .rooms
            .get_mut(&message.room_id)
            .ok_or_else(|| babble_types::Error::NotFound(message.room_id.to_string()))?;
        RealtimeSchemaRegistry::babble_core()
            .validate_payload(&room.spec.schema, &message.payload)?;
        if room
            .messages
            .iter()
            .any(|existing| existing.id == message.id)
        {
            return Ok(());
        }
        apply_message(&mut room.state, &message);
        room.messages.push(message);
        room.messages.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(())
    }

    pub fn apply_snapshot(&mut self, snapshot: RealtimeSnapshot) -> Result<()> {
        snapshot.id.validate()?;
        snapshot.state_hash.validate()?;
        if snapshot.state.canonical_hash()? != snapshot.state_hash {
            return Err(babble_types::Error::Conflict(format!(
                "snapshot state hash mismatch: {}",
                snapshot.id
            )));
        }
        let room = self
            .rooms
            .get_mut(&snapshot.room_id)
            .ok_or_else(|| babble_types::Error::NotFound(snapshot.room_id.to_string()))?;
        if room.spec.object_id != snapshot.object_id {
            return Err(babble_types::Error::Conflict(format!(
                "snapshot object mismatch for room {}",
                snapshot.room_id
            )));
        }
        if room
            .snapshots
            .iter()
            .any(|existing| existing.id == snapshot.id)
        {
            return Ok(());
        }
        room.state = snapshot.state.clone();
        room.snapshots.push(snapshot);
        room.snapshots.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(())
    }

    pub fn presence(&self, room_id: &RealtimeRoomId) -> Option<RoomPresence> {
        if !self.rooms.contains_key(room_id) {
            return None;
        }
        let active = self
            .sessions
            .values()
            .filter(|session| session.room_id == *room_id && session.state == SessionState::Active)
            .cloned()
            .collect::<Vec<_>>();
        Some(RoomPresence {
            room_id: room_id.clone(),
            active_sessions: active.iter().map(|session| session.id.clone()).collect(),
            participants: active
                .into_iter()
                .map(|session| session.participant)
                .collect(),
        })
    }
}

fn maybe_snapshot(
    room: &mut RoomRuntime,
    message: &RealtimeMessage,
) -> Result<Option<RealtimeSnapshot>> {
    let PersistencePolicy::SnapshotEvery { messages } = room.spec.persistence else {
        return Ok(None);
    };
    if !message.sequence.is_multiple_of(messages) {
        return Ok(None);
    }
    let state = room.state.clone();
    let state_hash = state.canonical_hash()?;
    let commitment = SnapshotCommitment {
        room_id: room.spec.id.clone(),
        object_id: room.spec.object_id.clone(),
        session_id: message.session_id.clone(),
        sequence: message.sequence,
        message_count: room.messages.len() as u64,
        state_hash,
        state,
        created_at: Timestamp::now(),
    };
    let snapshot = RealtimeSnapshot {
        id: RealtimeSnapshotId::from_hash(&commitment.canonical_hash()?),
        room_id: commitment.room_id,
        object_id: commitment.object_id,
        session_id: commitment.session_id,
        sequence: commitment.sequence,
        message_count: commitment.message_count,
        state_hash: commitment.state_hash,
        state: commitment.state,
        created_at: commitment.created_at,
    };
    room.snapshots.push(snapshot.clone());
    Ok(Some(snapshot))
}

fn apply_message(state: &mut CollaborativeState, message: &RealtimeMessage) {
    let RealtimePayload::State(operation) = &message.payload else {
        return;
    };
    match operation {
        RealtimeOperation::IncrementCounter { key, by } => {
            *state.counters.entry(key.clone()).or_default() += by;
        }
        RealtimeOperation::SetRegister { key, value } => {
            let next = RegisterValue {
                value: value.clone(),
                message_id: message.id.clone(),
                updated_at: message.created_at,
            };
            let replace = state.registers.get(key).is_none_or(|current| {
                next.updated_at > current.updated_at
                    || (next.updated_at == current.updated_at
                        && next.message_id > current.message_id)
            });
            if replace {
                state.registers.insert(key.clone(), next);
            }
        }
        RealtimeOperation::AddToSet { key, value } => {
            let version = state
                .set_versions
                .entry(key.clone())
                .or_default()
                .entry(value.clone())
                .or_insert_with(empty_set_version);
            if newer_set_clock(
                version.added_at,
                version.added_by.as_ref(),
                message.created_at,
                &message.id,
            ) {
                version.added_at = Some(message.created_at);
                version.added_by = Some(message.id.clone());
            }
            materialize_set_value(state, key, value);
        }
        RealtimeOperation::RemoveFromSet { key, value } => {
            let version = state
                .set_versions
                .entry(key.clone())
                .or_default()
                .entry(value.clone())
                .or_insert_with(empty_set_version);
            if newer_set_clock(
                version.removed_at,
                version.removed_by.as_ref(),
                message.created_at,
                &message.id,
            ) {
                version.removed_at = Some(message.created_at);
                version.removed_by = Some(message.id.clone());
            }
            materialize_set_value(state, key, value);
        }
    }
}

fn empty_set_version() -> SetVersion {
    SetVersion {
        added_by: None,
        added_at: None,
        removed_by: None,
        removed_at: None,
    }
}

fn newer_set_clock(
    current_at: Option<Timestamp>,
    current_id: Option<&RealtimeMessageId>,
    next_at: Timestamp,
    next_id: &RealtimeMessageId,
) -> bool {
    match (current_at, current_id) {
        (Some(current_at), Some(current_id)) => {
            next_at > current_at || (next_at == current_at && next_id > current_id)
        }
        _ => true,
    }
}

fn materialize_set_value(state: &mut CollaborativeState, key: &str, value: &str) {
    let visible = state
        .set_versions
        .get(key)
        .and_then(|values| values.get(value))
        .is_some_and(
            |version| match (version.added_at, version.added_by.as_ref()) {
                (Some(added_at), Some(added_by)) => {
                    match (version.removed_at, version.removed_by.as_ref()) {
                        (Some(removed_at), Some(removed_by)) => {
                            added_at > removed_at
                                || (added_at == removed_at && added_by > removed_by)
                        }
                        _ => true,
                    }
                }
                _ => false,
            },
        );
    if visible {
        state
            .sets
            .entry(key.to_string())
            .or_default()
            .insert(value.to_string());
    } else if let Some(set) = state.sets.get_mut(key) {
        set.remove(value);
        if set.is_empty() {
            state.sets.remove(key);
        }
    }
}

fn member_allowed(policy: &MembershipPolicy, participant: &IdentityId) -> bool {
    match policy {
        MembershipPolicy::Open => true,
        MembershipPolicy::AllowList(members) => members.contains(participant),
    }
}

fn validate_name(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(babble_types::Error::Conflict(format!(
            "invalid realtime room name: {value}"
        )))
    }
}

fn validate_realtime_schema(value: &str) -> Result<()> {
    match value {
        "babble.realtime.state.v1" | "babble.realtime.chat.v1" => Ok(()),
        value if value.starts_with("babble.realtime.") => Err(babble_types::Error::Conflict(
            format!("unsupported core realtime schema: {value}"),
        )),
        value if namespaced(value) => Ok(()),
        value => Err(babble_types::Error::Conflict(format!(
            "realtime schema must be a namespaced identifier: {value}"
        ))),
    }
}

fn validate_payload(schema: &str, payload: &RealtimePayload) -> Result<()> {
    match schema {
        "babble.realtime.state.v1" => match payload {
            RealtimePayload::State(operation) => validate_operation(operation),
            RealtimePayload::Presence(value) => validate_json_object("presence payload", value),
            RealtimePayload::Broadcast(_) => Err(babble_types::Error::Conflict(
                "state realtime schema does not allow broadcast payloads".to_string(),
            )),
        },
        "babble.realtime.chat.v1" => match payload {
            RealtimePayload::Broadcast(value) | RealtimePayload::Presence(value) => {
                validate_json_object("chat payload", value)
            }
            RealtimePayload::State(_) => Err(babble_types::Error::Conflict(
                "chat realtime schema does not allow state operations".to_string(),
            )),
        },
        _ => Ok(()),
    }
}

fn validate_operation(operation: &RealtimeOperation) -> Result<()> {
    match operation {
        RealtimeOperation::IncrementCounter { key, .. }
        | RealtimeOperation::SetRegister { key, .. }
        | RealtimeOperation::AddToSet { key, .. }
        | RealtimeOperation::RemoveFromSet { key, .. } => validate_key("operation key", key)?,
    }
    match operation {
        RealtimeOperation::SetRegister { value, .. } => {
            if value.is_null() {
                return Err(babble_types::Error::Conflict(
                    "register value must not be null".to_string(),
                ));
            }
        }
        RealtimeOperation::AddToSet { value, .. }
        | RealtimeOperation::RemoveFromSet { value, .. } => {
            validate_key("set value", value)?;
        }
        RealtimeOperation::IncrementCounter { .. } => {}
    }
    Ok(())
}

fn validate_key(label: &str, value: &str) -> Result<()> {
    let valid = !value.trim().is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'));
    if valid {
        Ok(())
    } else {
        Err(babble_types::Error::Conflict(format!(
            "invalid {label}: {value}"
        )))
    }
}

fn validate_json_object(label: &str, value: &Value) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(babble_types::Error::Conflict(format!(
            "{label} must be a JSON object"
        )))
    }
}

fn namespaced(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.contains('.') && !value.contains(char::is_whitespace)
}

pub fn room_hash_seed(object_id: &ObjectId, name: &str) -> Hash {
    Hash::from_bytes(format!("{object_id}:{name}").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object_id() -> ObjectId {
        ObjectId::new_unchecked(format!("obj_{}", "1".repeat(64)))
    }

    fn identity_id(value: char) -> IdentityId {
        IdentityId::new_unchecked(format!("id_{}", value.to_string().repeat(64)))
    }

    #[test]
    fn realtime_hub_tracks_presence_and_collaborative_state() {
        let mut hub = RealtimeHub::default();
        let spec = RoomSpec::new(
            object_id(),
            "main",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::DurableMessages,
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = hub.create_room(spec).unwrap().id;
        let session = hub.start_session(&room_id, identity_id('a')).unwrap();
        let first = hub
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "likes".to_string(),
                    by: 2,
                }),
                true,
            )
            .unwrap();
        let second = hub
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::SetRegister {
                    key: "title".to_string(),
                    value: serde_json::json!("Hello"),
                }),
                true,
            )
            .unwrap();

        let view = hub.room(&room_id).unwrap();
        assert_eq!(
            view.presence.participants,
            BTreeSet::from([identity_id('a')])
        );
        assert_eq!(view.state.counters.get("likes"), Some(&2));
        assert_eq!(view.state.registers["title"].message_id, second.message.id);
        assert_eq!(view.durable_messages, vec![first.message, second.message]);
    }

    #[test]
    fn realtime_sets_converge_when_messages_arrive_out_of_order() {
        let mut source = RealtimeHub::default();
        let spec = RoomSpec::new(
            object_id(),
            "sets",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::DurableMessages,
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = source.create_room(spec.clone()).unwrap().id;
        let session = source.start_session(&room_id, identity_id('a')).unwrap();
        let add = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::AddToSet {
                    key: "selected".to_string(),
                    value: "node-a".to_string(),
                }),
                true,
            )
            .unwrap();
        let remove = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::RemoveFromSet {
                    key: "selected".to_string(),
                    value: "node-a".to_string(),
                }),
                true,
            )
            .unwrap();

        let mut replay = RealtimeHub::default();
        replay.create_room(spec).unwrap();
        replay.apply_session(session).unwrap();
        replay.apply_message(remove.message).unwrap();
        replay.apply_message(add.message).unwrap();

        assert_eq!(
            replay.room(&room_id).unwrap().state.sets.get("selected"),
            None
        );
        assert_eq!(
            replay.room(&room_id).unwrap().state,
            source.room(&room_id).unwrap().state
        );
    }

    #[test]
    fn realtime_set_add_after_remove_remains_visible() {
        let mut source = RealtimeHub::default();
        let spec = RoomSpec::new(
            object_id(),
            "sets-visible",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::DurableMessages,
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = source.create_room(spec.clone()).unwrap().id;
        let session = source.start_session(&room_id, identity_id('a')).unwrap();
        let remove = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::RemoveFromSet {
                    key: "selected".to_string(),
                    value: "node-b".to_string(),
                }),
                true,
            )
            .unwrap();
        let add = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::AddToSet {
                    key: "selected".to_string(),
                    value: "node-b".to_string(),
                }),
                true,
            )
            .unwrap();

        let mut replay = RealtimeHub::default();
        replay.create_room(spec).unwrap();
        replay.apply_session(session).unwrap();
        replay.apply_message(add.message).unwrap();
        replay.apply_message(remove.message).unwrap();

        assert_eq!(
            replay.room(&room_id).unwrap().state.sets["selected"],
            BTreeSet::from(["node-b".to_string()])
        );
        assert_eq!(
            replay.room(&room_id).unwrap().state,
            source.room(&room_id).unwrap().state
        );
    }

    #[test]
    fn snapshot_rooms_commit_state_snapshots_on_cadence() {
        let mut source = RealtimeHub::default();
        let spec = RoomSpec::new(
            object_id(),
            "snapshot-room",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::SnapshotEvery { messages: 2 },
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = source.create_room(spec.clone()).unwrap().id;
        let session = source.start_session(&room_id, identity_id('a')).unwrap();
        let first = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "ticks".to_string(),
                    by: 1,
                }),
                false,
            )
            .unwrap();
        let second = source
            .publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "ticks".to_string(),
                    by: 2,
                }),
                false,
            )
            .unwrap();

        assert!(first.snapshot.is_none());
        let snapshot = second
            .snapshot
            .expect("second state message should produce a snapshot");
        assert_eq!(snapshot.sequence, second.message.sequence);
        assert_eq!(snapshot.message_count, 2);
        assert_eq!(snapshot.state.counters.get("ticks"), Some(&3));
        assert_eq!(
            snapshot.state.canonical_hash().unwrap(),
            snapshot.state_hash
        );

        let view = source.room(&room_id).unwrap();
        assert!(view.durable_messages.is_empty());
        assert_eq!(view.snapshots, vec![snapshot.clone()]);

        let mut replay = RealtimeHub::default();
        replay.create_room(spec).unwrap();
        replay.apply_snapshot(snapshot).unwrap();
        assert_eq!(
            replay.room(&room_id).unwrap().state.counters.get("ticks"),
            Some(&3)
        );
    }

    #[test]
    fn realtime_hub_enforces_membership_and_payload_limits() {
        let mut hub = RealtimeHub::default();
        let allowed = identity_id('a');
        let spec = RoomSpec::new(
            object_id(),
            "limited",
            "babble.realtime.chat.v1",
            MembershipPolicy::AllowList(BTreeSet::from([allowed.clone()])),
            PersistencePolicy::Ephemeral,
            RoomLimits {
                max_members: 1,
                max_payload_bytes: 4,
                max_messages_per_session: 1,
            },
        )
        .unwrap();
        let room_id = hub.create_room(spec).unwrap().id;
        assert!(hub.start_session(&room_id, identity_id('b')).is_err());
        let session = hub.start_session(&room_id, allowed).unwrap();
        let oversized = hub.publish(
            &session.id,
            RealtimePayload::Broadcast(serde_json::json!({"too": "large"})),
            false,
        );
        assert!(oversized.is_err());
    }

    #[test]
    fn realtime_schema_registry_rejects_wrong_payload_family() {
        assert!(
            RoomSpec::new(
                object_id(),
                "bad-schema",
                "babble.realtime.unknown.v1",
                MembershipPolicy::Open,
                PersistencePolicy::Ephemeral,
                RoomLimits::default(),
            )
            .is_err()
        );

        let mut hub = RealtimeHub::default();
        let spec = RoomSpec::new(
            object_id(),
            "state-only",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::DurableMessages,
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = hub.create_room(spec).unwrap().id;
        let session = hub.start_session(&room_id, identity_id('a')).unwrap();

        assert!(
            hub.publish(
                &session.id,
                RealtimePayload::Broadcast(serde_json::json!({"text": "hello"})),
                true,
            )
            .is_err()
        );
        assert!(
            hub.publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::AddToSet {
                    key: "selected".to_string(),
                    value: "".to_string(),
                }),
                true,
            )
            .is_err()
        );
    }
}
