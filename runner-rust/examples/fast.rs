//! `dense` against `dense-fast`: what declining to look buys and costs.
use base65t::{encode_dense, encode_with, Preset, Profile};
use std::time::Instant;

fn bench(n: usize, mut f: impl FnMut()) -> f64 {
    let mut best = 0.0f64;
    for _ in 0..5 {
        let reps = (192 << 20) / n.max(1) + 1;
        f();
        let t = Instant::now();
        for _ in 0..reps { f(); }
        let r = (n * reps) as f64 / t.elapsed().as_secs_f64() / (1 << 20) as f64;
        if r > best { best = r; }
    }
    best
}

fn main() {
    println!("| file | size: dense | size: dense-fast | dense | dense-fast | gain |");
    println!("|---|--:|--:|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let d = std::fs::read(&path).unwrap();
        if d.is_empty() { continue }
        let b64 = (4 * d.len()).div_ceil(3);
        let sd = encode_dense(&d, Profile::U).len();
        let sf = encode_with(&d, Preset::DenseFast, Profile::U).len();
        let td = bench(d.len(), || { std::hint::black_box(encode_dense(&d, Profile::U)); });
        let tf = bench(d.len(), || { std::hint::black_box(encode_with(&d, Preset::DenseFast, Profile::U)); });
        println!("| `{}` | {:.1} % | {:.1} % | {td:.0} MiB/s | {tf:.0} MiB/s | **{:.2}x** |",
                 path.rsplit('/').next().unwrap(),
                 100.0 * sd as f64 / b64 as f64, 100.0 * sf as f64 / b64 as f64, tf / td);
    }
}
