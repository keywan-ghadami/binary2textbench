//! One decision at the head of the stream, carried through: does it hold up?
//!
//! This measured the rule before it was a rule. It is kept, pointed at the
//! shipped `classify`, because the question it asks does not go away: the
//! encoder decides once, from a magic number or the entropy of the first four
//! kilobytes, whether to run the exact programme at all -- and a decision made
//! from a prefix can always be wrong about the rest of the file.
//!
//! What it prints is what the decision costs against always scanning: the size
//! it gives up where it says base64, and the files it scans for nothing where
//! it says exact. A wrong answer costs size or time, never correctness -- a
//! skipped file is exactly base64url, so §9.4 holds either way.
//!
//!     cargo run --release --example classify -- corpus/data/**/*
use base65t::*;

fn main() {
    println!("| file | bytes | verdict | always-scan | decided | given up |");
    println!("|---|--:|---|--:|--:|--:|");
    let (mut t_b64, mut t_scan, mut t_dec) = (0usize, 0usize, 0usize);
    let (mut wrong_skip, mut wasted) = (0usize, 0usize);
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if d.len() < 64 {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        // What the exact programme finds when it is always allowed to look.
        let scan = {
            use base65t::internals::{costs, emit, segment_with, LiteralEnd, Rules};
            let r = Rules::new(Profile::U, Some(1));
            let c = costs(&d, r);
            emit(&d, &segment_with(&d, r, &c, LiteralEnd::KeyOrder)).len()
        };
        let mode = classify(&d);
        // What the shipped encoder writes, decision included.
        let decided = encode_with(&d, Profile::U).len();
        t_b64 += b64;
        t_scan += scan;
        t_dec += decided;
        let lost = 100.0 * (decided as f64 - scan as f64) / b64 as f64;
        if mode == Mode::Base64 && lost > 0.5 {
            wrong_skip += 1;
        }
        if mode == Mode::Exact && scan as f64 > 0.995 * b64 as f64 {
            wasted += 1;
        }
        println!(
            "| `{}` | {} | {:?} | {:.1} % | {:.1} % | {} |",
            path.rsplit('/').next().unwrap(),
            d.len(),
            mode,
            100.0 * scan as f64 / b64 as f64,
            100.0 * decided as f64 / b64 as f64,
            if lost > 0.05 {
                format!("**{lost:.1} pt**")
            } else {
                "—".into()
            }
        );
    }
    println!(
        "\nAlways scanning: {:.2} % of base64. Deciding at the head: {:.2} %.",
        100.0 * t_scan as f64 / t_b64 as f64,
        100.0 * t_dec as f64 / t_b64 as f64
    );
    println!(
        "Files where the skip gave up more than half a point: {wrong_skip}. \
         Files scanned for nothing (>= 99.5 %): {wasted}."
    );
}
