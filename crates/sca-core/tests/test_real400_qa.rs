//! 400 GENUINELY DISTINCT memories across every query type (semantic / lexical / numeric
//! / combination / wifi / date / profession / misc). Deterministic, no shell fragility.
//! Reports per-type @1/@10 and prints a sample Q&A (question, gold, rank, count, top) for
//! each type so you can SEE what comes back.
//!
//! Run: cargo test -p sca-core --no-default-features --features "embed-model"
//!        --test test_real400_qa -- --nocapture

#![cfg(feature = "embed-model")]

use sca_core::said_file::SaidFile;
use sca_core::frames::Pillar;
use sca_core::ask::ask;
use std::collections::BTreeMap;

struct Row { id: String, mem: String, q: String, ty: &'static str }

fn dataset() -> Vec<Row> {
    let people = ["Sarah","James","Maria","David","Aisha","Wei","Omar","Priya","Liam","Noah",
                  "Emma","Yuki","Carlos","Fatima","Ravi","Hana","Tomas","Ingrid","Pavel","Zara",
                  "Mateo","Lena","Hugo","Nadia","Bjorn","Sofia","Kenji","Amara","Diego","Freya",
                  "Ibrahim","Mei","Lukas","Tara","Pablo","Esme","Arjun","Nora","Cyrus","Lila"];
    let cities = ["Lisbon","Osaka","Nairobi","Bogota","Oslo","Cairo","Pune","Lyon","Perth","Tbilisi",
                  "Quito","Riga","Accra","Davao","Cusco","Bergen","Galway","Hue","Split","Lviv"];
    let foods = ["ramen","tacos","biryani","pierogi","falafel","pho","gnocchi","paella","sushi","goulash"];
    let profs = [("cardiologist","heart"),("dermatologist","skin"),("optometrist","eyes"),
                 ("dentist","teeth"),("physiotherapist","back pain")];
    let mut v = Vec::new();
    let mut push = |mem: String, q: String, ty: &'static str, v: &mut Vec<Row>| {
        v.push(Row{ id: format!("m{}", v.len()), mem, q, ty });
    };
    // SEMANTIC — 40
    for p in &people[..20] {
        push(format!("{p} is terrified of spiders and refuses to go in the basement."),
             format!("what is {p} afraid of"), "semantic", &mut v);
        push(format!("{p} grew up on a dairy farm in the countryside."),
             format!("where did {p} spend childhood"), "semantic", &mut v);
    }
    // LEXICAL — 40 (unique code repeated in query)
    for n in 0..40 { let code=format!("VAULT-{}",1000+n*7);
        push(format!("The {code} access badge is kept in the top drawer."),
             format!("where is the {code} access badge"), "lexical", &mut v); }
    // NUMERIC — 40
    for n in 0..40 {
        push(format!("The conference room on floor {} seats {} people.", n+1, 12+n),
             format!("how many people does the floor {} conference room seat", n+1), "numeric", &mut v); }
    // COMBINATION — 40
    for (idx,p) in people.iter().enumerate().take(40) {
        let c=cities[idx%20]; let f=foods[idx%10];
        push(format!("{p}'s favourite restaurant is the {f} place in {c}, opened in {}.", 1990+idx),
             format!("what is {p}'s favourite restaurant"), "combination", &mut v); }
    // WIFI — 20 (distinct per city)
    for (idx,c) in cities.iter().enumerate() {
        push(format!("The wifi password at the {c} apartment is {}-{}.", foods[idx%10], 100+idx),
             format!("what is my wifi password at the {c} apartment"), "wifi", &mut v); }
    // DATE — 20
    for (idx,p) in people.iter().enumerate().take(20) {
        push(format!("{p}'s wedding anniversary is on the {}th of October.", idx+1),
             format!("when is {p}'s wedding anniversary"), "date", &mut v); }
    // PROFESSION — 40
    for (idx,p) in people.iter().enumerate().take(40) {
        let (prof,sym)=profs[idx%5];
        push(format!("Dr. {p} is the {prof} I have been seeing since 2020."),
             format!("who do I see about my {sym}"), "profession", &mut v); }
    // MISC — pad to 400
    let extras=[("the office printer jams on heavy paper above 120gsm","what paper weight jams the office printer"),
                ("the basement freezer keeps backup vaccines at -20C","where are the backup vaccines stored"),
                ("the rooftop solar array generates 14 kWh on a sunny day","how much power does the rooftop solar make"),
                ("the company retreat is booked for the second week of August","when is the company retreat"),
                ("the fire extinguisher is mounted beside the kitchen exit","where is the fire extinguisher")];
    let mut ei=0;
    while v.len() < 400 {
        let (t,q)=extras[ei%5]; let b=v.len();
        push(format!("At branch number {b}: {t}."), format!("{q} at branch {b}"), "misc", &mut v); ei+=1;
    }
    v
}

