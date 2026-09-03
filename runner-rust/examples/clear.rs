//! How much of the input stays readable, and what the profile decides.
//!
//! Size is not the only thing the encoding is worth. A raw block stands in
//! the output as it stood in the input, so a document whose text runs in
//! stretches of forty-eight bytes can be read in the encoded stream. Which
//! bytes qualify is the profile's question, and one byte the profile rejects
//! costs its whole block -- which is why the answer swings so far between U
//! and T on the same file.
//!
//!     cargo run --release --example clear -- corpus/data/*.*
use base65t::*;

/// Bytes that stand in the clear, by the form each block takes.
fn passthrough(data: &[u8], profile: Profile) -> f64 {
    let mut clear = 0usize;
    for block in data.chunks(BLOCK_BYTES) {
        let mask = (0..block.len())
            .filter(|&i| profile.allows(block[i]))
            .fold(0u64, |m, i| m | 1 << i);
        clear += match choose(block.len(), mask).0 {
            Form::Base64 => 0,
            Form::Raw => block.len(),
        };
    }
    100.0 * clear as f64 / data.len().max(1) as f64
}

fn main() {
    println!("| file | bytes | size U | size T | clear U | clear T |");
    println!("|---|--:|--:|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let d = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if d.len() < 512 {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        println!(
            "| `{}` | {} | {:.1} % | {:.1} % | {:.0} % | {:.0} % |",
            path.rsplit('/').next().unwrap(),
            d.len(),
            100.0 * encode_with(&d, Profile::U).len() as f64 / b64 as f64,
            100.0 * encode_with(&d, Profile::T).len() as f64 / b64 as f64,
            passthrough(&d, Profile::U),
            passthrough(&d, Profile::T)
        );
    }
}
