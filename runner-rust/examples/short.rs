//! The case base65t is actually for: small values, cache-resident.
//!
//! §0.1 names URL queries, cookie values, headers and cache keys. None of
//! those is eight megabytes, and at eight megabytes both codecs are bound by
//! memory bandwidth rather than by what they compute -- which is why the large
//! files say so little about the throughput a caller sees.
//!
//! **The size column is against `ceil(4n/3)`, unpadded**, because that is what
//! §9.4 promises and what a URL query would actually carry. The runner's own
//! base64 pads, which on a four-byte value is two characters out of eight --
//! quoting the padded ratio here would credit base65t with 25 % it did not
//! earn. The timing columns are against the runner's base64, which is the
//! denominator of every published figure; two `=` cost it nothing measurable.
use b2t_runner::codecs;
use std::time::Instant;

fn bench<T>(n: usize, reps: usize, mut f: impl FnMut() -> T) -> f64 {
    let mut best = 0.0f64;
    for _ in 0..5 {
        let _ = f();
        let t = Instant::now();
        for _ in 0..reps {
            std::hint::black_box(f());
        }
        let r = (n * reps) as f64 / t.elapsed().as_secs_f64() / (1 << 20) as f64;
        if r > best {
            best = r;
        }
    }
    best
}

fn main() {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for path in std::env::args().skip(1) {
        let d = std::fs::read(&path).unwrap();
        files.push((path.rsplit('/').next().unwrap().to_string(), d));
    }
    println!("| sample | bytes | size | encode | decode |");
    println!("|---|--:|--:|--:|--:|");
    // Summed as time, not as a mean of ratios: these samples differ in size
    // by a factor of forty, and a mean of ratios would weight a four-byte
    // customer number like a hundred-and-fifty-byte JWT.
    let (mut te0, mut te1, mut td0, mut td1) = (0.0, 0.0, 0.0, 0.0);
    for (name, d) in &files {
        if d.is_empty() {
            continue;
        }
        let reps = 2_000_000 / d.len().max(1) + 2000;
        let b64 = codecs::base64_encode(d);
        let dense = base65t::encode(d);
        let e0 = bench(d.len(), reps, || codecs::base64_encode(d));
        let e1 = bench(d.len(), reps, || base65t::encode(d));
        let x0 = bench(d.len(), reps, || codecs::base64_decode(&b64).unwrap());
        let x1 = bench(d.len(), reps, || {
            base65t::decode(&dense).unwrap()
        });
        let mb = d.len() as f64 / (1 << 20) as f64;
        te0 += mb / e0;
        te1 += mb / e1;
        td0 += mb / x0;
        td1 += mb / x1;
        println!(
            "| `{name}` | {} | {:.0} % | {:.0} % | {:.0} % |",
            d.len(),
            100.0 * dense.len() as f64 / (4 * d.len()).div_ceil(3) as f64,
            100.0 * e0 / e1,
            100.0 * x0 / x1
        );
    }
    println!(
        "\nover all samples, base65t as a share of base64's time: encode {:.0} %, decode {:.0} %",
        100.0 * te1 / te0,
        100.0 * td1 / td0
    );
}
