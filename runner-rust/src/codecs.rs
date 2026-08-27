// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The encodings under test, behind one shape.
//!
//! Three come from their own repositories and are built from source, so that
//! throughput is compared between encodings and not between languages. Three
//! are implemented here:
//!
//! * **Base64** is the baseline. Every number in the report is a ratio against
//!   it, measured in the same round on the same machine, which is what makes
//!   results from a noisy cloud runner comparable at all.
//! * **Classic basE91** is what Base91z is derived from, and its alphabet
//!   includes `"` -- so it is also the clearest demonstration of why escaping
//!   has to be measured.
//! * **Ascii85** is the classic five-character encoding, and the natural point
//!   of reference for Base85N.
//!
//! They are implemented here rather than pulled from crates.io on purpose: a
//! dependency's optimisation level would show up as an encoding's speed.

use std::fmt;

/// One encoding, as the runner uses it.
pub struct Codec {
    pub name: &'static str,
    /// What the encoding is, in one line, for the report.
    pub note: &'static str,
    pub encode: fn(&[u8]) -> String,
    pub decode: fn(&str) -> Result<Vec<u8>, CodecError>,
    /// Set when the codec ships its own compression decision, which is then
    /// measured as it ships rather than with a compressor bolted in front.
    pub native: Option<NativePath>,
}

/// A codec that compresses on its own behalf: encode and decode take the raw
/// bytes and the codec decides what to do with them.
pub struct NativePath {
    pub label: &'static str,
    pub note: &'static str,
    pub encode: fn(&[u8]) -> String,
    pub decode: fn(&str) -> Result<Vec<u8>, CodecError>,
}

#[derive(Debug)]
pub struct CodecError(pub String);

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<base94max::DecodeError> for CodecError {
    fn from(e: base94max::DecodeError) -> Self {
        CodecError(e.to_string())
    }
}

pub fn all() -> Vec<Codec> {
    vec![
        Codec {
            name: "base64",
            note: "RFC 4648, the baseline every ratio is against",
            encode: base64_encode,
            decode: base64_decode,
            native: None,
        },
        Codec {
            name: "base91-classic",
            note: "basE91 (Henke, 2005); its alphabet contains \" and \\",
            encode: base91_classic_encode,
            decode: base91_classic_decode,
            native: None,
        },
        Codec {
            name: "ascii85",
            note: "Ascii85 with the z shortcut, no delimiters",
            encode: ascii85_encode,
            decode: ascii85_decode,
            native: None,
        },
        Codec {
            name: "base85n",
            note: "Base85N 0.5.x, JSON-safe alphabet",
            encode: |d| base85n::encode(d),
            decode: |s| base85n::decode(s).map_err(|e| CodecError(format!("{e:?}"))),
            native: None,
        },
        Codec {
            name: "base94max",
            note: "Base94Max, adaptive 13/14-bit over all printable ASCII",
            encode: |d| base94max::encode(d),
            decode: |s| base94max::decode(s).map_err(CodecError::from),
            native: None,
        },
        Codec {
            name: "base91z",
            note: "Base91z 0.4.x, JSON-safe alphabet with typed segments",
            encode: |d| base91z::encode_plain(d),
            decode: |s| base91z::decode(s).map_err(|e| CodecError(format!("{e:?}"))),
            // Base91z is the one codec here that decides for itself whether to
            // compress. Measuring it only with an external compressor in front
            // would measure a configuration no caller of it has.
            native: Some(NativePath {
                label: "auto",
                note: "Base91z choosing its own compression, at its own default level",
                // `encode_at`, not `encode_auto`. Both decide for themselves
                // whether to compress and they agree on the corpus, but
                // `encode_auto` reaches the decision by building *both*
                // candidates and keeping the shorter -- which its own
                // documentation says "costs an order of magnitude", because the
                // uncompressed candidate runs the scan over data the scan has
                // plenty to find in. `encode_at` takes one histogram over a
                // kilobyte and builds only the candidate that names. That is
                // the path a caller gets, so it is the one to measure;
                // `encode_auto` is the reference the decision is checked
                // against, not a configuration anybody ships.
                //
                // DEFAULT_LEVEL is what a caller who does not pass a level
                // gets, so it is what "as it ships" means.
                encode: |d| base91z::encode_at(d, base91z::DEFAULT_LEVEL)
                    .expect("base91z encode_at"),
                decode: |s| base91z::decode(s).map_err(|e| CodecError(format!("{e:?}"))),
            }),
        },
    ]
}

