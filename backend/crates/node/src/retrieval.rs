//! Rebuilt from committed public Objects. Admission precedes provider enrichment.
use super::*;
use babel_lens::{Candidate, CandidateSource, CandidateSourceContribution};
use std::cmp::Reverse;

mod search;

#[derive(Default)]
pub(crate) struct DiscoveryIndex {
    documents: BTreeMap<ObjectId, Document>,
    recent: BTreeSet<(Reverse<Timestamp>, ObjectId)>,
    exploration: BTreeSet<(u64, ObjectId)>,
    grams: BTreeMap<Vec<u8>, BTreeSet<ObjectId>>,
    authors: BTreeMap<IdentityId, usize>,
    author_sizes: BTreeMap<usize, usize>,
}

struct Document {
    text: String,
    author: IdentityId,
    kind: String,
    created_at: Timestamp,
}

pub(super) struct Population {
    counts: BTreeMap<IdentityId, usize>,
    pub max_author_objects: usize,
    pub total: usize,
}

impl Population {
    pub fn author_count(&self, author: &IdentityId) -> usize {
        self.counts.get(author).copied().unwrap_or_default()
    }
}

impl DiscoveryIndex {
    pub fn insert(&mut self, object: &Object) {
        // Objects are immutable; repeated imports must not increase corpus counts.
        if self.documents.contains_key(&object.id) {
            return;
        }
        let text = searchable_object_text(object);
        for width in 1..=3 {
            for gram in text.as_bytes().windows(width).collect::<BTreeSet<_>>() {
                self.grams
                    .entry(gram.to_vec())
                    .or_default()
                    .insert(object.id.clone());
            }
        }
        let count = self.authors.entry(object.author.clone()).or_default();
        if *count > 0 {
            let bucket = self.author_sizes.get_mut(count).expect("indexed author");
            *bucket -= 1;
            if *bucket == 0 {
                self.author_sizes.remove(count);
            }
        }
        *count += 1;
        *self.author_sizes.entry(*count).or_default() += 1;
        self.recent
            .insert((Reverse(object.created_at), object.id.clone()));
        self.exploration
            .insert((stable_hash(object.id.as_str().bytes()), object.id.clone()));
        self.documents.insert(
            object.id.clone(),
            Document {
                text,
                author: object.author.clone(),
                kind: object.kind.as_str().into(),
                created_at: object.created_at,
            },
        );
    }

    pub fn population(&self, selected: &[Object], restricted: &BTreeSet<ObjectId>) -> Population {
        let mut removed = BTreeMap::<&IdentityId, usize>::new();
        for id in restricted {
            if let Some(doc) = self.documents.get(id) {
                *removed.entry(&doc.author).or_default() += 1;
            }
        }
        let mut deltas = BTreeMap::<usize, isize>::new();
        for (author, count) in &removed {
            let old = self.authors[*author];
            *deltas.entry(old).or_default() -= 1;
            *deltas.entry(old - count).or_default() += 1;
        }
        let populated = |size: &usize| {
            self.author_sizes.get(size).copied().unwrap_or(0) as isize
                + deltas.get(size).copied().unwrap_or(0)
                > 0
        };
        let max_author_objects = self
            .author_sizes
            .keys()
            .rev()
            .find(|size| populated(size))
            .into_iter()
            .chain(deltas.keys().rev().find(|size| populated(size)))
            .copied()
            .max()
            .unwrap_or(1);
        Population {
            counts: selected
                .iter()
                .map(|object| {
                    (
                        object.author.clone(),
                        self.authors[&object.author]
                            - removed.get(&object.author).copied().unwrap_or(0),
                    )
                })
                .collect(),
            max_author_objects,
            total: self.documents.len() - removed.values().sum::<usize>(),
        }
    }
}

const NEIGHBOR_RELATIONS: &[Relation] = &[
    Relation::References,
    Relation::Cites,
    Relation::Quotes,
    Relation::Extends,
    Relation::DerivesFrom,
    Relation::Supersedes,
    Relation::Forks,
    Relation::Remixes,
];
const BUDGET: usize = babel_discovery::MAX_CANDIDATES;

struct Source {
    kind: CandidateSource,
    ids: Vec<ObjectId>,
}

pub(super) struct Admission {
    pub members: BTreeMap<ObjectId, Vec<CandidateSource>>,
}

