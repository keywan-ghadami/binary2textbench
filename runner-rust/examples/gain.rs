//! Where does base65t actually save, and how much? Size only, no timing.
//!
//!     cargo run --release --example gain -- corpus/data/short/* corpus/data/*.*
//!
//! The answer turns out to be one shape rather than a gradient, which is why
//! this sorts rather than averages: an average over a corpus that holds both
//! populations describes neither.
use base65t::{encode_canonical, encode_dense, Profile};

fn main() {
    let mut rows: Vec<(f64, f64, String, usize)> = Vec::new();
    let (mut tot_b64, mut tot_d) = (0usize, 0usize);
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) { Ok(d) => d, Err(_) => continue };
        if d.is_empty() { continue }
        let b64 = (4 * d.len()).div_ceil(3);
        let u = encode_dense(&d, Profile::U).len();
        let c = encode_canonical(&d, Profile::U).len();
        tot_b64 += b64; tot_d += u;
        rows.push((100.0 * u as f64 / b64 as f64, 100.0 * c as f64 / b64 as f64,
                   path.rsplit('/').next().unwrap().to_string(), d.len()));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    println!("| sample | bytes | dense vs base64 | canonical vs base64 |");
    println!("|---|--:|--:|--:|");
    for (u, c, name, n) in &rows {
        println!("| `{name}` | {n} | {u:.1} % | {c:.1} % |");
    }
    let n = rows.len() as f64;
    let under99 = rows.iter().filter(|r| r.0 < 99.0).count();
    let under95 = rows.iter().filter(|r| r.0 < 95.0).count();
    let at100 = rows.iter().filter(|r| r.0 >= 99.9).count();
    println!("\n{} samples. Summed over all of them, dense is {:.1} % of base64.",
             rows.len(), 100.0 * tot_d as f64 / tot_b64 as f64);
    println!("Better than 99 %: {under99} ({:.0} %). Better than 95 %: {under95} ({:.0} %). \
              Indistinguishable from base64 (>= 99.9 %): {at100} ({:.0} %).",
             100.0 * under99 as f64 / n, 100.0 * under95 as f64 / n, 100.0 * at100 as f64 / n);
}
