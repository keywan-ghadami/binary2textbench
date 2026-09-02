// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! results.json, written at the grain it is read.
//!
//! Measuring produces one row per (sample, codec, stage): eighty-eight samples
//! over six codecs and five stages is three thousand and eighty rows, and
//! 1.5 MB. Neither reader shows one of them. The page and `scripts/report.py`
//! both sum straight to a cell keyed by codec, stage, group and category, which
//! is also the level the page's weight sliders act on. So that is what this
//! writes, and what `--out` gets.
//!
//! The summing happens here, on the measurements while they are still in
//! memory, rather than in a pass over a written file afterwards. A runner whose
//! output is in a shape nothing reads is a step every reader has to be told
//! about, and a file on the page that nobody converted. `--raw` writes the
//! per-sample rows for a reader who wants to look at one sample rather than at
//! the report; nothing in this repository reads them.
//!
//! Four things make it small. Three of them lose nothing:
//!
//! **A value is not stored along a dimension it does not vary over.** `input`
//! is identical for every codec and every stage, so it sits in `corpus`, once
//! per (group, category). `coded_in`, `comp_out` and the compression cost do
//! not vary by codec, so they sit in `stage_sizes`. The absolute encode and
//! decode nanoseconds are only ever read summed over all categories, so they
//! sit in `codec_times`. That one change is most of the difference.
//!
//! **Ratios are stored as ratios**, not as ratio x bytes. The reader divides by
//! the bytes either way; the stored number is a third as long.
//!
//! **`json_bytes` is `raw + esc`**, so the column is not written; both readers
//! add it back. It is an invariant of the shared escaper rather than a hope --
//! every escape this corpus produces is a two-character one -- and it is
//! checked over every row before the column is dropped.
//!
//! The fourth is a real loss and is declared: ratios keep `REL` decimals, and
//! the override cells are not written -- see `PRUNE_OVERRIDE`.
//!
//! **`group` is a dimension.** It has to be: `balanced` weights `binary_short`
//! separately from `binary`, and with only a category to key on there was
//! nothing for that weight to attach to. Both readers resolve a weight by
//! trying `{cat}_{group}` before `{cat}`.
//!
//! **No display copy.** `codecs[].note`, `profiles[].label` and
//! `profiles[].note` are words for a person to read; they live beside the thing
//! that renders them. `origin` and `path` go with the per-sample rows they
//! described.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::{CompressionRow, Meta, Results, Row, SampleMeta};

/// The shape of the file. The page refuses anything else rather than guessing,
/// so this is bumped whenever a column moves, and never for a new column at the
/// end of a row -- both readers read columns through the `*_cols` lists.
pub const FORMAT: u32 = 4;

/// Decimals kept on a stored ratio. The page prints three and the report two,
/// so this is the display precision with no margin above it -- chosen knowingly
/// for the size, not because the digits were meaningless. Raise it to 4 and the
/// file grows by about six kilobytes.
const REL: i32 = 3;

/// Decimals kept on an absolute nanosecond sum. One, not zero: this corpus has
/// samples of eleven bytes whose timings sit at 31.0048 ns, and rounding sums
/// of those to whole nanoseconds lost a tenth of a per cent.
const NS: i32 = 1;

/// A codec that carries its own compression is measured twice per stage: once
/// as it ships, once with its own compression forced off so the stage's
/// compressor sits in front of it like everybody else. Both readers show the
/// first and neither reads the second, so the second is not written. This is
/// the one place a measurement is dropped rather than restructured; `--raw`
/// still has it.
const PRUNE_OVERRIDE: bool = true;

const CORPUS_COLS: [&str; 4] = ["grp", "cat", "samples", "bytes"];
const STAGE_COLS: [&str; 7] = [
    "stage",
    "grp",
    "cat",
    "coded_in",
    "comp_out",
    "comp_ns",
    "decomp_ns",
];
const TIME_COLS: [&str; 5] = ["codec", "stage", "native", "enc_ns", "dec_ns"];
/// The seven keys, then the eight ratios in the order [`Dir`] is read out in,
/// then the two that only a self-compressing codec has. A row that ends in
/// nulls is written short, so anything optional has to stay at the end.
const CELL_COLS: [&str; 17] = [
    "codec",
    "stage",
    "grp",
    "cat",
    "native",
    "raw",
    "esc",
    "enc_raw",
    "enc_q",
    "dec_raw",
    "dec_q",
    "enc_tot",
    "enc_tot_q",
    "dec_tot",
    "dec_tot_q",
    "enc_cod",
    "dec_cod",
];

