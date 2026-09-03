//! Large inputs: what the block encoder costs against base64 at memory-bandwidth sizes.
//!
//! The companion is `short.rs`, which measures the values the format is
//! actually for. These two disagree, and the disagreement is the point: at
//! eight megabytes both codecs are bound by memory bandwidth rather than by
//! what they compute, and the ratio measures the scan; at sixty-four bytes it
//! measures the format.
//!
//! Best of five, each timed over at least 128 MiB of work, so that allocator
//! churn and a noisy shared runner do not decide the answer. Both sides live
//! in this process and were built by the same compiler with the same switches;
//! the base64 is the runner's own, which is the denominator of every published
//! figure.
use b2t_runner::codecs;
use base65t::Profile;
use std::time::Instant;

fn bench<T>(bytes: usize, mut f: impl FnMut() -> T) -> f64 {
    let mut best = 0.0f64;
    for _ in 0..5 {
        let reps = (128 << 20) / bytes.max(1) + 1;
        let _ = f();
        let t = Instant::now();
        for _ in 0..reps {
            std::hint::black_box(f());
        }
        let r = (bytes * reps) as f64 / t.elapsed().as_secs_f64() / (1 << 20) as f64;
        if r > best {
            best = r;
        }
    }
    best
}

fn main() {
    let profile = match std::env::var("PROFILE").as_deref() {
        Ok("T") => Profile::T,
        _ => Profile::U,
    };
    println!("profile {profile:?} — MiB/s, and base65t as a share of base64's time");
    println!("| file | size | b64 enc | b65t enc | enc | b64 dec | b65t dec | dec |");
    println!("|---|--:|--:|--:|--:|--:|--:|--:|");
    for path in std::env::args().skip(1) {
        let d = std::fs::read(&path).unwrap();
        if d.len() < 1 << 20 {
            continue;
        }
        let b64 = codecs::base64_encode(&d);
        let ours = base65t::encode_with(&d, profile);
        let e0 = bench(d.len(), || codecs::base64_encode(&d));
        let e1 = bench(d.len(), || base65t::encode_with(&d, profile));
        let x0 = bench(d.len(), || codecs::base64_decode(&b64).unwrap());
        let x1 = bench(d.len(), || base65t::decode(&ours, profile).unwrap());
        println!(
            "| `{}` | {:.1} % | {e0:.0} | {e1:.0} | **{:.0} %** | {x0:.0} | {x1:.0} | **{:.0} %** |",
            path.rsplit('/').next().unwrap(),
            100.0 * ours.len() as f64 / b64.len() as f64,
            100.0 * e0 / e1,
            100.0 * x0 / x1
        );
    }
}
