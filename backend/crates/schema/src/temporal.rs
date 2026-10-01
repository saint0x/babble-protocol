use babble_discovery::{NativeTemporalScorer, TemporalProvider, TemporalRequest};
use serde_json::{Value, json};

/// Native reference values for independent implementations of temporal-v1.
pub(super) fn fixtures() -> serde_json::Result<Value> {
    let mut cases = Vec::new();
    let item = |index: usize, class: &str, published: &str| {
        json!({
            "object_id": format!("obj_{index:064x}"),
            "published_at": published,
            "content_class": class,
            "quality_score": 0.5,
            "tags": [],
            "engagement": {"total_views": 0, "recent_views": 0, "total_interactions": 0, "recent_interactions": 0}
        })
    };
    let mut add = |name: &str, reference: &str, items: Vec<Value>| -> serde_json::Result<()> {
        let request: TemporalRequest =
            serde_json::from_value(json!({"reference_time": reference, "items": items}))?;
        let result = NativeTemporalScorer
            .score(&request)
            .map_err(super::serde_error)?;
        cases.push(json!({"name": name, "request": request, "result": result}));
        Ok(())
    };
    let reference = "2026-09-30T00:00:00Z";
    add("empty", reference, vec![])?;
    add(
        "all-classes",
        reference,
        ["news", "discussion", "analysis", "tutorial", "reference"]
            .iter()
            .enumerate()
            .map(|(index, class)| item(index, class, "2026-09-29T12:00:00Z"))
            .collect(),
    )?;
    for (name, published) in [
        ("zero-age", reference),
        ("two-hours", "2026-09-29T22:00:00Z"),
        (
            "two-hours-plus-nanosecond",
            "2026-09-29T21:59:59.999999999Z",
        ),
        ("one-day", "2026-09-29T00:00:00Z"),
        ("three-days", "2026-09-27T00:00:00Z"),
        ("seven-days", "2026-09-23T00:00:00Z"),
        ("thirty-days", "2026-08-31T00:00:00Z"),
        ("ancient", "0000-01-01T00:00:00+23:59"),
        ("future", "9999-12-31T23:59:59.999999999-23:59"),
    ] {
        add(name, reference, vec![item(0, "discussion", published)])?;
    }
    let mut mixed = item(0, "reference", "2026-09-29T00:00:00Z");
    mixed["tags"] = json!(["BREAKING", "evergreen", "reference"]);
    mixed["quality_score"] = json!(1.0);
    mixed["engagement"] = json!({"total_views": 100, "recent_views": 25, "total_interactions": 8, "recent_interactions": 2});
    add("tag-and-activity-components", reference, vec![mixed])?;
    let mut public = item(0, "analysis", "2026-09-29T00:00:00Z");
    public["engagement"] = json!({"total_views": 0, "recent_views": 0, "total_interactions": 4, "recent_interactions": 3});
    add(
        "public-relationships-without-views",
        reference,
        vec![public],
    )?;
    Ok(json!({"model": "temporal-v1", "version": "1", "cases": cases}))
}
