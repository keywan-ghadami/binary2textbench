//! How much of the input stays readable, and what the profile decides.
//!
//! Size is not the only thing the encoding is worth. A literal leaves its
//! bytes in the output as they were, so a document that is mostly text with
//! punctuation in it -- XML, JSON, a stylesheet -- can be read in the encoded
//! stream. Which bytes qualify is the profile's question, and the answer moves
//! far more than any encoder rule ever did: the same XML that keeps 12 % of
//! its bytes in the clear under profile U keeps 93 % under profile T.
//!
//!     cargo run --release --example clear -- corpus/data/*.*
use base65t::internals::{costs, segment_with, LiteralEnd, Rules, Seg};
use base65t::*;

fn passthrough(segs: &[Seg], n: usize) -> f64 {
    let lit: usize = segs
        .iter()
        .filter_map(|s| match s {
            Seg::Literal(i, j) => Some(j - i),
            _ => None,
        })
        .sum();
    100.0 * lit as f64 / n.max(1) as f64
}

fn main() {
    println!("| file | bytes | mode | size U | size T | clear U | clear T |");
    println!("|---|--:|---|--:|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if d.len() < 512 {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        let clear = |profile| {
            let r = Rules::new(profile, Some(1));
            let c = costs(&d, r);
            passthrough(&segment_with(&d, r, &c, LiteralEnd::KeyOrder), d.len())
        };
        println!(
            "| `{}` | {} | {:?} | {:.1} % | {:.1} % | {:.0} % | {:.0} % |",
            path.rsplit('/').next().unwrap(),
            d.len(),
            classify(&d),
            100.0 * encode_with(&d, Profile::U).len() as f64 / b64 as f64,
            100.0 * encode_with(&d, Profile::T).len() as f64 / b64 as f64,
            clear(Profile::U),
            clear(Profile::T)
        );
    }
}
