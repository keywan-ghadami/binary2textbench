// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Does Base91z's cheap decision reach the same answer as its reference one?
//!
//! The benchmark measures `encode_at`, because that is the path a caller gets:
//! it takes one histogram over a kilobyte and builds only the candidate that
//! names. `encode_auto` reaches the same decision by building both candidates
//! and keeping the shorter, which its own documentation says costs an order of
//! magnitude.
//!
//! Measuring the cheap one is only honest if it does not also produce worse
//! output. This walks the corpus and reports every sample where the two differ
//! in length, and by how much.
//!
//!     cargo run --release --example agree -- ../corpus/data/manifest.json

use std::path::PathBuf;

fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../corpus/data/manifest.json".into());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e} -- run corpus/manifest.py first"));
    let manifest: serde_json::Value = serde_json::from_str(&text).expect("manifest is JSON");
    let root = PathBuf::from(&path).parent().expect("manifest has a directory").to_path_buf();

    let level = base91z::DEFAULT_LEVEL;
    let (mut differ, mut worse, mut total, mut worst) = (0usize, 0usize, 0usize, 0i64);

    for s in manifest["samples"].as_array().expect("samples is a list") {
        let name = s["name"].as_str().expect("name");
        let data = std::fs::read(root.join(s["path"].as_str().expect("path"))).expect("sample");
        let cheap = base91z::encode_at(&data, level).expect("encode_at");
        let reference = base91z::encode_auto(&data, level).expect("encode_auto");
        total += 1;
        if cheap.len() != reference.len() {
            differ += 1;
            let delta = cheap.len() as i64 - reference.len() as i64;
            if delta > 0 {
                worse += 1;
                worst = worst.max(delta);
            }
            println!("{name}: encode_at {} vs encode_auto {} ({delta:+})",
                     cheap.len(), reference.len());
        }
        // Whatever it chose has to come back.
        assert_eq!(base91z::decode(&cheap).expect("decode"), data, "{name}");
    }

    println!("\n{total} samples, {differ} differ, {worse} where the cheap path is larger");
    if worse > 0 {
        println!("worst case: {worst} characters larger");
    }
}