/// What `--out` gets. Serialised in declaration order, which is the order it is
/// meant to be read in: what the indexes mean first, then the rows that use
/// them.
#[derive(Serialize)]
pub struct Compact<'a> {
    v: u32,
    meta: &'a Meta,
    groups: &'a [String],
    categories: Vec<&'a str>,
    stages: &'a [String],
    codecs: Vec<&'a str>,
    corpus_cols: [&'static str; 4],
    corpus: Vec<Vec<Value>>,
    stage_cols: [&'static str; 7],
    stage_sizes: Vec<Vec<Value>>,
    time_cols: [&'static str; 5],
    codec_times: Vec<Vec<Value>>,
    cell_cols: [&'static str; 17],
    cells: Vec<Vec<Value>>,
    profiles: Map<String, Value>,
}

impl Compact<'_> {
    /// How many cells the run came to, for the line the runner prints.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

/// One direction -- encode or decode -- of one cell, summed as ratio x bytes
/// and divided by the bytes at the end.
#[derive(Clone, Copy, Default)]
struct Dir {
    /// The codec's time over the baseline's, measured in the same round.
    raw: f64,
    /// The interquartile range of that ratio: how much of it is the machine.
    q: f64,
    /// The same, with the compression stage on both sides of the division.
    tot: f64,
    tot_q: f64,
    /// What the codec alone costs, for a codec that compresses on its own
    /// behalf. Equal to `raw` for every other codec, and then not written.
    cod: f64,
}

/// One (stage, group, category) row of `stage_sizes` while it is being summed.
/// Named rather than a tuple because four numbers in a row is three chances to
/// put one of them in the wrong column.
#[derive(Default)]
struct StageSize {
    coded_in: u64,
    comp_out: u64,
    comp_ns: f64,
    decomp_ns: f64,
}

/// One (codec, stage, group, category) cell while it is being summed.
#[derive(Default)]
struct Cell {
    /// Original sample bytes: what every ratio here is weighted by.
    input: u64,
    raw: u64,
    esc: u64,
    /// Encode, then decode.
    dir: [Dir; 2],
}

/// Sums the per-sample measurements into the file both readers read.
pub fn of(r: &Results) -> Compact<'_> {
    let groups = &r.corpus.groups;
    let gi = index(groups.iter().map(String::as_str));
    let mut categories: Vec<&str> = Vec::new();
    for s in &r.corpus.samples {
        if !categories.contains(&s.category.as_str()) {
            categories.push(&s.category);
        }
    }
    let ci = index(categories.iter().copied());
    let si = index(r.stages.iter().map(String::as_str));
    let codecs: Vec<&str> = r.codecs.iter().map(|c| c.name.as_str()).collect();
    let ki = index(codecs.iter().copied());

    let samples: BTreeMap<&str, &SampleMeta> = r
        .corpus
        .samples
        .iter()
        .map(|s| (s.name.as_str(), s))
        .collect();
    let comp: BTreeMap<(&str, &str), &CompressionRow> = r
        .compression
        .iter()
        .map(|c| ((c.sample.as_str(), c.stage.as_str()), c))
        .collect();

    // Every ratio in the file is against the baseline codec measured on the
    // same sample at the same stage, so a missing baseline row is not a cell
    // that comes out wrong -- it is nothing to divide by.
    let base: BTreeMap<(&str, &str), &Row> = r
        .measurements
        .iter()
        .filter(|m| m.codec == r.meta.baseline && !m.native)
        .map(|m| ((m.sample.as_str(), m.stage.as_str()), m))
        .collect();

    for m in &r.measurements {
        assert_eq!(
            m.json_bytes,
            m.encoded_bytes + m.escapes,
            "{} on {} at {}: the escaper produced a character that costs more \
             than one byte to escape, so json_bytes is not raw + esc and the \
             column cannot be dropped. Add it back to CELL_COLS.",
            m.codec,
            m.sample,
            m.stage
        );
        assert!(
            base.contains_key(&(m.sample.as_str(), m.stage.as_str())),
            "no {} row for {} at {}; every ratio is against it",
            r.meta.baseline,
            m.sample,
            m.stage
        );
    }

    let native_codecs: BTreeSet<&str> = r
        .measurements
        .iter()
        .filter(|m| m.native)
        .map(|m| m.codec.as_str())
        .collect();
    let keep = |m: &Row| {
        if !PRUNE_OVERRIDE {
            return true;
        }
        // For a codec that has a native path, the native rows are the ones the
        // report shows; for every other codec there are no native rows at all.
        if native_codecs.contains(m.codec.as_str()) {
            m.native
        } else {
            !m.native
        }
    };

    let mut corpus: BTreeMap<(&str, &str), (u64, u64)> = BTreeMap::new();
    for s in &r.corpus.samples {
        let a = corpus.entry((&s.group, &s.category)).or_default();
        a.0 += 1;
        a.1 += s.bytes as u64;
    }

    let mut stage_sizes: BTreeMap<(&str, &str, &str), StageSize> = BTreeMap::new();
    for c in &r.compression {
        let s = sample(&samples, &c.sample);
        let a = stage_sizes
            .entry((&c.stage, &s.group, &s.category))
            .or_default();
        a.coded_in += c.input_bytes as u64;
        a.comp_out += c.output_bytes as u64;
        a.comp_ns += c.compress.ns;
        a.decomp_ns += c.decompress.ns;
    }

    let mut times: BTreeMap<(&str, &str, u8), (f64, f64)> = BTreeMap::new();
    let mut cells: BTreeMap<(&str, &str, &str, &str, u8), Cell> = BTreeMap::new();
    for m in r.measurements.iter().filter(|m| keep(m)) {
        let s = sample(&samples, &m.sample);
        let nat = u8::from(m.native);
        let t = times.entry((&m.codec, &m.stage, nat)).or_default();
        t.0 += m.encode.ns;
        t.1 += m.decode.ns;

        let e = cells
            .entry((&m.codec, &m.stage, &s.group, &s.category, nat))
            .or_default();
        let n = m.input_bytes as f64;
        e.input += m.input_bytes as u64;
        e.raw += m.encoded_bytes as u64;
        e.esc += m.escapes as u64;

        let b = base[&(m.sample.as_str(), m.stage.as_str())];
        let cp = comp
            .get(&(m.sample.as_str(), m.stage.as_str()))
            .unwrap_or_else(|| panic!("no compression row for {} at {}", m.sample, m.stage));
        // The three views the page's controls offer, formed per sample because
        // the page forms them per sample: dividing sums instead would be a
        // different number. Only the products are accumulated.
        let each = [
            (m.encode.ns, b.encode.ns, m.encode_rel, cp.compress.ns),
            (m.decode.ns, b.decode.ns, m.decode_rel, cp.decompress.ns),
        ];
        for (d, (own, oth, rel, cns)) in e.dir.iter_mut().zip(each) {
            d.raw += rel.ns * n;
            d.q += rel.iqr_ns * n;
            let cod = if m.native {
                if oth != 0.0 {
                    own / oth
                } else {
                    0.0
                }
            } else {
                rel.ns
            };
            d.cod += cod * n;
            let mine = own + if m.native { 0.0 } else { cns };
            let theirs = oth + cns;
            d.tot += (if theirs != 0.0 { mine / theirs } else { 0.0 }) * n;
            d.tot_q += rel.iqr_ns * (if mine != 0.0 { own / mine } else { 1.0 }) * n;
        }
    }

    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(cells.len());
    for ((codec, stage, g, c, nat), e) in &cells {
        let n = if e.input == 0 { 1.0 } else { e.input as f64 };
        let [enc, dec] = e.dir;
        let mut row = vec![
            at(&ki, codec).into(),
            at(&si, stage).into(),
            at(&gi, g).into(),
            at(&ci, c).into(),
            (*nat).into(),
            e.raw.into(),
            e.esc.into(),
        ];
        for v in [
            enc.raw, enc.q, dec.raw, dec.q, enc.tot, enc.tot_q, dec.tot, dec.tot_q,
        ] {
            row.push(round(v / n, REL).into());
        }
        for v in [enc.cod, dec.cod] {
            row.push(if *nat == 1 {
                round(v / n, REL).into()
            } else {
                Value::Null
            });
        }
        while row.last() == Some(&Value::Null) {
            row.pop();
        }
        rows.push(row);
    }

    Compact {
        v: FORMAT,
        meta: &r.meta,
        groups,
        categories,
        stages: &r.stages,
        codecs,
        corpus_cols: CORPUS_COLS,
        corpus: corpus
            .iter()
            .map(|((g, c), a)| vec![at(&gi, g).into(), at(&ci, c).into(), a.0.into(), a.1.into()])
            .collect(),
        stage_cols: STAGE_COLS,
        stage_sizes: stage_sizes
            .iter()
            .map(|((st, g, c), a)| {
                vec![
                    at(&si, st).into(),
                    at(&gi, g).into(),
                    at(&ci, c).into(),
                    a.coded_in.into(),
                    a.comp_out.into(),
                    round(a.comp_ns, NS).into(),
                    round(a.decomp_ns, NS).into(),
                ]
            })
            .collect(),
        time_cols: TIME_COLS,
        codec_times: times
            .iter()
            .map(|((k, st, nat), a)| {
                vec![
                    at(&ki, k).into(),
                    at(&si, st).into(),
                    (*nat).into(),
                    round(a.0, NS).into(),
                    round(a.1, NS).into(),
                ]
            })
            .collect(),
        cell_cols: CELL_COLS,
        cells: rows,
        profiles: weights(&r.profiles),
    }
}

/// The weights, and nothing else. A profile's label and note are words for a
/// person to read and belong beside the control that renders them; a weight of
/// zero is the default the readers already apply to a category a profile does
/// not mention, so it is not written either.
fn weights(profiles: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    let Some(table) = profiles.get("profile").and_then(Value::as_object) else {
        return out;
    };
    for (name, p) in table {
        let mut kept = Map::new();
        if let Some(w) = p.get("weights").and_then(Value::as_object) {
            for (k, v) in w {
                if v.as_f64().is_some_and(|f| f != 0.0) {
                    kept.insert(k.clone(), v.clone());
                }
            }
        }
        out.insert(name.clone(), Value::Object(kept));
    }
    out
}

fn index<'a>(names: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    names.enumerate().map(|(i, n)| (n, i)).collect()
}

/// A name's position in the header list it has to be in. Absent means the file
/// would carry an index pointing at nothing, so it is a panic and not a hole.
fn at(m: &BTreeMap<&str, usize>, name: &str) -> usize {
    *m.get(name)
        .unwrap_or_else(|| panic!("{name:?} is not in the list the rows index into"))
}

fn sample<'a>(samples: &BTreeMap<&str, &'a SampleMeta>, name: &str) -> &'a SampleMeta {
    samples
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("{name} was measured but is not in the manifest"))
}

