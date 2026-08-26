// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The JSON string escaper every codec is measured through.
//!
//! An encoded payload does not travel as itself; it travels inside something,
//! and for most callers that something is a JSON string. Escaping is therefore
//! part of the pipeline, and it is part of the pipeline for every codec, so it
//! is timed rather than deducted.
//!
//! There is one escaper and all six codecs go through it. That matters: if each
//! codec brought its own, a codec could win by having a better-optimised
//! escaper rather than a better alphabet, and the alphabet is the thing being
//! compared. With one shared implementation, the only way to spend less time
//! here is to hand it fewer characters that need escaping.
//!
//! The rule is RFC 8259's minimum, which is what `serde_json` and
//! `JSON.stringify` emit: `"` and `\` take a backslash, the C0 controls take
//! their short form where they have one and `\u00XX` otherwise, and everything
//! else is copied through. Non-ASCII is *not* escaped -- but no codec here
//! emits any, so that branch never fires and costs nothing.

/// The two-character escapes, indexed by byte. `0` means "not escaped this way".
static SHORT: [u8; 0x80] = {
    let mut t = [0u8; 0x80];
    t[0x08] = b'b';
    t[0x09] = b't';
    t[0x0a] = b'n';
    t[0x0c] = b'f';
    t[0x0d] = b'r';
    t[b'"' as usize] = b'"';
    t[b'\\' as usize] = b'\\';
    t
};

#[inline]
fn needs_escape(b: u8) -> bool {
    b < 0x20 || b == b'"' || b == b'\\'
}

/// Appends `s` to `out` as it would appear between the quotes of a JSON string.
///
/// The quotes themselves are not written: what is being measured is the payload,
/// and two characters of container are the same two characters for everyone.
pub fn escape_into(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    let mut start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if !needs_escape(b) {
            continue;
        }
        // Copy the clean run in one go; this is the path that carries almost
        // all of the bytes for almost every codec.
        out.push_str(&s[start..i]);
        let short = SHORT[b as usize];
        if short != 0 {
            out.push('\\');
            out.push(short as char);
        } else {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.push_str("\\u00");
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0xf) as usize] as char);
        }
        start = i + 1;
    }
    out.push_str(&s[start..]);
}

/// Reverses `escape_into`. Used on the decode side, where unescaping is as much
/// a part of the pipeline as escaping is on the encode side.
pub fn unescape(s: &str) -> Result<String, String> {
    if !s.as_bytes().contains(&b'\\') {
        return Ok(s.to_owned());
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\\' {
                i += 1;
            }
            out.push_str(&s[start..i]);
            continue;
        }
        i += 1;
        let tag = *bytes.get(i).ok_or("escape at end of string")?;
        i += 1;
        match tag {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{08}'),
            b'f' => out.push('\u{0c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => {
                let hex = s.get(i..i + 4).ok_or("truncated \\u escape")?;
                let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                out.push(char::from_u32(cp).ok_or("not a scalar value")?);
                i += 4;
            }
            other => return Err(format!("unknown escape \\{}", other as char)),
        }
    }
    Ok(out)
}

/// How many characters of the output the escaper had to touch. This is the
/// number that explains a size difference between two codecs whose raw output
/// is the same length.
pub fn escape_count(s: &str) -> usize {
    s.bytes().filter(|&b| needs_escape(b)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape(s: &str) -> String {
        let mut out = String::new();
        escape_into(s, &mut out);
        out
    }

    #[test]
    fn matches_serde_json() {
        // serde_json is the reference for what a caller's JSON encoder does.
        // Every byte that can appear in any of these alphabets, plus the
        // controls that cannot but would be wrong to get wrong.
        let all: String = (0u8..0x80).map(|b| b as char).collect();
        for case in [
            "",
            "plain",
            "quote\"here",
            "back\\slash",
            "tab\tnewline\n",
            &all,
        ] {
            let mine = format!("\"{}\"", escape(case));
            let theirs = serde_json::to_string(case).unwrap();
            assert_eq!(mine, theirs, "escaping {case:?}");
            assert_eq!(unescape(&escape(case)).unwrap(), case, "round trip {case:?}");
        }
    }

    #[test]
    fn counts_what_it_touched() {
        assert_eq!(escape_count("abc"), 0);
        assert_eq!(escape_count("a\"b\\c"), 2);
    }
}
