use babble_crypto::Keypair;
use babble_graph::moderation::*;
use babble_identity::{Identity, IdentityKind};
use babble_types::{Canonical, IdentityId, ObjectId, Timestamp};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(crate) fn fixtures() -> babble_types::Result<Value> {
    let key = Keypair::from_ed25519_secret_hex(&"41".repeat(32))?;
    let at: Timestamp = serde_json::from_value(json!("2026-09-30T00:00:00Z"))
        .map_err(|e| babble_types::Error::Canonical(e.to_string()))?;
    let identity = |handle: &str| -> babble_types::Result<Identity> {
        let commitment =
            json!({"kind":"Person","handle":handle,"public_key":key.public_key(),"created_at":at});
        let identity = Identity {
            id: IdentityId::from_hash(&commitment.canonical_hash()?),
            kind: IdentityKind::Person,
            handle: handle.into(),
            public_key: key.public_key(),
            created_at: at,
            signature: key.sign(&commitment.canonical_bytes()?),
        };
        identity.verify()?;
        Ok(identity)
    };
    let reporter = identity("moderation-reporter")?;
    let subject = identity("moderation-subject")?;
    let reviewer = identity("moderation-reviewer")?;
    let reviewers = BTreeSet::from([reviewer.id.clone()]);
    let intent = ModerationIntent::Report(ReportRequest {
        object_id: ObjectId::from_hash(&"moderation-fixture-object".canonical_hash()?),
        reason: ModerationReason::Fraud,
        details: "Private evidence for the canonical moderation fixture".into(),
        idempotency_key: "moderation-fixture-report".into(),
    });
    let pending = transition(None, &reporter.id, &reviewers, &intent, &subject.id, 1, at)?;
    let receipt = ModerationReceipt::sign(
        ModerationReceiptPayload {
            actor: reporter.id.clone(),
            reviewers: reviewers.clone(),
            intent,
            result: pending.clone(),
            sequence: 1,
            previous_id: None,
        },
        &key,
    )?;
    receipt.verify(&reporter)?;
    let request = DecisionRequest {
        outcome: ModerationOutcome::Restrict,
        reason: ModerationReason::Fraud,
        explanation: "Reviewed the evidence under the integrity policy".into(),
        policy_version: POLICY.into(),
        source_signals: vec![],
        expected_revision: 1,
        idempotency_key: "moderation-fixture-decision".into(),
    };
    let decided = transition(
        Some(pending.clone()),
        &reviewer.id,
        &reviewers,
        &ModerationIntent::Decision {
            case_id: pending.id.clone(),
            request,
        },
        &subject.id,
        2,
        at,
    )?;
    let appealed = transition(
        Some(decided),
        &subject.id,
        &reviewers,
        &ModerationIntent::Appeal {
            case_id: pending.id.clone(),
            request: AppealRequest {
                details: "Private appeal evidence for the canonical fixture".into(),
                expected_revision: 2,
                idempotency_key: "moderation-fixture-appeal".into(),
            },
        },
        &subject.id,
        3,
        at,
    )?;
    Ok(
        json!({"reporter":reporter,"subject":subject,"reviewer":reviewer,"pending":pending,"receipt":receipt,"reviewer_view":appealed,"author_view":appealed.clone().view(&subject.id,false)?,"reporter_view":appealed.clone().view(&reporter.id,false)?}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn moderation_fixtures_are_reproducible_and_nullable_fields_required() {
        let first = super::fixtures().unwrap();
        assert_eq!(first, super::fixtures().unwrap());
        assert!(first["author_view"]["reporter_id"].is_null());
        assert!(first["author_view"]["details"].is_null());
        assert!(first["reporter_view"]["appeal"]["details"].is_null());
        let schemas = crate::protocol_schemas().unwrap();
        for (name, fields) in [
            (
                "moderation.ModerationCase",
                vec!["reporter_id", "reason", "details", "appeal"],
            ),
            (
                "moderation.ModerationAppeal",
                vec!["appellant_id", "details"],
            ),
            ("moderation.ModerationPage", vec!["next_before"]),
        ] {
            assert_eq!(schemas[name]["additionalProperties"], false);
            for field in fields {
                assert!(
                    schemas[name]["required"]
                        .as_array()
                        .unwrap()
                        .contains(&serde_json::json!(field))
                );
            }
        }
    }
}
