//! Regression: `tag_counts` aggregates the tag vocabulary across ACTIVE memories —
//! every distinct tag + how many memories carry it, sorted by count desc then name asc,
//! with an optional prefix filter. This is the read side that makes write-only tags a
//! browsable vocabulary (the `list-tags` CLI / `list_tags` MCP command). It is
//! taxonomy-agnostic: no hard-coded namespaces, it reports whatever was stored.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_tag_counts -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;

fn find<'a>(v: &'a [(String, usize)], tag: &str) -> Option<usize> {
    v.iter().find(|(t, _)| t == tag).map(|(_, n)| *n)
}

#[test]
fn tag_counts_aggregates_sorts_and_prefix_filters() {
    let path = "test_tag_counts.said";
    let _ = std::fs::remove_file(path);
    let mut brain = SaidFile::create(path);
    assert!(brain.auto_load_encoder(), "embedded encoder must load");

    // Empty brain → no tags.
    assert!(brain.tag_counts(None).is_empty(), "empty brain has no tags");

    // Three memories with overlapping tags. `project:said` on all three, `status:planned`
    // on two, `status:shipped` on one. (remember_with_salience auto-adds a pillar:<name>
    // tag too — the test asserts on the tags WE set, not that pillar tag.)
    brain.remember_with_salience(Some("m1"), "First launch item.", None, Pillar::Semantic,
        vec!["project:said".into(), "status:planned".into()]);
    brain.remember_with_salience(Some("m2"), "Second launch item.", None, Pillar::Semantic,
        vec!["project:said".into(), "status:planned".into()]);
    brain.remember_with_salience(Some("m3"), "A shipped item.", None, Pillar::Semantic,
        vec!["project:said".into(), "status:shipped".into()]);
    brain.build_index().expect("build_index");

    let all = brain.tag_counts(None);

    // Counts are correct.
    assert_eq!(find(&all, "project:said"), Some(3), "project:said on all 3");
    assert_eq!(find(&all, "status:planned"), Some(2), "status:planned on 2");
    assert_eq!(find(&all, "status:shipped"), Some(1), "status:shipped on 1");

    // Sorted by count desc, then name asc. The top entries are all count-3 tags
    // (project:said plus the auto-added pillar:semantic), ordered alphabetically among
    // ties; every one of them outranks the count-2 and count-1 tags below.
    assert_eq!(all[0].1, 3, "highest-count tag ranks first");
    let planned_rank = all.iter().position(|(t, _)| t == "status:planned").unwrap();
    let said_rank = all.iter().position(|(t, _)| t == "project:said").unwrap();
    assert!(said_rank < planned_rank, "count-3 tag ranks above count-2 tag");

    // Prefix filter narrows to a namespace and nothing else leaks in.
    let status = brain.tag_counts(Some("status:"));
    assert!(status.iter().all(|(t, _)| t.starts_with("status:")),
        "prefix filter keeps only status: tags");
    assert_eq!(find(&status, "status:planned"), Some(2));
    assert!(find(&status, "project:said").is_none(), "project: tag excluded by status: prefix");

    // A prefix that matches nothing → empty (not an error).
    assert!(brain.tag_counts(Some("nope:")).is_empty(), "no match → empty");

    // Deleting a memory drops its tag contribution (active-only).
    brain.delete("m3");
    brain.build_index().ok();
    let after = brain.tag_counts(None);
    assert!(find(&after, "status:shipped").is_none(),
        "deleted memory's unique tag disappears from the vocabulary");
    assert_eq!(find(&after, "project:said"), Some(2), "remaining two still counted");

    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(format!("{}.spill", path));
}
