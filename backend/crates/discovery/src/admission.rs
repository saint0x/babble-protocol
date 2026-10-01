use crate::ObjectSignals;
use babble_lens::{Candidate, CandidateSource, CandidateSourceContribution};
use babble_types::ObjectId;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct SourceQueues<'a> {
    summaries: &'a BTreeMap<ObjectId, ObjectSignals>,
    members: BTreeMap<CandidateSource, BTreeSet<&'a ObjectId>>,
}

impl<'a> SourceQueues<'a> {
    pub(crate) fn new(summaries: &'a BTreeMap<ObjectId, ObjectSignals>) -> Self {
        Self {
            summaries,
            members: BTreeMap::new(),
        }
    }

    pub(crate) fn extend<'b>(
        &mut self,
        source: CandidateSource,
        ids: impl IntoIterator<Item = &'b ObjectId>,
    ) {
        let members = self.members.entry(source).or_default();
        for id in ids {
            if let Some((key, summary)) = self.summaries.get_key_value(id)
                && key == &summary.object_id
            {
                members.insert(key);
            }
        }
    }

    pub(crate) fn select(self, limit: usize, seed: u64) -> Vec<Candidate> {
        let mut selected = BTreeSet::new();
        if let Some(exploration) = self.members.get(&CandidateSource::Exploration) {
            selected.extend(exploration.iter().copied().take(limit));
        }

        let mut queues: Vec<_> = self
            .members
            .iter()
            .filter(|(_, ids)| !ids.is_empty())
            .collect();
        queues.sort_by_key(|(source, _)| std::cmp::Reverse(source_priority(source)));
        if !queues.is_empty() {
            let start = (seed % queues.len() as u64) as usize;
            queues.rotate_left(start);
        }
        let mut cursors: Vec<_> = queues.iter().map(|(_, ids)| ids.iter()).collect();
        while selected.len() < limit {
            let before = selected.len();
            for cursor in &mut cursors {
                if selected.len() == limit {
                    break;
                }
                // Overlap consumes no slot or turn: advance to the next unseen object.
                for id in cursor.by_ref() {
                    if selected.insert(*id) {
                        break;
                    }
                }
            }
            if selected.len() == before {
                break;
            }
        }

        let mut candidates: Vec<_> = selected
            .into_iter()
            .map(|id| {
                let mut sources: Vec<_> = self
                    .members
                    .iter()
                    .filter(|(_, ids)| ids.contains(id))
                    .map(|(source, _)| CandidateSourceContribution {
                        source: source.clone(),
                        weight: 1.0,
                    })
                    .collect();
                sources.sort_by_key(|entry| std::cmp::Reverse(source_priority(&entry.source)));
                let mut candidate = self.summaries[id].candidate(sources[0].source.clone());
                candidate.sources = sources;
                candidate
            })
            .collect();
        candidates.sort_by(|left, right| {
            source_priority(&right.source)
                .cmp(&source_priority(&left.source))
                .then_with(|| right.created_at.cmp(&left.created_at))
                .then_with(|| left.object_id.cmp(&right.object_id))
        });
        candidates
    }
}

pub(crate) fn source_priority(source: &CandidateSource) -> u8 {
    match source {
        CandidateSource::Following => 90,
        CandidateSource::Evidence => 80,
        CandidateSource::Contradiction => 75,
        CandidateSource::SemanticNeighborhood => 65,
        CandidateSource::Emerging => 55,
        CandidateSource::Temporal => 45,
        CandidateSource::SocialGraph => 40,
        CandidateSource::Exploration => 35,
    }
}

// Fixed FNV-1a over canonical, bounded public roots. No randomized hasher, clock,
// private graph, or new request field participates in admission priority.
pub(crate) fn request_seed(anchors: &BTreeSet<&ObjectId>, followed: &BTreeSet<&ObjectId>) -> u64 {
    let mut seed = 0xcbf29ce484222325_u64;
    for roots in [anchors, followed] {
        for id in roots {
            for byte in id.as_str().bytes().chain([0]) {
                seed = (seed ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        seed = (seed ^ 255).wrapping_mul(0x100000001b3);
    }
    seed
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_lens::{EvidenceSignals, ReputationSignals};
    use babble_types::Timestamp;
    use time::OffsetDateTime;

    #[test]
    fn scarce_capacity_follows_seeded_rotation_of_eligible_sources() {
        let ids: Vec<_> = (0..3)
            .map(|i| ObjectId::new_unchecked(format!("obj_{i:064x}")))
            .collect();
        let summaries: BTreeMap<_, _> = ids
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    ObjectSignals {
                        object_id: id.clone(),
                        created_at: Timestamp(OffsetDateTime::UNIX_EPOCH),
                        followed_author: false,
                        relevance: 0.0,
                        novelty: 0.0,
                        evidence_quality: 0.0,
                        contradiction: 0.0,
                        evidence: EvidenceSignals::default(),
                        reputation: ReputationSignals::default(),
                        temporal: 0.0,
                        exploration: 0.0,
                    },
                )
            })
            .collect();
        for seed in 0..6 {
            let mut queues = SourceQueues::new(&summaries);
            queues.extend(CandidateSource::Following, [&ids[0]]);
            queues.extend(CandidateSource::Evidence, [&ids[1]]);
            queues.extend(CandidateSource::Contradiction, [&ids[2]]);
            queues.extend(CandidateSource::SocialGraph, []);
            let selected = queues.select(1, seed);
            assert_eq!(selected[0].object_id, ids[seed as usize % 3]);
        }
    }
}
