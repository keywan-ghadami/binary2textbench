//! Where does base65t actually save, and how much? Size only, no timing.
//!
//!     cargo run --release --example gain -- corpus/data/short/* corpus/data/*.*
//!
//! The answer turns out to be one shape rather than a gradient, which is why
//! this sorts rather than averages: an average over a corpus that holds both
//! populations describes neither. One population is already text and is being
//! carried through a channel that must accept bytes; the other is compressed
//! or binary and has nothing for a literal to find. There is very little in
//! between, and that is the finding.
use base65t::encode;

fn main() {
    let mut rows: Vec<(f64, String, usize)> = Vec::new();
    let (mut tot_b64, mut tot_u) = (0usize, 0usize);
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if d.is_empty() {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        let u = encode(&d).len();
        tot_b64 += b64;
        tot_u += u;
        rows.push((
            100.0 * u as f64 / b64 as f64,
            path.rsplit('/').next().unwrap().to_string(),
            d.len(),
        ));
    }
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    println!("| sample | bytes | size against base64 |");
    println!("|---|--:|--:|");
    for (u, name, n) in &rows {
        println!("| `{name}` | {n} | {u:.1} % |");
    }
    let n = rows.len() as f64;
    let under99 = rows.iter().filter(|r| r.0 < 99.0).count();
    let under95 = rows.iter().filter(|r| r.0 < 95.0).count();
    let at100 = rows.iter().filter(|r| r.0 >= 99.9).count();
    println!(
        "\n{} samples. Summed, as size: {:.2} % of base64.",
        rows.len(),
        100.0 * tot_u as f64 / tot_b64 as f64
    );
    println!(
        "Better than 99 %: {under99} ({:.0} %). Better than 95 %: {under95} ({:.0} %). \
         Indistinguishable from base64 (>= 99.9 %): {at100} ({:.0} %).",
        100.0 * under99 as f64 / n,
        100.0 * under95 as f64 / n,
        100.0 * at100 as f64 / n
    );
}