impl Admission {
    pub fn candidates(
        &self,
        summaries: &BTreeMap<ObjectId, babel_discovery::ObjectSignals>,
    ) -> Vec<Candidate> {
        self.members
            .iter()
            .map(|(id, sources)| {
                let mut candidate = summaries[id].candidate(sources[0].clone());
                candidate.sources = sources
                    .iter()
                    .map(|source| CandidateSourceContribution {
                        source: source.clone(),
                        weight: 1.0,
                    })
                    .collect();
                candidate
            })
            .collect()
    }
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub(super) fn retrieve(
        &self,
        query: &DiscoveryQuery,
        restricted: &BTreeSet<ObjectId>,
        matches: Option<&[Object]>,
        exploration_slots: usize,
    ) -> Admission {
        let eligible_search =
            matches.map(|objects| objects.iter().map(|o| &o.id).collect::<BTreeSet<_>>());
        let eligible = |id: &ObjectId| {
            self.discovery_index.documents.contains_key(id)
                && !restricted.contains(id)
                && eligible_search.as_ref().is_none_or(|ids| ids.contains(id))
        };
        let anchors: BTreeSet<_> = query
            .anchors
            .iter()
            .filter(|id| !restricted.contains(*id))
            .collect();
        let followed: BTreeSet<_> = query
            .followed_objects
            .iter()
            .filter(|id| !restricted.contains(*id))
            .collect();
        let graph = self.state.graph();
        let neighbors =
            |roots: &BTreeSet<&ObjectId>, relations: &[Relation], directions: &[bool]| {
                roots
                    .iter()
                    .flat_map(|root| {
                        relations.iter().flat_map(|relation| {
                            directions.iter().filter_map(|incoming| {
                                graph.relation_neighbors(root, relation, *incoming)
                            })
                        })
                    })
                    .collect::<Vec<_>>()
            };
        let social_roots = anchors.union(&followed).copied().collect();
        let social = neighbors(&social_roots, &[Relation::Follows], &[false, true]);
        let evidence = neighbors(&anchors, &[Relation::EvidenceFor], &[true]);
        let contradiction = neighbors(&anchors, &[Relation::EvidenceAgainst], &[true]);
        let neighborhood = neighbors(&anchors, NEIGHBOR_RELATIONS, &[false, true]);
        // Fair across roots/relations as well as sources; no high-degree root is
        // materialized, and duplicate/restricted entries consume no admission slot.
        let mut sources = vec![
            Source {
                kind: CandidateSource::Following,
                ids: followed
                    .iter()
                    .filter(|id| eligible(id))
                    .map(|id| (*id).clone())
                    .collect(),
            },
            Source {
                kind: CandidateSource::Evidence,
                ids: merge_neighbors(&evidence, &eligible),
            },
            Source {
                kind: CandidateSource::Contradiction,
                ids: merge_neighbors(&contradiction, &eligible),
            },
            Source {
                kind: CandidateSource::SemanticNeighborhood,
                ids: merge_neighbors(&neighborhood, &eligible),
            },
            Source {
                kind: CandidateSource::Temporal,
                ids: anchors
                    .iter()
                    .filter(|id| eligible(id))
                    .map(|id| (*id).clone())
                    .collect(),
            },
            Source {
                kind: CandidateSource::SocialGraph,
                ids: merge_neighbors(&social, &eligible),
            },
        ];
        let recent: Vec<_> = if let Some(objects) = matches {
            let mut positions: Vec<_> = objects.iter().filter(|o| eligible(&o.id)).collect();
            positions.sort_by_key(|o| (Reverse(o.created_at), &o.id));
            positions.into_iter().map(|o| o.id.clone()).collect()
        } else {
            self.discovery_index
                .recent
                .iter()
                .map(|(_, id)| id)
                .filter(|id| eligible(id))
                .take(BUDGET)
                .cloned()
                .collect()
        };
        sources.push(Source {
            kind: CandidateSource::Temporal,
            ids: recent,
        });
        sources.push(Source {
            kind: CandidateSource::SocialGraph,
            ids: graph
                .active_objects()
                .filter(|id| eligible(id))
                .take(BUDGET)
                .cloned()
                .collect(),
        });
        let seed = stable_hash(
            anchors
                .iter()
                .chain(&followed)
                .flat_map(|id| id.as_str().bytes().chain([0])),
        );
        let exploration: Vec<_> = if let Some(objects) = matches {
            let mut positions: Vec<_> = objects
                .iter()
                .filter(|o| eligible(&o.id))
                .map(|o| (stable_hash(o.id.as_str().bytes()).wrapping_sub(seed), &o.id))
                .collect();
            positions.sort();
            positions
                .into_iter()
                .take(exploration_slots)
                .map(|(_, id)| id.clone())
                .collect()
        } else {
            let start = (seed, ObjectId::new_unchecked(String::new()));
            self.discovery_index
                .exploration
                .range(start.clone()..)
                .chain(self.discovery_index.exploration.range(..start))
                .map(|(_, id)| id)
                .filter(|id| eligible(id))
                .take(exploration_slots)
                .cloned()
                .collect()
        };
        sources.push(Source {
            kind: CandidateSource::Exploration,
            ids: exploration,
        });
        let members = select_sources(&sources, seed)
            .into_iter()
            .map(|id| {
                // Membership probes include graph contributions beyond each bounded
                // source prefix, so deduplication never drops applicable provenance.
                let mut provenance = Vec::new();
                for source in &sources {
                    let present = match source.kind {
                        CandidateSource::Evidence => evidence.iter().any(|set| set.contains(&id)),
                        CandidateSource::Contradiction => {
                            contradiction.iter().any(|set| set.contains(&id))
                        }
                        CandidateSource::SemanticNeighborhood => {
                            neighborhood.iter().any(|set| set.contains(&id))
                        }
                        CandidateSource::SocialGraph => {
                            social.iter().any(|set| set.contains(&id))
                                || graph.has_public_activity(&id)
                        }
                        _ => source.ids.contains(&id),
                    };
                    if present && !provenance.contains(&source.kind) {
                        provenance.push(source.kind.clone());
                    }
                }
                (id, provenance)
            })
            .collect();
        Admission { members }
    }
}