// --- Base64 -------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(4 * data.len().div_ceil(3));
    // as_chunks rather than chunks_exact: the group size is a constant, so it
    // belongs in the type where the compiler can see it rather than in a
    // runtime length the indexing below has to be trusted against.
    let (groups, remainder) = data.as_chunks::<3>();
    for c in groups {
        let n = (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32;
        out.push(B64[(n >> 18) as usize & 63]);
        out.push(B64[(n >> 12) as usize & 63]);
        out.push(B64[(n >> 6) as usize & 63]);
        out.push(B64[n as usize & 63]);
    }
    match remainder {
        [a] => {
            let n = (*a as u32) << 16;
            out.push(B64[(n >> 18) as usize & 63]);
            out.push(B64[(n >> 12) as usize & 63]);
            out.extend_from_slice(b"==");
        }
        [a, b] => {
            let n = (*a as u32) << 16 | (*b as u32) << 8;
            out.push(B64[(n >> 18) as usize & 63]);
            out.push(B64[(n >> 12) as usize & 63]);
            out.push(B64[(n >> 6) as usize & 63]);
            out.push(b'=');
        }
        _ => {}
    }
    String::from_utf8(out).expect("alphabet is ASCII")
}

/// Reverse lookup for the alphabet above, built once.
///
/// A table rather than a chain of range tests, and not as a micro-optimisation:
/// Base64 is the denominator of every figure in the report, so a baseline that
/// decodes by branching four times per character would flatter every other
/// codec by the width of that handicap. The other decoders here all index a
/// table or subtract a constant; this one has to be measured on the same terms.
fn b64_table() -> &'static [i8; 256] {
    use std::sync::OnceLock;
    static T: OnceLock<[i8; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [-1i8; 256];
        for (v, &c) in B64.iter().enumerate() {
            t[c as usize] = v as i8;
        }
        t
    })
}

#[inline]
fn b64_value(table: &[i8; 256], c: u8) -> Result<u32, CodecError> {
    let v = table[c as usize];
    if v < 0 {
        return Err(CodecError(format!("base64: invalid character {:?}", c as char)));
    }
    Ok(v as u32)
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>, CodecError> {
    let b = s.as_bytes();
    let body = b.strip_suffix(b"==").map(|x| (x, 1))
        .or_else(|| b.strip_suffix(b"=").map(|x| (x, 2)))
        .unwrap_or((b, 3));
    let (body, tail) = body;
    if body.len() % 4 != 0 && (body.len() % 4) < 2 {
        return Err(CodecError("base64: truncated input".into()));
    }
    let table = b64_table();
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 3);
    let (groups, rem) = body.as_chunks::<4>();
    for c in groups {
        let n = b64_value(table, c[0])? << 18
            | b64_value(table, c[1])? << 12
            | b64_value(table, c[2])? << 6
            | b64_value(table, c[3])?;
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
    }
    if !rem.is_empty() {
        let mut n = 0u32;
        for (i, &c) in rem.iter().enumerate() {
            n |= b64_value(table, c)? << (18 - 6 * i);
        }
        for i in 0..tail.min(rem.len() - 1) {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Ok(out)
}

// --- Classic basE91 -----------------------------------------------------

// The alphabet and the encoder are taken from base91z's `examples/against.rs`,
// which already had both and a vector to check them against.
const B91: &[u8; 91] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";

pub fn base91_classic_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len() * 5 / 4 + 4);
    let (mut b, mut n) = (0u32, 0u32);
    for &byte in data {
        b |= (byte as u32) << n;
        n += 8;
        if n > 13 {
            let mut v = b & 8191;
            if v > 88 {
                b >>= 13;
                n -= 13;
            } else {
                v = b & 16383;
                b >>= 14;
                n -= 14;
            }
            out.push(B91[(v % 91) as usize]);
            out.push(B91[(v / 91) as usize]);
        }
    }
    if n > 0 {
        out.push(B91[(b % 91) as usize]);
        if n > 7 || b > 90 {
            out.push(B91[(b / 91) as usize]);
        }
    }
    String::from_utf8(out).expect("alphabet is ASCII")
}

/// Reverse lookup for the alphabet above, built once.
fn b91_table() -> &'static [i8; 256] {
    use std::sync::OnceLock;
    static T: OnceLock<[i8; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [-1i8; 256];
        for (v, &c) in B91.iter().enumerate() {
            t[c as usize] = v as i8;
        }
        t
    })
}

pub fn base91_classic_decode(s: &str) -> Result<Vec<u8>, CodecError> {
    let table = b91_table();
    let mut out = Vec::with_capacity(s.len() * 6 / 5 + 2);
    let (mut b, mut n) = (0u32, 0u32);
    let mut v: i32 = -1;
    for &c in s.as_bytes() {
        let d = table[c as usize];
        if d < 0 {
            return Err(CodecError(format!("basE91: invalid character {:?}", c as char)));
        }
        if v < 0 {
            v = d as i32;
        } else {
            let value = v as u32 + d as u32 * 91;
            b |= value << n;
            n += if value & 8191 > 88 { 13 } else { 14 };
            while n > 7 {
                out.push(b as u8);
                b >>= 8;
                n -= 8;
            }
            v = -1;
        }
    }
    if v >= 0 {
        out.push((b | (v as u32) << n) as u8);
    }
    Ok(out)
}

