use crate::{CapabilityBindingUsage, LocalNode};
use babel_capabilities::CapabilityReceipt;
use babel_judgment::JudgmentProvider;
use babel_realtime::{RealtimeMessage, RealtimePayload, RealtimeSession, RealtimeSnapshot};
use babel_types::{Canonical, IdentityId, ObjectId, RealtimeRoomId, RealtimeSessionId, Result};

const REALTIME_JOIN_CAPABILITY: &str = "babel.realtime.join";
const REALTIME_SEND_CAPABILITY: &str = "babel.realtime.send";
const REALTIME_LEAVE_CAPABILITY: &str = "babel.realtime.leave";
const REALTIME_CAPABILITY_VERSION: u32 = 1;

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn start_realtime_session_with_capability(
        &mut self,
        author_id: &IdentityId,
        room_id: &RealtimeRoomId,
        grant_ids: &[String],
    ) -> Result<(RealtimeSession, CapabilityReceipt)> {
        self.check_ready()?;
        let room = self
            .realtime
            .room(room_id)
            .ok_or_else(|| babel_types::Error::NotFound(room_id.to_string()))?;
        let receipt = self.authorize_realtime_capability(
            &room.spec.object_id,
            REALTIME_JOIN_CAPABILITY,
            &room.spec.name,
            0,
            1,
            grant_ids,
        )?;
        let session = self.start_realtime_session(author_id, room_id)?;
        Ok((session, receipt))
    }

    pub fn close_realtime_session_with_capability(
        &mut self,
        author_id: &IdentityId,
        session_id: &RealtimeSessionId,
        object_id: &ObjectId,
        grant_ids: &[String],
    ) -> Result<(RealtimeSession, babel_state::Event, CapabilityReceipt)> {
        self.check_ready()?;
        let session = self
            .realtime
            .session(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        if &session.object_id != object_id {
            return Err(babel_types::Error::Conflict(format!(
                "realtime session {} belongs to Object {}, not {}",
                session.id, session.object_id, object_id
            )));
        }
        let room = self
            .realtime
            .room(&session.room_id)
            .ok_or_else(|| babel_types::Error::NotFound(session.room_id.to_string()))?;
        let receipt = self.authorize_realtime_capability(
            object_id,
            REALTIME_LEAVE_CAPABILITY,
            &room.spec.name,
            0,
            0,
            grant_ids,
        )?;
        let (session, event) = self.close_realtime_session(author_id, session_id, object_id)?;
        Ok((session, event, receipt))
    }

    pub fn publish_realtime_message_with_capability(
        &mut self,
        author_id: &IdentityId,
        session_id: &RealtimeSessionId,
        object_id: &ObjectId,
        payload: RealtimePayload,
        durable: bool,
        grant_ids: &[String],
    ) -> Result<(RealtimeMessage, Option<RealtimeSnapshot>, CapabilityReceipt)> {
        self.check_ready()?;
        let session = self
            .realtime
            .session(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        if &session.object_id != object_id {
            return Err(babel_types::Error::Conflict(format!(
                "realtime session {} belongs to Object {}, not {}",
                session.id, session.object_id, object_id
            )));
        }
        let room = self
            .realtime
            .room(&session.room_id)
            .ok_or_else(|| babel_types::Error::NotFound(session.room_id.to_string()))?;
        let requested_bytes = payload.canonical_bytes()?.len() as u64;
        let receipt = self.authorize_realtime_capability(
            object_id,
            REALTIME_SEND_CAPABILITY,
            &room.spec.name,
            requested_bytes,
            0,
            grant_ids,
        )?;
        let (message, snapshot) =
            self.publish_realtime_message(author_id, session_id, object_id, payload, durable)?;
        Ok((message, snapshot, receipt))
    }

    fn authorize_realtime_capability(
        &self,
        object_id: &ObjectId,
        capability: &str,
        room_name: &str,
        requested_bytes: u64,
        realtime_connections: u32,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.require_object(object_id)?;
        let receipt = self.authorize_capability_binding_with_usage(
            object_id,
            capability,
            REALTIME_CAPABILITY_VERSION,
            grant_ids,
            CapabilityBindingUsage {
                requested_bytes,
                realtime_connections,
                windows: Vec::new(),
            },
        )?;
        let scoped_room = receipt
            .scope
            .get("room")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                babel_types::Error::Conflict(
                    "realtime capability grant scope must include room".to_string(),
                )
            })?;
        if scoped_room != room_name {
            return Err(babel_types::Error::Conflict(format!(
                "realtime capability room scope {scoped_room} does not match room {room_name}"
            )));
        }
        Ok(receipt)
    }
}
