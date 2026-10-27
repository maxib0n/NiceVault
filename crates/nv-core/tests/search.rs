use nv_core::{Filters, KdfPreset, KindGroup, SearchPage, SearchQuery, Sort, TagError, Vault};
use nv_fs::{Fs, Kind, Node, NodeId, ROOT};
use secrecy::SecretString;
use std::path::Path;
use std::time::Instant;

fn pw() -> SecretString {
    SecretString::from("pw".to_owned())
}

fn q(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), limit: 100, ..Default::default() }
}

fn names(p: &SearchPage) -> Vec<String> {
    p.hits.iter().map(|h| h.node.name.clone()).collect()
}

fn find(v: &Vault, text: &str) -> Vec<String> {
    names(&v.search(&q(text)).unwrap())
}

fn mk(v: &Vault, parent: NodeId, name: &str, kind: Kind) -> Node {
    v.create(parent, name, kind).unwrap()
}

fn file(v: &Vault, parent: NodeId, name: &str, len: usize) -> Node {
    let f = mk(v, parent, name, Kind::File);
    if len > 0 {
        v.write(f.id, 0, &vec![7u8; len]).unwrap();
    }
    f
}

fn sample(dir: &Path) -> (Vault, Vec<Node>) {
    let v = Vault::create(&dir.join("s.nvault"), &pw(), KdfPreset::Fast).unwrap();
    let foto = mk(&v, ROOT, "Foto", Kind::Dir);
    let mare = mk(&v, foto.id, "Mare", Kind::Dir);
    let estate = mk(&v, foto.id, "Estate 2024", Kind::Dir);
    let docs = mk(&v, ROOT, "Documenti", Kind::Dir);
    let out = vec![
        file(&v, mare.id, "Spiaggia.jpg", 3000),
        file(&v, mare.id, "Città al mare.JPG", 5000),
        file(&v, mare.id, "amarena.png", 100),
        file(&v, estate.id, "mare.jpg", 2000),
        file(&v, docs.id, "Bolletta luce.pdf", 40_000),
        file(&v, docs.id, "note.md", 10),
        mk(&v, ROOT, "Mare", Kind::Dir),
        file(&v, ROOT, "clip.mkv", 1_000_000),
        foto,
        mare,
        estate,
        docs,
    ];
    v.flush().unwrap();
    (v, out)
}

#[test]
fn finds_names_ranked_with_paths_and_highlights() {
    let t = tempfile::tempdir().unwrap();
    let (v, _) = sample(t.path());
    let p = v.search(&q("MARE")).unwrap();
    assert_eq!(names(&p), vec!["Mare", "Mare", "mare.jpg", "Città al mare.JPG", "amarena.png"]);
    assert_eq!(p.total, 5);
    assert_eq!(p.next_offset, None);
    let city = p.hits.iter().find(|h| h.node.name.starts_with("Città")).unwrap();
    assert_eq!(city.path, "/Foto/Mare");
    assert_eq!(city.highlights, vec![[9, 13]]);
    assert_eq!(p.hits.iter().find(|h| h.node.name == "amarena.png").unwrap().highlights, vec![[1, 5]]);
    assert_eq!(p.hits.iter().filter(|h| h.node.name == "Mare").map(|h| h.path.as_str()).collect::<Vec<_>>(), vec!["/Foto", "/"]);

    assert_eq!(find(&v, "citta"), vec!["Città al mare.JPG"]);
    assert_eq!(find(&v, "CITTÀ"), vec!["Città al mare.JPG"]);
    assert_eq!(find(&v, "mare jpg"), vec!["mare.jpg", "Città al mare.JPG"]);
    assert_eq!(find(&v, "\"al mare\""), vec!["Città al mare.JPG"]);
    assert_eq!(find(&v, "\"mare al\""), Vec::<String>::new());
    assert_eq!(find(&v, "zzz"), Vec::<String>::new());
    assert_eq!(find(&v, "olle"), vec!["Bolletta luce.pdf"]);
    assert_eq!(v.search(&q("a")).unwrap().total, 8);
    let all = find(&v, "");
    assert_eq!(all.len(), 12);
    assert_eq!(&all[..5], &["Documenti", "Estate 2024", "Foto", "Mare", "Mare"]);
}

