// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The pieces this repository implements itself, checked against somebody
//! else's implementation.
//!
//! Base64 is the baseline every ratio in the report is divided by, which makes
//! it the one place where being wrong would be invisible: a broken baseline
//! moves every number by the same factor and nothing looks out of place. So it
//! is checked against `base64(1)` rather than only against this crate's own
//! idea of what Base64 is. The JSON escaper is checked against Python's
//! `json.dumps` for the same reason -- it decides the primary size figure for
//! every codec.
//!
//! Missing tools skip rather than fail: the suite should still run on a machine
//! without coreutils or Python.

use std::io::Write;
use std::process::{Command, Stdio};

use b2t_runner::{codecs, json};

fn have(cmd: &str) -> bool {
    Command::new(cmd)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn pipe(cmd: &str, args: &[&str], input: &[u8]) -> String {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write");
    let out = child.wait_with_output().expect("wait");
    assert!(out.status.success(), "{cmd} exited unsuccessfully");
    String::from_utf8(out.stdout).expect("output is utf-8")
}

/// Boundary lengths, every byte value, and a seeded run long enough to cross
/// many blocks.
fn cases() -> Vec<Vec<u8>> {
    let mut s: u32 = 0x5eed_1234;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        (s & 0xff) as u8
    };
    let long: Vec<u8> = (0..3000).map(|_| next()).collect();
    let mut v: Vec<Vec<u8>> = (0..=16).map(|n| long[..n].to_vec()).collect();
    v.push((0..=255u8).collect());
    v.push(long);
    v.push(vec![0u8; 64]);
    v.push(vec![0xffu8; 64]);
    v
}

#[test]
fn base64_agrees_with_coreutils() {
    if !have("base64") {
        eprintln!("skipping: no base64(1) on PATH");
        return;
    }
    for data in cases() {
        // -w0 keeps it on one line; the wrapping is the tool's, not the format's.
        let theirs = pipe("base64", &["-w0"], &data);
        assert_eq!(
            codecs::base64_encode(&data),
            theirs.trim_end(),
            "encoding {} bytes",
            data.len()
        );
    }
}

#[test]
fn ascii85_agrees_with_python() {
    if !have("python3") {
        eprintln!("skipping: no python3 on PATH");
        return;
    }
    let script = "import base64,sys; \
                  sys.stdout.write(base64.a85encode(sys.stdin.buffer.read()).decode())";
    for data in cases() {
        let theirs = pipe("python3", &["-c", script], &data);
        assert_eq!(
            codecs::ascii85_encode(&data),
            theirs,
            "encoding {} bytes",
            data.len()
        );
    }
}

#[test]
fn escaping_agrees_with_python() {
    if !have("python3") {
        eprintln!("skipping: no python3 on PATH");
        return;
    }
    // Every character any of these alphabets can emit, plus the controls that
    // none of them emits but that the escaper must still get right.
    let printable: String = (0x20u8..0x7f).map(|b| b as char).collect();
    let payload = format!("{printable}\t\n\r\u{0b}\u{0c}\u{01}");

    let script = "import json,sys; \
                  sys.stdout.write(json.dumps(sys.stdin.read(), ensure_ascii=False))";
    let theirs = pipe("python3", &["-c", script], payload.as_bytes());

    let mut mine = String::from("\"");
    json::escape_into(&payload, &mut mine);
    mine.push('"');
    assert_eq!(mine, theirs, "JSON escaping");
}