// --- Ascii85 ------------------------------------------------------------

pub fn ascii85_encode(data: &[u8]) -> String {
    let mut out = Vec::with_capacity(data.len() * 5 / 4 + 4);
    let (groups, rem) = data.as_chunks::<4>();
    for c in groups {
        let n = u32::from_be_bytes(*c);
        if n == 0 {
            // The shortcut that makes Ascii85 good at zero runs, and part of
            // the format rather than an optimisation: leaving it out would
            // measure a different encoding.
            out.push(b'z');
            continue;
        }
        push_base85(&mut out, n, 5);
    }
    if !rem.is_empty() {
        let mut buf = [0u8; 4];
        buf[..rem.len()].copy_from_slice(rem);
        push_base85(&mut out, u32::from_be_bytes(buf), rem.len() + 1);
    }
    String::from_utf8(out).expect("alphabet is ASCII")
}

fn push_base85(out: &mut Vec<u8>, mut n: u32, take: usize) {
    let mut digits = [0u8; 5];
    for slot in digits.iter_mut().rev() {
        *slot = b'!' + (n % 85) as u8;
        n /= 85;
    }
    out.extend_from_slice(&digits[..take]);
}

pub fn ascii85_decode(s: &str) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(s.len() * 4 / 5 + 4);
    let mut group = [0u8; 5];
    let mut have = 0usize;
    for &c in s.as_bytes() {
        if c == b'z' && have == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&c) {
            return Err(CodecError(format!("ascii85: invalid character {:?}", c as char)));
        }
        group[have] = c - b'!';
        have += 1;
        if have == 5 {
            out.extend_from_slice(&decode_group(&group, 5)?.to_be_bytes());
            have = 0;
        }
    }
    if have > 0 {
        if have == 1 {
            return Err(CodecError("ascii85: orphan character in final group".into()));
        }
        // A partial group is padded with the highest digit, then truncated.
        let mut padded = [84u8; 5];
        padded[..have].copy_from_slice(&group[..have]);
        let n = decode_group(&padded, have)?;
        out.extend_from_slice(&n.to_be_bytes()[..have - 1]);
    }
    Ok(out)
}

fn decode_group(g: &[u8; 5], filled: usize) -> Result<u32, CodecError> {
    let mut n: u64 = 0;
    for &d in g {
        n = n * 85 + d as u64;
    }
    if n > u32::MAX as u64 {
        return Err(CodecError(format!("ascii85: group of {filled} overflows 32 bits")));
    }
    Ok(n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Known vectors first, round trips second: a codec that round-trips its
    /// own mistake consistently would pass the second test and fail the first.
    #[test]
    fn known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");

        // The vector base91z's examples/against.rs already checked against.
        assert_eq!(base91_classic_encode(b"test"), "fPNKd");
        assert_eq!(base91_classic_encode(b""), "");

        // Cross-checked against Python's base64.a85encode.
        assert_eq!(ascii85_encode(b""), "");
        assert_eq!(ascii85_encode(b"Man "), "9jqo^");
        assert_eq!(ascii85_encode(b"man "), "D..<)");
        assert_eq!(ascii85_encode(b"sure"), "F*2M7");
        assert_eq!(ascii85_encode(b"easy"), "ARTY*");
        assert_eq!(ascii85_encode(&[0, 0, 0, 0]), "z");
    }

    #[test]
    fn every_codec_round_trips_every_short_length() {
        let mut s: u32 = 0xdead_beef;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s & 0xff) as u8
        };
        let data: Vec<u8> = (0..600).map(|_| next()).collect();
        for codec in all() {
            for len in 0..=600 {
                let slice = &data[..len];
                let encoded = (codec.encode)(slice);
                let back = (codec.decode)(&encoded)
                    .unwrap_or_else(|e| panic!("{} at length {len}: {e}", codec.name));
                assert_eq!(back, slice, "{} at length {len}", codec.name);
            }
        }
    }

    #[test]
    fn zero_runs_round_trip() {
        // The Ascii85 z shortcut and Base91z's run classes both key on these.
        for codec in all() {
            for len in [1, 4, 5, 8, 100, 1000] {
                let zeros = vec![0u8; len];
                assert_eq!(
                    (codec.decode)(&(codec.encode)(&zeros)).unwrap(),
                    zeros,
                    "{} on {len} zero bytes",
                    codec.name
                );
            }
        }
    }
}