#[test]
fn filters_order_and_pages() {
    let t = tempfile::tempdir().unwrap();
    let (v, nodes) = sample(t.path());
    let with = |text: &str, f: Filters| names(&v.search(&SearchQuery { text: text.into(), filters: f, limit: 100, ..Default::default() }).unwrap());
    assert_eq!(with("mare", Filters { kinds: vec![KindGroup::Photos], ..Default::default() }), vec!["mare.jpg", "Città al mare.JPG", "amarena.png"]);
    assert_eq!(with("mare", Filters { kinds: vec![KindGroup::Folders], ..Default::default() }), vec!["Mare", "Mare"]);
    assert_eq!(with("", Filters { kinds: vec![KindGroup::Videos, KindGroup::Documents], ..Default::default() }), vec!["Bolletta luce.pdf", "clip.mkv", "note.md"]);
    assert_eq!(with("", Filters { min_size: Some(2000), max_size: Some(5000), ..Default::default() }), vec!["Città al mare.JPG", "mare.jpg", "Spiaggia.jpg"]);
    let foto = nodes.iter().find(|n| n.name == "Foto").unwrap().id;
    let mare_dir = nodes.iter().find(|n| n.name == "Mare" && n.parent == foto).unwrap().id;
    assert_eq!(with("mare", Filters { within: Some(foto), ..Default::default() }), vec!["Mare", "mare.jpg", "Città al mare.JPG", "amarena.png"]);
    assert_eq!(with("", Filters { within: Some(mare_dir), ..Default::default() }), vec!["amarena.png", "Città al mare.JPG", "Spiaggia.jpg"]);
    let now = v.getattr(nodes[0].id).unwrap().mtime;
    assert_eq!(with("", Filters { modified_before: Some(now - 1_000_000_000), ..Default::default() }).len(), 0);
    let old = nodes.iter().find(|n| n.name == "note.md").unwrap().id;
    v.set_mtime(old, 1_000).unwrap();
    assert_eq!(with("", Filters { modified_before: Some(2_000), ..Default::default() }), vec!["note.md"]);
    assert_eq!(with("", Filters { modified_after: Some(2_000), kinds: vec![KindGroup::Documents], ..Default::default() }), vec!["Bolletta luce.pdf"]);

    let sorted = |sort: Sort, desc: bool| {
        names(&v.search(&SearchQuery { sort, desc, filters: Filters { kinds: vec![KindGroup::Photos], ..Default::default() }, limit: 100, ..Default::default() }).unwrap())
    };
    assert_eq!(sorted(Sort::Size, false), vec!["amarena.png", "mare.jpg", "Spiaggia.jpg", "Città al mare.JPG"]);
    assert_eq!(sorted(Sort::Size, true), vec!["Città al mare.JPG", "Spiaggia.jpg", "mare.jpg", "amarena.png"]);
    assert_eq!(sorted(Sort::Name, true), vec!["Spiaggia.jpg", "mare.jpg", "Città al mare.JPG", "amarena.png"]);
    assert_eq!(sorted(Sort::Type, false), vec!["Città al mare.JPG", "mare.jpg", "Spiaggia.jpg", "amarena.png"]);
    let by_size_desc = names(&v.search(&SearchQuery { sort: Sort::Size, desc: true, limit: 100, ..Default::default() }).unwrap());
    assert_eq!(&by_size_desc[..5], &["Mare", "Mare", "Foto", "Estate 2024", "Documenti"], "folders stay first");
    let mut got = vec![];
    let mut off = Some(0);
    while let Some(o) = off {
        let p = v.search(&SearchQuery { text: "a".into(), offset: o, limit: 4, ..Default::default() }).unwrap();
        assert_eq!(p.total, 8);
        assert!(p.hits.len() <= 4);
        got.extend(names(&p));
        off = p.next_offset;
    }
    assert_eq!(got, find(&v, "a"), "pages concatenate to the whole list");
    assert!(v.search(&SearchQuery { text: "a".into(), offset: 50, limit: 4, ..Default::default() }).unwrap().hits.is_empty());
}

