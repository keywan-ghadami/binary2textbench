//! What does the space carry in profile T?
//!
//!     cargo run --release --example tspace -- corpus/data/*.* corpus/data/short/*
//!
//! Profile T admits printable ASCII without `"` and `\`, and the space is one
//! of those 93 characters. It is also the one character that makes a T value
//! fail a container it would otherwise pass: a whitespace-separated log line
//! has to quote it, and an HTTP header value has its edges stripped. This
//! measures what dropping it would cost, in size and in readability, so the
//! question is decided by the number rather than by the argument.

const BLOCK: usize = 48;
const SAMPLE_BLOCKS: usize = 64;

fn admits(b: u8, space: bool) -> bool {
    (0x20..=0x7e).contains(&b) && b != b'"' && b != b'\\' && (space || b != b' ')
}

/// The encoder of §9.0 and §9.6, size and cleartext bytes only.
fn measure(d: &[u8], space: bool) -> (usize, usize) {
    let raw_ok = |blk: &[u8]| blk.len() >= 4 && blk.iter().all(|&b| admits(b, space));
    let sampled =
        d.len() > SAMPLE_BLOCKS * BLOCK && !d.chunks(BLOCK).take(SAMPLE_BLOCKS).any(raw_ok);
    if sampled {
        return ((4 * d.len()).div_ceil(3), 0);
    }
    let (mut len, mut clear) = (0usize, 0usize);
    for blk in d.chunks(BLOCK) {
        if raw_ok(blk) {
            len += blk.len() + 2;
            clear += blk.len();
        } else {
            len += (4 * blk.len()).div_ceil(3);
        }
    }
    (len, clear)
}

fn main() {
    let (mut b64s, mut with, mut without) = (0usize, 0usize, 0usize);
    let (mut cw, mut cwo, mut bytes) = (0usize, 0usize, 0usize);
    let mut rows = Vec::new();
    for path in std::env::args().skip(1) {
        let Ok(d) = std::fs::read(&path) else {
            continue;
        };
        if d.is_empty() {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        let (lw, kw) = measure(&d, true);
        let (lo, ko) = measure(&d, false);
        b64s += b64;
        with += lw;
        without += lo;
        cw += kw;
        cwo += ko;
        bytes += d.len();
        if lw != lo {
            rows.push((
                100.0 * (lo as f64 - lw as f64) / b64 as f64,
                path.rsplit('/').next().unwrap().to_string(),
                d.len(),
                100.0 * lw as f64 / b64 as f64,
                100.0 * lo as f64 / b64 as f64,
                100.0 * kw as f64 / d.len() as f64,
                100.0 * ko as f64 / d.len() as f64,
            ));
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("Files where the space changes anything, worst first.");
    println!();
    println!("| file | bytes | size T | size T-no-space | clear T | clear T-no-space |");
    println!("|---|--:|--:|--:|--:|--:|");
    for (_, name, n, a, b, ca, cb) in rows.iter().take(20) {
        println!("| `{name}` | {n} | {a:.1} % | {b:.1} % | {ca:.0} % | {cb:.0} % |");
    }
    println!();
    println!("{} files changed by the space.", rows.len());
    println!(
        "Summed over everything, as size: T {:.2} %, T without the space {:.2} % \
         (base64 = 100 %).",
        100.0 * with as f64 / b64s as f64,
        100.0 * without as f64 / b64s as f64
    );
    println!(
        "Cleartext share of all input bytes: T {:.1} %, T without the space {:.1} %.",
        100.0 * cw as f64 / bytes as f64,
        100.0 * cwo as f64 / bytes as f64
    );
}