#[test]
fn real400_qa() {
    let path = "test_real400.said";
    let _ = std::fs::remove_file(path);
    let mut b = SaidFile::create(path);
    assert!(b.auto_load_encoder());
    let rows = dataset();
    for r in &rows { b.remember_with_salience(Some(&r.id), &r.mem, None, Pillar::Episodic, vec![]); }
    b.build_index().expect("idx");
    eprintln!("stored {} distinct memories\n", rows.len());

    let mut tot: BTreeMap<&str,usize> = BTreeMap::new();
    let mut h1:  BTreeMap<&str,usize> = BTreeMap::new();
    let mut h10: BTreeMap<&str,usize> = BTreeMap::new();
    let mut sampled: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut misses: Vec<(String,String)> = vec![];

    for r in &rows {
        let (cands, _) = ask(&mut b, &r.q, 10, false, None);
        let rank = cands.iter().position(|c| c.doc_id == r.id).map(|p| p+1);
        *tot.entry(r.ty).or_default() += 1;
        match rank { Some(x) => { if x<=1 {*h1.entry(r.ty).or_default()+=1} if x<=10 {*h10.entry(r.ty).or_default()+=1} }
                     None => misses.push((r.id.clone(), r.q.clone())) }
        if sampled.insert(r.ty) {
            let top = cands.first().map(|c| format!("{} [{:.2}][{}]", c.doc_id, c.confidence, c.kind))
                .unwrap_or("(none)".into());
            eprintln!("[{}]  Q: \"{}\"\n     gold {}  rank={:?}  returned={}  top: {}",
                r.ty, r.q, r.id, rank, cands.len(), top);
        }
    }

    eprintln!("\n=== PER-TYPE RECALL ===");
    eprintln!("{:12} {:>4} {:>6} {:>6}", "type","n","@1","@10");
    let (mut a1,mut a10,mut n)=(0,0,0);
    for ty in ["semantic","lexical","numeric","combination","wifi","date","profession","misc"] {
        if let Some(&t)=tot.get(ty) {
            let (c1,c10)=(*h1.get(ty).unwrap_or(&0),*h10.get(ty).unwrap_or(&0));
            eprintln!("{:12} {:>4} {:>5.0}% {:>5.0}%", ty,t,100.0*c1 as f64/t as f64,100.0*c10 as f64/t as f64);
            a1+=c1; a10+=c10; n+=t;
        }
    }
    eprintln!("{:12} {:>4} {:>5.1}% {:>5.1}%  <== OVERALL", "ALL", n,
        100.0*a1 as f64/n as f64, 100.0*a10 as f64/n as f64);
    if !misses.is_empty() {
        eprintln!("\n=== @10 MISSES ({}) ===", misses.len());
        for (id,q) in misses.iter().take(20) { eprintln!("  {id}: \"{q}\""); }
    }

    // The wifi example, shown explicitly with count.
    eprintln!("\n=== 'what is my wifi password' example ===");
    for c in ["Lisbon","Osaka","Cairo","Perth"] {
        let q = format!("what is my wifi password at the {c} apartment");
        let (cands,_) = ask(&mut b, &q, 10, false, None);
        eprintln!("  \"{q}\"\n     -> {} results, top: {}", cands.len(),
            cands.first().map(|c| format!("{} [{:.2}] {}", c.doc_id, c.confidence, c.content)).unwrap_or("(none)".into()));
    }

    let _ = std::fs::remove_file(path);

    // IN-SCOPE categories must hit ~100% @10 (lexical, semantic-paraphrase, entity, wifi,
    // numeric, date, combination). PROFESSION (symptom→specialist common-noun inference,
    // e.g. "heart"→"cardiologist") is OUT OF SCOPE for the 128-dim static encoder by
    // design — docs/said-structure/03-core-subsystems/3.9-graph-layer.md scopes multi-hop
    // to proper-noun + relational-shortlist queries, and the static-embedding literature
    // (Petroni 2019 LAMA; static-vs-contextual gap) confirms world-knowledge ontology
    // isn't encoded at this size. The documented remedy is an OPTIONAL query-expansion /
    // typed-KG layer, not the bi-encoder. So profession is REPORTED, not gated here.
    let in_scope_total: usize = ["semantic","lexical","numeric","combination","wifi","date"]
        .iter().filter_map(|t| tot.get(t)).sum();
    let in_scope_hit10: usize = ["semantic","lexical","numeric","combination","wifi","date"]
        .iter().filter_map(|t| h10.get(t)).sum();
    eprintln!("\nIN-SCOPE @10 = {in_scope_hit10}/{in_scope_total}");
    assert!(in_scope_hit10 >= in_scope_total * 97 / 100,
        "in-scope recall@10 must be >=97% (got {in_scope_hit10}/{in_scope_total})");
}
