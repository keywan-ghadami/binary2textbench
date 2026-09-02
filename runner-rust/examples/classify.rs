//! One decision at the head of the stream, carried through: does it hold up?
//!
//! The cheapest possible rule, and the one other formats use: look at the
//! first bytes for a magic number that names an already-compressed container,
//! and where there is none, measure the entropy of a sample. High entropy or a
//! known magic number means there is nothing for a literal to find, so the
//! whole stream goes through base64 with no scan at all. Anything else gets
//! the exact programme.
//!
//! What this prints is what that decision costs against always scanning: the
//! size it gives up where it says no, and the work it wastes where it says yes
//! and finds little.
use base65t::*;

const MAGIC: [(&[u8], &str); 9] = [
    (&[0x1f, 0x8b], "gzip"),
    (&[0x28, 0xb5, 0x2f, 0xfd], "zstd"),
    (&[0xfd, b'7', b'z', b'X', b'Z'], "xz"),
    (b"BZh", "bzip2"),
    (b"PK\x03\x04", "zip"),
    (&[0xff, 0xd8, 0xff], "jpeg"),
    (&[0x89, b'P', b'N', b'G'], "png"),
    (b"\x00asm", "wasm"),
    (b"OggS", "ogg"),
];

fn entropy(d: &[u8]) -> f64 {
    let mut c = [0usize; 256];
    for &b in d { c[b as usize] += 1 }
    let n = d.len() as f64;
    -c.iter().filter(|&&k| k > 0)
        .map(|&k| { let p = k as f64 / n; p * p.log2() }).sum::<f64>()
}

fn main() {
    let limit: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(7.4);
    println!("entropy threshold {limit:.1} bits/byte\n");
    println!("| file | bytes | verdict | always-scan | decided | given up |");
    println!("|---|--:|---|--:|--:|--:|");
    let (mut t_b64, mut t_scan, mut t_dec) = (0usize, 0usize, 0usize);
    let (mut wrong_skip, mut wasted) = (0usize, 0usize);
    for path in std::env::args().skip(2) {
        let d = match std::fs::read(&path) { Ok(d) => d, Err(_) => continue };
        if d.len() < 64 { continue }
        let b64 = (4 * d.len()).div_ceil(3);
        let scan = encode_canonical(&d, Profile::U).len();
        let sample = &d[..(4096).min(d.len())];
        let magic = MAGIC.iter().find(|(m, _)| d.starts_with(m));
        let h = entropy(sample);
        let (skip, why) = match magic {
            Some((_, name)) => (true, name.to_string()),
            None if h > limit => (true, format!("H={h:.2}")),
            None => (false, format!("H={h:.2}")),
        };
        let decided = if skip { b64 } else { scan };
        t_b64 += b64; t_scan += scan; t_dec += decided;
        let lost = 100.0 * (decided as f64 - scan as f64) / b64 as f64;
        if skip && lost > 0.5 { wrong_skip += 1 }
        if !skip && scan as f64 > 0.995 * b64 as f64 { wasted += 1 }
        println!("| `{}` | {} | {} {why} | {:.1} % | {:.1} % | {} |",
            path.rsplit('/').next().unwrap(), d.len(),
            if skip { "base64," } else { "scan," },
            100.0 * scan as f64 / b64 as f64, 100.0 * decided as f64 / b64 as f64,
            if lost > 0.05 { format!("**{lost:.1} pt**") } else { "—".into() });
    }
    println!("\nAlways scanning: {:.2} % of base64. Deciding at the head: {:.2} %.",
             100.0 * t_scan as f64 / t_b64 as f64, 100.0 * t_dec as f64 / t_b64 as f64);
    println!("Files where the skip gave up more than half a point: {wrong_skip}. \
              Files scanned for nothing (>= 99.5 %): {wasted}.");
}