#[test]
fn index_follows_every_change() {
    let t = tempfile::tempdir().unwrap();
    let (v, nodes) = sample(t.path());
    assert_eq!(find(&v, "spiaggia"), vec!["Spiaggia.jpg"]);
    let spiaggia = nodes[0].id;
    let docs = nodes.iter().find(|n| n.name == "Documenti").unwrap().id;
    let foto = nodes.iter().find(|n| n.name == "Foto").unwrap().id;

    v.rename(spiaggia, docs, "Lungomare.jpg").unwrap();
    assert_eq!(find(&v, "spiaggia"), Vec::<String>::new());
    let p = v.search(&q("lungomare")).unwrap();
    assert_eq!((names(&p), p.hits[0].path.as_str()), (vec!["Lungomare.jpg".to_owned()], "/Documenti"));
    v.rename(foto, docs, "Foto vecchie").unwrap();
    let p = v.search(&q("amarena")).unwrap();
    assert_eq!(p.hits[0].path, "/Documenti/Foto vecchie/Mare");
    let n = file(&v, ROOT, "nuovo.txt", 0);
    v.write(n.id, 0, &[1u8; 12345]).unwrap();
    let with_size = |min: u64| {
        names(&v.search(&SearchQuery { text: "nuovo".into(), filters: Filters { min_size: Some(min), ..Default::default() }, limit: 10, ..Default::default() }).unwrap())
    };
    assert_eq!(with_size(12345), vec!["nuovo.txt"]);
    v.flush().unwrap();
    assert_eq!(with_size(12345), vec!["nuovo.txt"]);
    v.set_size(n.id, 10).unwrap();
    assert!(with_size(12345).is_empty());
    v.flush().unwrap();
    assert!(with_size(12345).is_empty());
    assert_eq!(v.search(&q("nuovo")).unwrap().hits[0].node.size, 10);
    v.remove(n.id).unwrap();
    assert!(find(&v, "nuovo").is_empty());
    v.remove_recursive(docs).unwrap();
    assert!(find(&v, "amarena").is_empty());
    assert!(find(&v, "lungomare").is_empty());
    assert_eq!(find(&v, "mare"), vec!["Mare"]);
    let a = file(&v, ROOT, "a.txt", 1);
    let b = file(&v, ROOT, "b.txt", 2);
    assert_eq!(find(&v, ".txt"), vec!["a.txt", "b.txt"]);
    v.rename(a.id, ROOT, "b.txt").unwrap();
    let p = v.search(&q(".txt")).unwrap();
    assert_eq!((names(&p), p.hits[0].node.id), (vec!["b.txt".to_owned()], a.id));
    let _ = b;
    v.close().unwrap();
}

#[test]
fn names_are_persisted_in_the_volume_and_survive_reopen_and_key_rotation() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("s.nvault");
    let (v, nodes) = sample(t.path());
    assert_eq!(v.meta_get(nv_core::SEARCH_SCHEMA_KEY).unwrap().as_deref(), Some(&b"1"[..]));
    let p = v.search(&q("mare")).unwrap();
    assert_eq!(p.pending, 12);
    assert_eq!(v.search_index_step(5).unwrap(), 7);
    assert_eq!(v.search_index_step(5).unwrap(), 2);
    assert_eq!(v.search_index_step(5).unwrap(), 0);
    assert_eq!(v.search(&q("mare")).unwrap().pending, 0);
    let tag = v.tag_create("Vacanze", "prussia").unwrap();
    v.tags_set(&[tag.id], &[nodes[1].id], true).unwrap();
    v.flush().unwrap();
    v.close().unwrap();

    let v = Vault::open(&path, &pw()).unwrap();
    let p = v.search(&q("citta")).unwrap();
    assert_eq!((names(&p), p.pending, p.hits[0].tags.clone()), (vec!["Città al mare.JPG".to_owned()], 0, vec![tag.id]));
    v.rename(nodes[1].id, nodes[1].parent, "Paese.jpg").unwrap();
    assert_eq!(v.search(&q("paese")).unwrap().pending, 1, "a rename queues the name");
    v.rotate_master(0, &pw()).unwrap();
    let p = v.search(&q("paese")).unwrap();
    assert_eq!((names(&p), p.pending, p.hits[0].tags.clone()), (vec!["Paese.jpg".to_owned()], 1, vec![tag.id]));
    let f = file(&v, ROOT, "dopo.txt", 1);
    assert_eq!(v.search(&q("dopo")).unwrap().pending, 2, "triggers still there after the rotation");
    assert_eq!(v.search_index_step(100).unwrap(), 0);
    let _ = f;
    v.check_integrity().unwrap();
    v.close().unwrap();
}

