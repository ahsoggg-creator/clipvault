//! Measures the history on synthetic entries. See docs/BENCHMARKS.md for the method.
//!
//!   cargo run --release -p clipvault-core --example bench [entries]

use std::time::Instant;

use clipvault_core::crypto::random_bytes;
use clipvault_core::{History, Limits, NewItem, Query};

const WORDS: [&str; 24] = [
    "report", "invoice", "meeting", "project", "release", "customer", "database", "network", "server", "config",
    "backup", "deploy", "review", "design", "budget", "ticket", "sprint", "module", "access", "token", "string",
    "value", "error", "status",
];

fn text(rng: &mut u64, n: u64) -> String {
    let mut next = || {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        *rng
    };
    let words = 6 + next() % 30;
    let mut s = format!("#{n} ");
    for _ in 0..words {
        s.push_str(WORDS[(next() % WORDS.len() as u64) as usize]);
        s.push(' ');
        if next() % 7 == 0 {
            s.push_str(&format!("{} ", next() % 100_000));
        }
    }
    s
}

fn median_ms(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(50_000);
    let lim = Limits::default();
    let mut rng = 0x9E37_79B9_7F4A_7C15u64;

    // 1. building the history in memory (BLAKE3 + index)
    let mut h = History::memory();
    let t = Instant::now();
    for i in 0..n {
        let item = NewItem {
            text: text(&mut rng, i),
            source_app: "bench.exe".into(),
            ..Default::default()
        };
        h.add(item, 1_000_000 + i, &lim).unwrap();
    }
    let build = t.elapsed();
    println!("entries: {n}");
    println!(
        "add to the in-memory index: {:.2} s total, {:.1} us per entry",
        build.as_secs_f64(),
        build.as_secs_f64() * 1e6 / n as f64
    );

    // 2. dedup: re-adding existing text must not grow the history
    let mut rng2 = 0x9E37_79B9_7F4A_7C15u64;
    let t = Instant::now();
    let reps = 1000.min(n);
    for i in 0..reps {
        let item = NewItem {
            text: text(&mut rng2, i),
            ..Default::default()
        };
        h.add(item, 2_000_000 + i, &lim).unwrap();
    }
    println!(
        "dedup of {reps} repeats: {:.1} us each, entries now {}",
        t.elapsed().as_secs_f64() * 1e6 / reps as f64,
        h.len()
    );

    // 3. search latency
    for q in ["invoice", "rprt bckp", "deploy 4", "zzzz", "serv conf tok"] {
        let query = Query {
            text: q.into(),
            ..Default::default()
        };
        let times: Vec<f64> = (0..15)
            .map(|_| {
                let t = Instant::now();
                let r = h.query(&query);
                std::hint::black_box(r.len());
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        println!(
            "search {q:?}: median {:.1} ms, {} hits",
            median_ms(times),
            h.query(&query).len()
        );
    }

    // 4. the encrypted vault: write (one fsync per entry, as in the app) and the start-up replay
    let persist = 2000.min(n);
    let dir = std::env::temp_dir().join(format!("clipvault-bench-{}", std::process::id()));
    let key = random_bytes();
    let mut p = History::open(&dir, &key).unwrap();
    let mut rng3 = 0x1234_5678_9ABC_DEF1u64;
    let t = Instant::now();
    for i in 0..persist {
        p.add(
            NewItem {
                text: text(&mut rng3, i),
                ..Default::default()
            },
            1_000_000 + i,
            &lim,
        )
        .unwrap();
    }
    println!(
        "encrypted append of {persist} entries (fsync each): {:.2} ms per entry",
        t.elapsed().as_secs_f64() * 1000.0 / persist as f64
    );
    drop(p);
    let size = std::fs::metadata(dir.join("vault.bin")).map_or(0, |m| m.len());
    let t = Instant::now();
    let p = History::open(&dir, &key).unwrap();
    println!(
        "start-up (decrypt + rebuild index) of {} entries: {:.0} ms, vault file {:.1} KB",
        p.len(),
        t.elapsed().as_secs_f64() * 1000.0,
        size as f64 / 1024.0
    );
    drop(p);
    let _ = std::fs::remove_dir_all(&dir);
}