fn select_sources(sources: &[Source], seed: u64) -> BTreeSet<ObjectId> {
    let mut selected = BTreeSet::new();
    let mut cursors: Vec<_> = sources
        .iter()
        .filter(|s| !s.ids.is_empty() && s.kind != CandidateSource::Exploration)
        .map(|s| s.ids.iter())
        .collect();
    let exploration = sources
        .iter()
        .find(|s| s.kind == CandidateSource::Exploration);
    if let Some(source) = exploration {
        // Reserve one turn per other queue even when exploration requests 200.
        selected.extend(
            source
                .ids
                .iter()
                .take(BUDGET.saturating_sub(cursors.len()))
                .cloned(),
        );
    }
    if !cursors.is_empty() {
        let start = (seed % cursors.len() as u64) as usize;
        cursors.rotate_left(start);
    }
    while selected.len() < BUDGET {
        let before = selected.len();
        for cursor in &mut cursors {
            if selected.len() == BUDGET {
                break;
            }
            for id in cursor.by_ref() {
                if selected.insert(id.clone()) {
                    break;
                }
            }
        }
        if before == selected.len() {
            break;
        }
    }
    // Return unused capacity to exploration after the other queues have run.
    if let Some(source) = exploration {
        for id in &source.ids {
            if selected.len() == BUDGET {
                break;
            }
            selected.insert(id.clone());
        }
    }
    selected
}

fn merge_neighbors(
    sets: &[&BTreeSet<ObjectId>],
    eligible: &impl Fn(&ObjectId) -> bool,
) -> Vec<ObjectId> {
    let mut cursors: Vec<_> = sets.iter().map(|set| set.iter()).collect();
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    while ids.len() < BUDGET {
        let before = ids.len();
        for cursor in &mut cursors {
            if ids.len() == BUDGET {
                break;
            }
            for id in cursor.by_ref() {
                if eligible(id) && seen.insert(id) {
                    ids.push(id.clone());
                    break;
                }
            }
        }
        if before == ids.len() {
            break;
        }
    }
    ids
}

fn stable_hash(bytes: impl Iterator<Item = u8>) -> u64 {
    bytes.fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximum_exploration_reserves_every_other_source_for_every_rotation() {
        let ids: Vec<_> = (0..207)
            .map(|i| ObjectId::new_unchecked(format!("obj_{i:064x}")))
            .collect();
        let mut sources: Vec<_> = [
            CandidateSource::Following,
            CandidateSource::Evidence,
            CandidateSource::Contradiction,
            CandidateSource::SemanticNeighborhood,
            CandidateSource::Temporal,
            CandidateSource::SocialGraph,
            CandidateSource::Temporal,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| Source {
            kind,
            ids: vec![ids[i].clone()],
        })
        .collect();
        sources.push(Source {
            kind: CandidateSource::Exploration,
            ids: ids[7..].to_vec(),
        });
        for seed in 0..64 {
            let selected = select_sources(&sources, seed);
            assert_eq!(selected.len(), 200);
            assert!(ids[..7].iter().all(|id| selected.contains(id)));
        }
        // Exhausted or overlapping sources return their unused reservation.
        sources[0].ids = vec![ids[7].clone()];
        sources[1].ids.clear();
        assert_eq!(select_sources(&sources, 0).len(), 200);
    }
}
