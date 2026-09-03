//! What does `.` carry, and would 65 characters do?
//!
//!     cargo run --release --example dot -- corpus/data/**/*
//!
//! The output alphabet is base64url's 64 characters plus `~` plus `.` -- 66,
//! RFC 3986 *unreserved*. Dropping `.` would make it exactly base64url plus
//! one character, which is what the format is named after. This measures the
//! cost, the same way `tspace` measured the space.

const BLOCK: usize = 48;
const SAMPLE_BLOCKS: usize = 64;

fn admits(b: u8, dot: bool) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'~' || (dot && b == b'.')
}

fn measure(d: &[u8], dot: bool) -> (usize, usize) {
    let raw_ok = |b: &[u8]| b.len() >= 4 && b.iter().all(|&c| admits(c, dot));
    if d.len() > SAMPLE_BLOCKS * BLOCK && !d.chunks(BLOCK).take(SAMPLE_BLOCKS).any(raw_ok) {
        return ((4 * d.len()).div_ceil(3), 0);
    }
    let (mut len, mut clear) = (0usize, 0usize);
    for b in d.chunks(BLOCK) {
        if raw_ok(b) {
            len += b.len() + 2;
            clear += b.len();
        } else {
            len += (4 * b.len()).div_ceil(3);
        }
    }
    (len, clear)
}

fn main() {
    let (mut b64s, mut with, mut without, mut cw, mut cwo, mut bytes) = (0, 0, 0, 0, 0, 0usize);
    let mut rows = Vec::new();
    for path in std::env::args().skip(1) {
        let Ok(d) = std::fs::read(&path) else { continue };
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
                lo as f64 - lw as f64,
                path.rsplit('/').next().unwrap().to_string(),
                d.len(),
                100.0 * lw as f64 / b64 as f64,
                100.0 * lo as f64 / b64 as f64,
            ));
        }
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    println!("| file | bytes | size with `.` | size without |");
    println!("|---|--:|--:|--:|");
    for (_, n, b, a, c) in rows.iter().take(15) {
        println!("| `{n}` | {b} | {a:.1} % | {c:.1} % |");
    }
    println!("\n{} of the samples change.", rows.len());
    println!(
        "Summed, as size: with `.` {:.2} %, without {:.2} % (base64 = 100 %).",
        100.0 * with as f64 / b64s as f64,
        100.0 * without as f64 / b64s as f64
    );
    println!(
        "Cleartext share of all input bytes: {:.1} % against {:.1} %.",
        100.0 * cw as f64 / bytes as f64,
        100.0 * cwo as f64 / bytes as f64
    );
}
