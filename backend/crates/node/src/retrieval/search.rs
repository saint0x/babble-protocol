//! Byte n-grams preserve substring semantics, including short/non-ASCII queries.
use super::*;

impl DiscoveryIndex {
    pub(crate) fn search(
        &self,
        query: &ObjectSearchQuery,
        restricted: &BTreeSet<ObjectId>,
    ) -> Vec<(ObjectId, u64, Vec<String>)> {
        let limit = if query.limit == 0 {
            50
        } else {
            query.limit.min(BUDGET)
        };
        let phrase = query
            .query
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);
        let terms = query.query.as_deref().map(search_terms).unwrap_or_default();
        let eligible = |id: &ObjectId| {
            let doc = &self.documents[id];
            !restricted.contains(id)
                && query.author.as_ref().is_none_or(|a| a == &doc.author)
                && query.kind.as_ref().is_none_or(|k| k == &doc.kind)
        };
        if phrase.is_none() {
            return self
                .recent
                .iter()
                .map(|(_, id)| id)
                .filter(|id| eligible(id))
                .take(limit)
                .map(|id| (id.clone(), 1, vec!["unfiltered".into()]))
                .collect();
        }
        let mut matches = BTreeSet::new();
        for needle in phrase.iter().chain(&terms) {
            let width = needle.len().min(3);
            let postings: Option<Vec<_>> = needle
                .as_bytes()
                .windows(width)
                .map(|gram| self.grams.get(gram))
                .collect();
            let Some(postings) = postings else {
                continue;
            };
            if let Some(smallest) = postings.iter().min_by_key(|ids| ids.len()) {
                for id in *smallest {
                    if eligible(id)
                        && postings.iter().all(|ids| ids.contains(id))
                        && self.documents[id].text.contains(needle)
                    {
                        matches.insert(id);
                    }
                }
            }
        }
        // Only matching index documents are scored. Keep bounded winners and
        // clone public Object payloads after ranking, never for the whole store.
        let mut best = Vec::<(ObjectId, u64, Vec<String>)>::new();
        for id in matches {
            let (score, reasons) =
                score_search_text(&self.documents[id].text, phrase.as_deref(), &terms);
            let position = best.partition_point(|(current, current_score, _)| {
                current_score
                    .cmp(&score)
                    .reverse()
                    .then_with(|| {
                        self.documents[id]
                            .created_at
                            .cmp(&self.documents[current].created_at)
                    })
                    .then_with(|| current.cmp(id))
                    .is_lt()
            });
            if position < limit {
                best.insert(position, (id.clone(), score, reasons));
                best.truncate(limit);
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_substring_search_matches_full_scan_oracle_for_generated_unicode_queries() {
        let key = Keypair::from_ed25519_secret_hex(&"ab".repeat(32)).unwrap();
        let author =
            Identity::create(babel_identity::IdentityKind::Person, "index-author", &key).unwrap();
        let mut index = DiscoveryIndex::default();
        let mut objects = Vec::new();
        let mut seed = 290930_u64;
        let words = [
            "alpha",
            "ALPHA",
            "alphabet",
            "beta",
            "b",
            "\u{6771}\u{4eac}",
            "caf\u{e9}",
            "CAF\u{c9}",
            "?",
            "a_b",
        ];
        for n in 0..350 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut object = Object::text(
                &author,
                &format!(
                    "{} {} {}",
                    words[seed as usize % words.len()],
                    words[(seed >> 12) as usize % words.len()],
                    n
                ),
            )
            .unwrap();
            object.id = ObjectId::new_unchecked(format!("obj_{n:064x}"));
            object.created_at =
                Timestamp(time::OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(n));
            index.insert(&object);
            index.insert(&object);
            objects.push(object);
        }
        let restricted: BTreeSet<_> = objects.iter().step_by(7).map(|o| o.id.clone()).collect();
        let mut queries: Vec<String> = ["", "  ", "alpha beta", "absent", "?", "obj_", "schema"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        for word in words {
            for (start, _) in word.char_indices() {
                queries.push(word[start..].into());
            }
        }
        for query in queries {
            for limit in [1, 9, 200] {
                let phrase = (!query.trim().is_empty()).then(|| query.trim().to_ascii_lowercase());
                let terms = search_terms(&query);
                let mut expected = objects
                    .iter()
                    .filter(|o| !restricted.contains(&o.id))
                    .filter_map(|o| {
                        let (score, reasons) = score_search_text(
                            &searchable_object_text(o),
                            phrase.as_deref(),
                            &terms,
                        );
                        (score > 0).then_some((o.id.clone(), score, reasons))
                    })
                    .collect::<Vec<_>>();
                expected.sort_by(|a, b| {
                    b.1.cmp(&a.1)
                        .then_with(|| {
                            index.documents[&b.0]
                                .created_at
                                .cmp(&index.documents[&a.0].created_at)
                        })
                        .then_with(|| a.0.cmp(&b.0))
                });
                expected.truncate(limit);
                assert_eq!(
                    index.search(
                        &ObjectSearchQuery {
                            query: Some(query.clone()),
                            author: Some(author.id.clone()),
                            kind: Some(babel_object::ObjectKind::text().as_str().into()),
                            limit
                        },
                        &restricted
                    ),
                    expected,
                    "query={query:?}, limit={limit}"
                );
            }
        }
        assert_eq!(index.authors[&author.id], 350);
        let population = index.population(&objects[..1], &restricted);
        assert_eq!(population.total, 300);
        assert_eq!(population.max_author_objects, 300);
    }
}