#[test]
fn tags_rules_counts_and_filters() {
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("s.nvault");
    let (v, nodes) = sample(t.path());
    let (spiaggia, citta, bolletta) = (nodes[0].id, nodes[1].id, nodes[4].id);
    let viaggi = v.tag_create("  Viaggi ", "bottiglia").unwrap();
    assert_eq!((viaggi.name.as_str(), viaggi.color.as_str(), viaggi.count), ("Viaggi", "bottiglia", 0));
    assert!(matches!(v.tag_create("VIÀGGI", "neutral"), Err(TagError::Exists)));
    assert!(matches!(v.tag_create("", "neutral"), Err(TagError::InvalidName)));
    assert!(matches!(v.tag_create(&"x".repeat(65), "neutral"), Err(TagError::InvalidName)));
    assert!(matches!(v.tag_create("a\nb", "neutral"), Err(TagError::InvalidName)));
    assert!(matches!(v.tag_create("Rosso", "bordeaux"), Err(TagError::InvalidColor)));
    let lavoro = v.tag_create("Lavoro", "seppia").unwrap();
    let urgente = v.tag_create("Urgente", "velluto").unwrap();

    assert_eq!(v.tags_set(&[viaggi.id], &[spiaggia, citta, ROOT, 999_999], true).unwrap(), vec![spiaggia, citta], "root and missing nodes skipped");
    assert_eq!(v.tags_set(&[viaggi.id], &[spiaggia], true).unwrap(), Vec::<NodeId>::new(), "already there");
    assert_eq!(v.tags_set(&[lavoro.id, urgente.id], &[bolletta, citta], true).unwrap(), vec![citta, bolletta]);
    assert!(matches!(v.tags_set(&[12345], &[spiaggia], true), Err(TagError::NotFound)));
    let counts: Vec<(String, u64)> = v.tags().unwrap().into_iter().map(|t| (t.name, t.count)).collect();
    assert_eq!(counts, vec![("Lavoro".into(), 2), ("Urgente".into(), 2), ("Viaggi".into(), 2)]);
    let tagged = |tags: Vec<u64>, any: bool| {
        names(&v.search(&SearchQuery { filters: Filters { tags, tags_any: any, ..Default::default() }, limit: 50, ..Default::default() }).unwrap())
    };
    assert_eq!(tagged(vec![viaggi.id], false), vec!["Città al mare.JPG", "Spiaggia.jpg"]);
    assert_eq!(tagged(vec![viaggi.id, lavoro.id], false), vec!["Città al mare.JPG"]);
    assert_eq!(tagged(vec![viaggi.id, lavoro.id], true), vec!["Bolletta luce.pdf", "Città al mare.JPG", "Spiaggia.jpg"]);
    assert_eq!(v.node_tags(&[citta, bolletta, spiaggia, nodes[2].id]).unwrap()[&citta], vec![viaggi.id, lavoro.id, urgente.id]);
    assert_eq!(v.child_tags(nodes[0].parent).unwrap().len(), 2);

    assert_eq!(v.tags_set(&[urgente.id], &[citta, bolletta, spiaggia], false).unwrap(), vec![citta, bolletta]);
    let renamed = v.tag_update(lavoro.id, Some("Ufficio"), Some("ardesia")).unwrap();
    assert_eq!((renamed.name.as_str(), renamed.color.as_str(), renamed.count), ("Ufficio", "ardesia", 2));
    assert!(matches!(v.tag_update(lavoro.id, Some("viaggi"), None), Err(TagError::Exists)));
    assert_eq!(v.tag_update(lavoro.id, Some("UFFICIO"), None).unwrap().name, "UFFICIO", "its own name in another case");
    assert!(matches!(v.tag_update(777, None, Some("neutral")), Err(TagError::NotFound)));
    assert_eq!(v.tag_delete(viaggi.id).unwrap(), vec![spiaggia, citta]);
    assert!(tagged(vec![viaggi.id], true).is_empty());
    assert_eq!(v.search(&q("spiaggia")).unwrap().hits[0].tags, Vec::<u64>::new());
    v.remove(bolletta).unwrap();
    v.flush().unwrap();
    assert_eq!(v.tags().unwrap().iter().find(|t| t.id == lavoro.id).unwrap().count, 1);
    let again = file(&v, nodes[4].parent, "Nuova bolletta.pdf", 1);
    assert!(v.node_tags(&[again.id]).unwrap().is_empty());
    v.flush().unwrap();
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    let tags: Vec<(String, String, u64)> = v.tags().unwrap().into_iter().map(|t| (t.name, t.color, t.count)).collect();
    assert_eq!(tags, vec![("UFFICIO".into(), "ardesia".into(), 1), ("Urgente".into(), "velluto".into(), 0)]);
}

