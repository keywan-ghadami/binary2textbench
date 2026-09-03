//! What a sample of the real decision costs, against always deciding.
//!
//! The encoder asks one question of every block: does the profile admit all
//! forty-eight bytes? Where the answer is always no -- compressed data,
//! binary, and also English prose in profile U, whose spaces put a rejected
//! byte in every block -- that question is pure overhead, and it is the whole
//! of what base65t costs over base64 when encoding.
//!
//! A sample answers it in advance: run the same check over the first `k`
//! blocks. If none of them is raw, write the whole stream as base64url and
//! stop asking. That is exactly base64, in base64's time, byte for byte.
//!
//! What it can cost is size, and only where the head of a file misrepresents
//! its tail. This measures that, per file and summed, at several sample
//! sizes, against always checking.
//!
//!     cargo run --release --example sample -- corpus/data/**/*

use base65t::{choose, encode_with, Form, Profile, BLOCK_BYTES};

/// Bytes the encoder writes when it checks every block.
fn size_checked(data: &[u8], p: Profile) -> usize {
    encode_with(data, p).len()
}

/// Bytes it writes when the first `k` blocks decide for the whole file.
fn size_sampled(data: &[u8], p: Profile, k: usize) -> (usize, bool) {
    let any_raw = data
        .chunks(BLOCK_BYTES)
        .take(k)
        .any(|b| choose(b.len(), p.admits_all(b)).0 == Form::Raw);
    if any_raw {
        (size_checked(data, p), true)
    } else {
        ((4 * data.len()).div_ceil(3), false)
    }
}

fn main() {
    let ks = [8usize, 16, 32, 64, 128];
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for path in std::env::args().skip(1) {
        if let Ok(d) = std::fs::read(&path) {
            if !d.is_empty() {
                files.push((path.rsplit('/').next().unwrap().to_string(), d));
            }
        }
    }
    for p in [Profile::U, Profile::T] {
        println!("\n## profile {p:?}\n");
        println!(
            "| file | bytes | checked | {} |",
            ks.iter()
                .map(|k| format!("k={k}"))
                .collect::<Vec<_>>()
                .join(" | ")
        );
        println!("|---|--:|--:|{}", "--:|".repeat(ks.len()));
        let mut tot_b64 = 0usize;
        let mut tot_checked = 0usize;
        let mut tot = vec![0usize; ks.len()];
        let mut skipped = vec![0usize; ks.len()];
        let mut lost = vec![0usize; ks.len()];
        for (name, d) in &files {
            let b64 = (4 * d.len()).div_ceil(3);
            let checked = size_checked(d, p);
            tot_b64 += b64;
            tot_checked += checked;
            let mut cells = Vec::new();
            for (i, &k) in ks.iter().enumerate() {
                let (s, asked) = size_sampled(d, p, k);
                tot[i] += s;
                if !asked {
                    skipped[i] += 1;
                    if s > checked {
                        lost[i] += 1;
                    }
                }
                cells.push(format!("{:.1} %", 100.0 * s as f64 / b64 as f64));
            }
            // Only the rows where the sample changes something, or where the
            // file is large enough for the question to matter.
            let moved = cells.iter().any(|c| c != &cells[0]);
            if moved || d.len() > 100_000 {
                println!(
                    "| `{name}` | {} | {:.1} % | {} |",
                    d.len(),
                    100.0 * checked as f64 / b64 as f64,
                    cells.join(" | ")
                );
            }
        }
        println!(
            "\n**summed:** always checking {:.2} % of base64",
            100.0 * tot_checked as f64 / tot_b64 as f64
        );
        for (i, &k) in ks.iter().enumerate() {
            println!(
                "  k={k:<4} {:.2} %  ({} of {} files written as pure base64, {} of them gave something up)",
                100.0 * tot[i] as f64 / tot_b64 as f64,
                skipped[i],
                files.len(),
                lost[i]
            );
        }
    }
}