/// Rounded for the file, not for arithmetic: everything above is summed at full
/// precision and only what is written is cut.
fn round(x: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (x * f).round() / f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CodecMeta, Manifest};
    use b2t_runner::timing::Summary;
    use serde_json::json;

    fn s(ns: f64, iqr_ns: f64) -> Summary {
        Summary {
            ns,
            iqr_ns,
            rounds: 1,
        }
    }

    fn sample(name: &str, bytes: usize) -> SampleMeta {
        SampleMeta {
            name: name.into(),
            group: "core".into(),
            category: "text".into(),
            bytes,
            sha256: String::new(),
            origin: String::new(),
            path: String::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        name: &str,
        codec: &str,
        native: bool,
        bytes: usize,
        encoded: usize,
        escapes: usize,
        enc: (f64, f64, f64),
        dec: (f64, f64, f64),
    ) -> Row {
        Row {
            sample: name.into(),
            codec: codec.into(),
            stage: "zstd:1".into(),
            native,
            coded_input_bytes: bytes / 2,
            input_bytes: bytes,
            encoded_bytes: encoded,
            json_bytes: encoded + escapes,
            escapes,
            encode: s(enc.0, 0.0),
            decode: s(dec.0, 0.0),
            encode_rel: s(enc.1, enc.2),
            decode_rel: s(dec.1, dec.2),
        }
    }

    /// Two samples of different sizes, a baseline codec and one that carries
    /// its own compression -- which is the case every rule in this module is
    /// about. The numbers are made up and none of them is a round 1.0, so a
    /// column read from the wrong place shows up as a wrong number rather than
    /// as the same number.
    fn results() -> Results {
        let mut measurements = Vec::new();
        for (name, bytes, encoded) in [("a", 100usize, 10usize), ("b", 300, 30)] {
            measurements.push(row(
                name,
                "base64",
                false,
                bytes,
                encoded,
                encoded / 10,
                (100.0, 1.0, 0.03),
                (400.0, 1.0, 0.04),
            ));
            // As it ships: the row the report shows.
            measurements.push(row(
                name,
                "fancy",
                true,
                bytes,
                encoded / 2,
                0,
                (50.0, 0.4, 0.1),
                (100.0, 0.2, 0.2),
            ));
            // The same codec with its own compression forced off. Nothing
            // reads it, so nothing here may contain it -- and it is absurd on
            // purpose, so that leaking it would be unmissable.
            measurements.push(row(
                name,
                "fancy",
                false,
                bytes,
                9999,
                0,
                (9999.0, 99.0, 99.0),
                (9999.0, 99.0, 99.0),
            ));
        }
        Results {
            meta: Meta {
                generated: "now".into(),
                rounds: 1,
                rustc: String::new(),
                cpu: String::new(),
                codec_revisions: BTreeMap::new(),
                baseline: "base64",
            },
            corpus: Manifest {
                groups: vec!["core".into()],
                samples: vec![sample("a", 100), sample("b", 300)],
            },
            codecs: vec![
                CodecMeta {
                    name: "base64".into(),
                    note: "words".into(),
                },
                CodecMeta {
                    name: "fancy".into(),
                    note: "words".into(),
                },
            ],
            stages: vec!["zstd:1".into()],
            profiles: json!({"profile": {"p": {
                "label": "words", "weights": {"text": 2.0, "binary": 0.0}}}}),
            compression: vec![
                CompressionRow {
                    sample: "a".into(),
                    stage: "zstd:1".into(),
                    input_bytes: 100,
                    output_bytes: 50,
                    compress: s(200.0, 0.0),
                    decompress: s(100.0, 0.0),
                },
                CompressionRow {
                    sample: "b".into(),
                    stage: "zstd:1".into(),
                    input_bytes: 300,
                    output_bytes: 150,
                    compress: s(200.0, 0.0),
                    decompress: s(100.0, 0.0),
                },
            ],
            measurements,
        }
    }

    #[test]
    fn sums_to_the_cells_both_readers_read() {
        let out = serde_json::to_value(of(&results())).expect("serialise");

        assert_eq!(out["v"], json!(FORMAT));
        assert_eq!(out["corpus"], json!([[0, 0, 2, 400]]));
        assert_eq!(
            out["stage_sizes"],
            json!([[0, 0, 0, 400, 200, 400.0, 200.0]])
        );
        // Two rows, not three: the override cells are not written, so the
        // absurd numbers above are nowhere in the file.
        assert_eq!(
            out["codec_times"],
            json!([[0, 0, 0, 200.0, 800.0], [1, 0, 1, 100.0, 200.0]])
        );
        assert_eq!(
            out["cells"],
            json!([
                // base64: 1.0 against itself, and the stage on both sides of
                // the total. Written short, because the last two columns are
                // for a codec that decides its own compression.
                [0, 0, 0, 0, 0, 40, 4, 1.0, 0.03, 1.0, 0.04, 1.0, 0.01, 1.0, 0.032],
                // fancy: 50/(100+200) encoding, 100/(400+100) decoding, and
                // enc_cod/dec_cod as measured against the baseline rather than
                // as the relative reading, because the stage is inside it.
                [1, 0, 0, 0, 1, 20, 0, 0.4, 0.1, 0.2, 0.2, 0.167, 0.1, 0.2, 0.2, 0.5, 0.25],
            ])
        );
        // The weights, and only the ones that weigh anything.
        assert_eq!(out["profiles"], json!({"p": {"text": 2.0}}));
    }

    #[test]
    #[should_panic(expected = "json_bytes is not raw + esc")]
    fn refuses_to_drop_a_column_that_is_not_implied() {
        let mut r = results();
        r.measurements[0].json_bytes += 1;
        of(&r);
    }
}