#[test]
#[ignore]
fn bench_100k() {
    let t = tempfile::Builder::new().prefix("nv-bench").tempdir_in(std::env::var("NV_BENCH_DIR").unwrap_or_else(|_| ".".into())).unwrap();
    let path = t.path().join("bench.nvault");
    let words = [
        "Fattura", "Vacanze", "Mare", "Montagna", "Città", "Relazione", "Progetto", "Foto", "Scontrino", "Contratto", "Bolletta", "Lettera",
        "Curriculum", "Ricetta", "Viaggio", "Riunione", "Bilancio", "Estate", "Inverno", "Spiaggia", "Lavoro", "Casa", "Auto", "Medico",
    ];
    let exts = ["jpg", "pdf", "docx", "png", "mp4", "txt", "xlsx", "mp3", "zip", "md", "heic", "mov"];
    let t0 = Instant::now();
    let v = Vault::create(&path, &pw(), KdfPreset::Fast).unwrap();
    let mut seed = 0x2545F4914F6CDD1Du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut ids = vec![];
    for d in 0..1000u64 {
        let parent = if d < 20 { ROOT } else { ids[(rnd() % 20) as usize] };
        let dir = v.create(parent, &format!("{} {} {d}", words[(rnd() % 24) as usize], 2000 + d % 26), Kind::Dir).unwrap();
        ids.push(dir.id);
        for f in 0..99u64 {
            let name = format!("{} {} {:04}.{}", words[(rnd() % 24) as usize], words[(rnd() % 24) as usize], f + d * 100, exts[(rnd() % 12) as usize]);
            v.create(dir.id, &name, Kind::File).unwrap();
        }
    }
    v.flush().unwrap();
    println!("create 100k nodes: {:?}", t0.elapsed());
    let t1 = Instant::now();
    let mut steps = 0;
    while v.search_index_step(5000).unwrap() > 0 {
        steps += 1;
    }
    v.flush().unwrap();
    println!("persist 100k folded names ({steps} steps of 5000): {:?}", t1.elapsed());
    v.close().unwrap();
    println!("file size: {:.1} MB", std::fs::metadata(&path).unwrap().len() as f64 / 1e6);

    let t2 = Instant::now();
    let v = Vault::open(&path, &pw()).unwrap();
    println!("open: {:?}", t2.elapsed());
    let time = |label: &str, sq: SearchQuery| {
        let mut best = f64::MAX;
        let mut total = 0;
        for _ in 0..7 {
            let t = Instant::now();
            let p = v.search(&sq).unwrap();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
            total = p.total;
        }
        println!("{label:<48} {best:7.2} ms  ({total} matches)");
        best
    };
    let t3 = Instant::now();
    v.search(&q("x")).unwrap();
    println!("first search (builds the in-memory copy from the tables): {:?}", t3.elapsed());
    let mut worst: f64 = 0.0;
    worst = worst.max(time("'citta' (accent-insensitive, common)", q("citta")));
    worst = worst.max(time("'fattura 0042' (two terms, rare)", q("fattura 0042")));
    worst = worst.max(time("'a' (one letter, ~everything)", q("a")));
    worst = worst.max(time("'zzzz' (no match: full scan)", q("zzzz")));
    worst = worst.max(time("'' + photos (filter only, by name)", SearchQuery { filters: Filters { kinds: vec![KindGroup::Photos], ..Default::default() }, limit: 100, ..Default::default() }));
    worst = worst.max(time("'' all nodes sorted by size desc", SearchQuery { sort: Sort::Size, desc: true, limit: 100, ..Default::default() }));
    worst = worst.max(time("'mare' within a folder", SearchQuery { text: "mare".into(), filters: Filters { within: Some(ids[3]), ..Default::default() }, limit: 100, ..Default::default() }));
    worst = worst.max(time("'vacanze' page 5 (offset 400)", SearchQuery { text: "vacanze".into(), offset: 400, limit: 100, ..Default::default() }));
    let n = v.create(ids[0], "Appena creato.txt", Kind::File).unwrap();
    let t4 = Instant::now();
    assert_eq!(find(&v, "appena creato"), vec!["Appena creato.txt"]);
    println!("search right after a create: {:?}", t4.elapsed());
    v.rename(n.id, ids[1], "Rinominato.txt").unwrap();
    let t5 = Instant::now();
    assert_eq!(find(&v, "rinominato"), vec!["Rinominato.txt"]);
    println!("search right after a rename: {:?}", t5.elapsed());
    let tag = v.tag_create("Bench", "neutral").unwrap();
    let all: Vec<NodeId> = (2..20_002).collect();
    let t6 = Instant::now();
    v.tags_set(&[tag.id], &all, true).unwrap();
    println!("tag 20k nodes: {:?}", t6.elapsed());
    worst = worst.max(time("'' + tag filter (20k tagged)", SearchQuery { filters: Filters { tags: vec![tag.id], ..Default::default() }, limit: 100, ..Default::default() }));
    worst = worst.max(time("'foto' + tag + photos", SearchQuery { text: "foto".into(), filters: Filters { tags: vec![tag.id], kinds: vec![KindGroup::Photos], ..Default::default() }, limit: 100, ..Default::default() }));
    let t7 = Instant::now();
    let l = v.readdir(ids[5]).unwrap();
    let tags = v.child_tags(ids[5]).unwrap();
    println!("readdir 100 + child_tags: {:?} ({} nodes, {} tagged)", t7.elapsed(), l.len(), tags.len());
    println!("worst query: {worst:.2} ms");
    v.close().unwrap();

    let v = Vault::open(&path, &pw()).unwrap();
    let t8 = Instant::now();
    v.search(&q("x")).unwrap();
    println!("first search, every name stored: {:?}", t8.elapsed());
    v.meta_set("nv.search_norm", Some(b"0")).unwrap();
    v.close().unwrap();
    let v = Vault::open(&path, &pw()).unwrap();
    let t9 = Instant::now();
    let p = v.search(&q("x")).unwrap();
    println!("first search, no stored name (old volume): {:?}, pending {}", t9.elapsed(), p.pending);
    let t10 = Instant::now();
    let mut steps = 0;
    while v.search_index_step(5000).unwrap() > 0 {
        steps += 1;
    }
    v.flush().unwrap();
    println!("old-volume fill ({steps} steps of 5000): {:?}", t10.elapsed());
    v.close().unwrap();
    assert!(worst < 30.0, "worst query {worst:.2} ms");
}
