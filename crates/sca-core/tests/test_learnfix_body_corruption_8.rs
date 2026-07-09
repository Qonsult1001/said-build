#![cfg(all(feature = "embed-model", feature = "code"))]
//! REGRESSION GUARD for FIXES-LOG #8: on a LARGE block-compacted brain, a 2nd learn_coding_fix must NOT
//! blank the 1st fix's body. Reproduces ONLY at scale (the `said init` block-dict save path); small
//! in-process brains do not trigger it. The test indexes a real directory to reach the block path.
//!
//! REGRESSION GUARD (FIXES-LOG #8, FIXED): runs in CI to catch any re-regression.
//!   cargo test -p sca-core --no-default-features --features "embed-model,code" \
//!     --test test_learnfix_body_corruption_8 -- --ignored --nocapture
use sca_core::said_file::SaidFile;
fn blen(b: &mut SaidFile, id: &str) -> usize { b.get(id).map(|s| s.len()).unwrap_or(0) }

#[test]
fn second_learnfix_must_not_blank_first_body_on_block_compacted_brain() {
    let dir = std::env::temp_dir().join(format!("said_8_{}", std::process::id()));
    let src = dir.join("src"); std::fs::create_dir_all(&src).unwrap();
    // enough real code to trigger block compaction (the failing path)
    for i in 0..60 {
        std::fs::write(src.join(format!("m{i}.rs")),
            format!("/// handler {i}\npub fn handler_{i}(r: Req) -> Res {{ validate(r); dispatch(r) }}\n")).unwrap();
    }
    let p = dir.join("b.said");
    let mut b = SaidFile::create(p.to_string_lossy().as_ref());
    assert!(b.auto_load_encoder());
    // index the dir the way `said init` does (AST chunks → block path)
    // (use add_dir-equivalent: remember each file's content)
    for i in 0..60 {
        let c = std::fs::read_to_string(src.join(format!("m{i}.rs"))).unwrap();
        b.remember_as(&format!("src/m{i}.rs::handler_{i}::function:1"), &c, None);
    }
    b.build_index().unwrap();
    b.compact();          // force the block-dict path (what init produces)
    b.save().unwrap();

    let id1 = sca_core::ask::learn_coding_fix(&mut b, "first unique caching problem",
        "BODY ONE: long enough to verify retrievability after later writes.", "[]", Some("fix-one"));
    b.save().unwrap();
    assert!(blen(&mut b, &id1) > 40, "fix#1 body present after store (got {})", blen(&mut b, &id1));

    let id2 = sca_core::ask::learn_coding_fix(&mut b, "second different parsing problem",
        "BODY TWO: another long enough body.", "[]", Some("fix-two"));
    b.save().unwrap();

    let f1 = blen(&mut b, &id1); let f2 = blen(&mut b, &id2);
    eprintln!("fix#1={f1} fix#2={f2}");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(f2 > 40, "fix#2 body present (got {f2})");
    assert!(f1 > 40, "REGRESSION #8: fix#1 body BLANKED by the 2nd learn_fix (got {f1})");
}
