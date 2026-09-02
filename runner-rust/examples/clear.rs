//! How much of the input stays readable, and what the exact programme adds.
//!
//! Size is not the only thing the two segmentation rules differ on. The linear
//! rule takes no literal under eleven bytes; the exact programme has no
//! threshold and reaches down to seven, so it leaves text in the clear that
//! the other spells out in base64. On a document that is mostly text with
//! punctuation in it -- XML, JSON, a stylesheet -- that is the difference
//! between reading it and not.
use base65t::internals::{segment_greedy, segment_with, costs, LiteralEnd, Rules, Seg};
use base65t::*;

fn passthrough(segs: &[Seg], n: usize) -> f64 {
    let lit: usize = segs.iter().filter_map(|s| match s {
        Seg::Literal(i, j) => Some(j - i), _ => None }).sum();
    100.0 * lit as f64 / n.max(1) as f64
}

fn main() {
    println!("| file | bytes | size: linear | size: exact | clear: linear | clear: exact |");
    println!("|---|--:|--:|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) { Ok(d) => d, Err(_) => continue };
        if d.len() < 512 { continue }
        let b64 = (4 * d.len()).div_ceil(3);
        for profile in [Profile::U, Profile::T] {
            let lin = segment_greedy(&d, Rules::preset(profile, Some(11), false));
            let r = Rules::preset(profile, Some(1), false);
            let c = costs(&d, r);
            let exact = segment_with(&d, r, &c, LiteralEnd::KeyOrder);
            println!("| `{}` ({profile:?}) | {} | {:.1} % | {:.1} % | {:.0} % | {:.0} % |",
                path.rsplit('/').next().unwrap(), d.len(),
                100.0 * encode_dense(&d, profile).len() as f64 / b64 as f64,
                100.0 * encode_canonical(&d, profile).len() as f64 / b64 as f64,
                passthrough(&lin, d.len()), passthrough(&exact, d.len()));
        }
    }
}
