//! How much of the input stays readable in the encoded stream.
//!
//! Size is not the only thing the encoding is worth. A raw block stands in
//! the output as it stood in the input, so a document whose text runs in
//! stretches of forty-eight bytes can be read in the encoded stream. One byte
//! the alphabet rejects costs its whole block, so the answer is all-or-
//! nothing per block and swings hard on what a file is made of.
//!
//!     cargo run --release --example clear -- corpus/data/*.*
use base65t::*;

/// Bytes that stand in the clear, by the form each block takes.
fn passthrough(data: &[u8]) -> f64 {
    let mut clear = 0usize;
    for block in data.chunks(BLOCK_BYTES) {
        clear += match choose(block.len(), admits_all(block)).0 {
            Form::Base64 => 0,
            Form::Raw => block.len(),
        };
    }
    100.0 * clear as f64 / data.len().max(1) as f64
}

fn main() {
    println!("| file | bytes | size | in the clear |");
    println!("|---|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let Ok(d) = std::fs::read(&path) else {
            continue;
        };
        if d.is_empty() {
            continue;
        }
        let b64 = (4 * d.len()).div_ceil(3);
        println!(
            "| `{}` | {} | {:.1} % | {:.0} % |",
            path.rsplit('/').next().unwrap(),
            d.len(),
            100.0 * encode(&d).len() as f64 / b64 as f64,
            passthrough(&d)
        );
    }
    println!();
    println!("Size is against unpadded base64; less is better. \"In the clear\" is the");
    println!("share of input bytes standing in the output as they stood in the input.");
}
