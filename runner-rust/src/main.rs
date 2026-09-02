// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Measures every encoding against every sample and writes results.json.
//!
//! What is measured is the whole path a payload takes, because that is what a
//! caller pays:
//!
//! ```text
//!   encode:  bytes -> [zstd] -> codec -> JSON escaping -> the string on the wire
//!   decode:  the string -> JSON unescaping -> codec -> [zstd] -> bytes
//! ```
//!
//! The JSON step is inside the clock, not deducted from it. An encoding whose
//! alphabet contains `"` pays for that twice -- once in the characters it adds
//! and once in the work of adding them -- and leaving the second cost out would
//! flatter it. There is one escaper, shared by every codec (see `json`), so
//! what differs between codecs is how much work they hand it.
//!
//! The compression stage is timed once per (sample, level) rather than once per
//! codec, because it is the same bytes doing the same work whichever codec
//! comes after it, and with a context kept for the whole run rather than built
//! per call -- see `Stages`, where the reason is that a discarded `ZSTD_DCtx`
//! is not a property of an encoding. results.json carries the two costs
//! separately and the site adds them; that way a reader can ask for the total,
//! or for the codec alone, without a second run.
//!
//! A codec that decides for itself whether to compress is timed whole, at the
//! level the stage names -- so it has one cell per stage like everybody else,
//! marked `native`, and a compression setting means one thing across the whole
//! report. Its measurements behind the stage's compressor are kept too, marked
//! `native: false`, for a reader who wants the container on its own.
//!
//! Nothing is written that did not come back: every cell round-trips through
//! the full pipeline and is compared against the input before it is timed.
//!
//! What is written is the shape the page and the report read: sums per (codec,
//! stage, group, category), not the per-sample rows they are formed from. See
//! `compact`, which does the summing and says what each column is. `--raw`
//! writes the per-sample rows as well, for looking at one sample.
//!
//!     b2t-runner --manifest ../corpus/data/manifest.json --out ../results.json

mod compact;

use b2t_runner::{codecs, json, timing};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use timing::{time_once, Summary};

/// The compression stages every codec is measured behind. `None` is here as its
/// own row rather than as a baseline to subtract: for a short payload or an
/// incompressible one it is what a caller should actually do, and the report
/// should be able to say so.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    None,
    Zstd(i32),
}

impl Stage {
    fn label(&self) -> String {
        match self {
            Stage::None => "none".into(),
            Stage::Zstd(l) => format!("zstd:{l}"),
        }
    }

    fn apply(&self, data: &[u8]) -> Vec<u8> {
        match self {
            Stage::None => data.to_vec(),
            Stage::Zstd(l) => zstd::bulk::compress(data, *l).expect("zstd compress"),
        }
    }

    fn undo(&self, data: &[u8], hint: usize) -> Vec<u8> {
        match self {
            Stage::None => data.to_vec(),
            Stage::Zstd(_) => zstd::bulk::decompress(data, hint.max(1)).expect("zstd decompress"),
        }
    }
}

// -5 for the fastest setting anyone uses, 1 for the everyday default, 9 and 19
// for the two points on the curve where a caller trades real time for real
// size. Four levels and no compression is five rows per codec; more would make
// the table unreadable without saying anything new.
const STAGES: [Stage; 5] = [
    Stage::None,
    Stage::Zstd(-5),
    Stage::Zstd(1),
    Stage::Zstd(9),
    Stage::Zstd(19),
];

// --- the corpus manifest ------------------------------------------------

#[derive(Deserialize, Serialize, Clone)]
struct Manifest {
    groups: Vec<String>,
    samples: Vec<SampleMeta>,
}

#[derive(Deserialize, Serialize, Clone)]
struct SampleMeta {
    name: String,
    group: String,
    category: String,
    bytes: usize,
    sha256: String,
    origin: String,
    path: String,
}

// --- what gets measured -------------------------------------------------

/// Everything the run produced, one row per measurement. This is not the shape
/// that gets written: both readers sum these rows to a cell per (codec, stage,
/// group, category) and show none of them individually, so `compact` does that
/// summing here and `--out` gets the result. `--raw` writes this, for a reader
/// who wants a single sample rather than the report.
#[derive(Serialize)]
struct Results {
    meta: Meta,
    corpus: Manifest,
    codecs: Vec<CodecMeta>,
    stages: Vec<String>,
    profiles: serde_json::Value,
    /// One entry per (sample, stage): the cost of the compression stage alone,
    /// shared by every codec measured behind it.
    compression: Vec<CompressionRow>,
    /// One entry per (sample, codec, stage): the cost of the codec and the JSON
    /// escaping, and the sizes that come out.
    measurements: Vec<Row>,
}

#[derive(Serialize)]
struct Meta {
    generated: String,
    rounds: usize,
    rustc: String,
    cpu: String,
    codec_revisions: BTreeMap<String, String>,
    /// The name of the codec every ratio on the site is taken against.
    baseline: &'static str,
}

#[derive(Serialize)]
struct CodecMeta {
    name: String,
    note: String,
}

#[derive(Serialize)]
struct CompressionRow {
    sample: String,
    stage: String,
    input_bytes: usize,
    output_bytes: usize,
    compress: Summary,
    decompress: Summary,
}

#[derive(Serialize)]
struct Row {
    sample: String,
    codec: String,
    stage: String,
    /// True where the codec carried the compression itself rather than being
    /// measured behind the stage. Base91z has both: this is the one the report
    /// shows, because it is the one a caller of it gets.
    native: bool,
    /// Bytes handed to the codec: the sample, or what the stage made of it.
    coded_input_bytes: usize,
    /// The original sample size, which every ratio worth reading is against.
    input_bytes: usize,
    /// The encoded string before escaping. The secondary size figure.
    encoded_bytes: usize,
    /// The encoded string as it appears inside a JSON string. The primary one.
    json_bytes: usize,
    /// How many characters the escaper had to touch. Explains the gap above.
    escapes: usize,
    /// Codec encode plus JSON escaping, in nanoseconds. Honest, but the number
    /// to distrust: on a shared runner it carries the machine's mood.
    encode: Summary,
    /// JSON unescaping plus codec decode, in nanoseconds.
    decode: Summary,
    /// The same two figures divided by Base64's, measured in the same round and
    /// bracketed around them in time. These are what the report leads with,
    /// because they hold still when the machine does not. 1.0 is Base64;
    /// above 1.0 is slower than Base64.
    encode_rel: Summary,
    decode_rel: Summary,
}

// --- the run ------------------------------------------------------------

/// The compression stage for one sample at one level. Measured once and shared
/// by every codec behind it: it is the same bytes doing the same work whichever
/// codec follows, so measuring it per codec would only multiply the noise.
struct CompressionCell {
    sample: usize,
    stage: Stage,
    output: Vec<u8>,
    compress_rounds: Vec<f64>,
    decompress_rounds: Vec<f64>,
}

/// One thing to measure. Built up front so that a round can walk them in a
/// rotated order without deciding anything while the clock is running.
struct Cell {
    sample: usize,
    codec: usize,
    stage: Stage,
    /// Set for a codec that compresses on its own behalf; then `stage` is
    /// cosmetic and the codec is handed the raw sample.
    native: bool,
    /// What the codec is given: the sample, or the compressed sample.
    input: Vec<u8>,
    encoded: String,
    escaped: String,
    /// Nanoseconds per operation, one entry per round.
    encode_rounds: Vec<f64>,
    decode_rounds: Vec<f64>,
    /// The same measurement divided by Base64's, taken in the same round and
    /// bracketed around it in time. This is the figure the report leads with;
    /// see `measure_group` for why it is formed here and not afterwards.
    encode_rel_rounds: Vec<f64>,
    decode_rel_rounds: Vec<f64>,
}

fn main() {
    let args = Args::parse();

    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(&args.manifest).unwrap_or_else(|e| {
            panic!(
                "{}: {e}\n  run: python3 corpus/manifest.py --groups=core,short,synthetic",
                args.manifest.display()
            )
        }),
    )
    .expect("manifest is JSON");

    let root = args.manifest.parent().expect("manifest has a directory");
    let samples: Vec<(SampleMeta, Vec<u8>)> = manifest
        .samples
        .iter()
        .filter(|s| args.groups.is_empty() || args.groups.contains(&s.group))
        .map(|s| {
            let path = root.join(&s.path);
            let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(
                data.len(),
                s.bytes,
                "{} changed size since the manifest",
                s.name
            );
            (s.clone(), data)
        })
        .collect();
    assert!(!samples.is_empty(), "no samples selected");

    let codecs = codecs::all();
    let total_bytes: usize = samples.iter().map(|(_, d)| d.len()).sum();
    eprintln!(
        "{} samples, {total_bytes} bytes, {} codecs, {} stages, {} rounds",
        samples.len(),
        codecs.len(),
        STAGES.len(),
        args.rounds
    );

    // --- build the cells, verifying each one round-trips ----------------
    eprintln!("checking round trips ...");
    let started = Instant::now();
    let mut cells: Vec<Cell> = Vec::new();
    let mut compression: Vec<CompressionCell> = Vec::new();

    for (si, (meta, data)) in samples.iter().enumerate() {
        for stage in STAGES {
            let compressed = stage.apply(data);
            for (ci, codec) in codecs.iter().enumerate() {
                let input = compressed.clone();
                let encoded = (codec.encode)(&input);
                let mut escaped = String::with_capacity(encoded.len());
                json::escape_into(&encoded, &mut escaped);

                // The whole pipeline, backwards, compared against the input.
                // A codec that cannot return the bytes is not measured; it is
                // a bug report.
                let unescaped = json::unescape(&escaped)
                    .unwrap_or_else(|e| panic!("{}: unescape: {e}", codec.name));
                let decoded = (codec.decode)(&unescaped)
                    .unwrap_or_else(|e| panic!("{} on {}: {e}", codec.name, meta.name));
                let restored = stage.undo(&decoded, data.len());
                assert_eq!(
                    restored,
                    *data,
                    "{} + {} did not return {}",
                    codec.name,
                    stage.label(),
                    meta.name
                );

                cells.push(Cell {
                    sample: si,
                    codec: ci,
                    stage,
                    native: false,
                    input,
                    encoded,
                    escaped,
                    encode_rounds: Vec::with_capacity(args.rounds),
                    decode_rounds: Vec::with_capacity(args.rounds),
                    encode_rel_rounds: Vec::with_capacity(args.rounds),
                    decode_rel_rounds: Vec::with_capacity(args.rounds),
                });
            }
            compression.push(CompressionCell {
                sample: si,
                stage,
                output: compressed,
                compress_rounds: Vec::with_capacity(args.rounds),
                decompress_rounds: Vec::with_capacity(args.rounds),
            });
        }

        // The codecs that ship their own compression decision, measured as
        // they ship: raw bytes in, one call, nothing bolted on -- but at the
        // level the stage names, so that picking a compression setting in the
        // report means the same thing for every row in it. One cell per stage,
        // the same as every other codec has.
        for stage in STAGES {
            for (ci, codec) in codecs.iter().enumerate() {
                let Some(native) = &codec.native else {
                    continue;
                };
                let level = match stage {
                    Stage::None => None,
                    Stage::Zstd(l) => Some(l),
                };
                let encoded = (native.encode)(data, level);
                let mut escaped = String::with_capacity(encoded.len());
                json::escape_into(&encoded, &mut escaped);
                let unescaped = json::unescape(&escaped).expect("unescape");
                let decoded = (native.decode)(&unescaped).unwrap_or_else(|e| {
                    panic!(
                        "{} native at {} on {}: {e}",
                        codec.name,
                        stage.label(),
                        meta.name
                    )
                });
                assert_eq!(
                    decoded,
                    *data,
                    "{} native at {} did not return {}",
                    codec.name,
                    stage.label(),
                    meta.name
                );

                cells.push(Cell {
                    sample: si,
                    codec: ci,
                    stage,
                    native: true,
                    input: data.clone(),
                    encoded,
                    escaped,
                    encode_rounds: Vec::with_capacity(args.rounds),
                    decode_rounds: Vec::with_capacity(args.rounds),
                    encode_rel_rounds: Vec::with_capacity(args.rounds),
                    decode_rel_rounds: Vec::with_capacity(args.rounds),
                });
            }
        }
    }
    eprintln!(
        "  {} cells, all round-tripped, in {:.1} s",
        cells.len(),
        started.elapsed().as_secs_f64()
    );

    // Group the cells by what they are measured against. A codec's reading is
    // divided by a Base64 reading over the same input, so a group is one sample
    // at one compression stage -- and a native cell, which takes the raw sample,
    // belongs with the uncompressed group for the same reason.
    let baseline = codecs
        .iter()
        .position(|c| c.name == "base64")
        .expect("base64 is the baseline and must be present");
    let mut groups: BTreeMap<(usize, String), Vec<usize>> = BTreeMap::new();
    for (i, cell) in cells.iter().enumerate() {
        groups
            .entry((cell.sample, cell.stage.label()))
            .or_default()
            .push(i);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();

    // --- the rounds -----------------------------------------------------
    // Warm-up first, discarded: the first pass pays for page faults and for
    // whatever the allocator has to ask the kernel for, and none of that is a
    // property of an encoding.
    eprintln!("warming up ...");
    let sources: Vec<Vec<u8>> = samples.iter().map(|(_, d)| d.clone()).collect();
    let widest = sources.iter().map(Vec::len).max().unwrap_or(0);
    let mut stages = Stages::new(
        STAGES.iter().filter_map(|s| match s {
            Stage::Zstd(l) => Some(*l),
            Stage::None => None,
        }),
        widest,
    );
    for group in &groups {
        measure_group(&mut cells, group, baseline, &codecs, false);
    }
    measure_compression(&mut compression, &sources, &mut stages, false);

    for round in 0..args.rounds {
        let t = Instant::now();
        for group in &mut groups {
            // Rotate the order within the group. A codec measured first in
            // every round is the one that always pays for a cold cache;
            // rotating moves that cost around rather than parking it on one
            // name. The baseline is found by name, not by position, so this
            // does not disturb the bracketing.
            let by = round % group.len().max(1);
            group.rotate_left(by);
            measure_group(&mut cells, group, baseline, &codecs, true);
        }
        measure_compression(&mut compression, &sources, &mut stages, true);
        eprintln!(
            "round {}/{} in {:.1} s",
            round + 1,
            args.rounds,
            t.elapsed().as_secs_f64()
        );
    }

    // --- collect --------------------------------------------------------
    let mut measurements: Vec<Row> = cells
        .iter_mut()
        .map(|cell| {
            let (meta, data) = &samples[cell.sample];
            let codec = &codecs[cell.codec];
            Row {
                sample: meta.name.clone(),
                codec: codec.name.to_string(),
                stage: cell.stage.label(),
                native: cell.native,
                coded_input_bytes: cell.input.len(),
                input_bytes: data.len(),
                encoded_bytes: cell.encoded.len(),
                json_bytes: cell.escaped.len(),
                escapes: json::escape_count(&cell.encoded),
                encode: Summary::of(&mut cell.encode_rounds),
                decode: Summary::of(&mut cell.decode_rounds),
                encode_rel: Summary::of(&mut cell.encode_rel_rounds),
                decode_rel: Summary::of(&mut cell.decode_rel_rounds),
            }
        })
        .collect();
    measurements.sort_by(|a, b| {
        (&a.sample, &a.codec, &a.stage, a.native).cmp(&(&b.sample, &b.codec, &b.stage, b.native))
    });

    let mut compression_rows: Vec<CompressionRow> = compression
        .iter_mut()
        .map(|c| CompressionRow {
            sample: samples[c.sample].0.name.clone(),
            stage: c.stage.label(),
            input_bytes: samples[c.sample].1.len(),
            output_bytes: c.output.len(),
            compress: Summary::of(&mut c.compress_rounds),
            decompress: Summary::of(&mut c.decompress_rounds),
        })
        .collect();
    compression_rows.sort_by(|a, b| (&a.sample, &a.stage).cmp(&(&b.sample, &b.stage)));

    let results = Results {
        meta: Meta {
            generated: iso_now(),
            rounds: args.rounds,
            rustc: run("rustc", &["--version"]),
            cpu: cpu_model(),
            codec_revisions: codec_revisions(),
            baseline: "base64",
        },
        corpus: Manifest {
            groups: manifest.groups.clone(),
            samples: samples.iter().map(|(m, _)| m.clone()).collect(),
        },
        // One entry per codec, because the report shows one row per codec. A
        // codec with a native path is described by that path, since that is the
        // row: its measurements behind the stage are still in `measurements`,
        // marked `native: false`, for a reader who wants the container on its
        // own.
        codecs: codecs
            .iter()
            .map(|c| CodecMeta {
                name: c.name.into(),
                note: c.native.as_ref().map_or(c.note, |n| n.note).into(),
            })
            .collect(),
        stages: STAGES.iter().map(|s| s.label()).collect(),
        profiles: read_profiles(&args.profiles),
        compression: compression_rows,
        measurements,
    };

    // The per-sample rows first, where they were asked for: if summing them
    // trips one of its own checks, the numbers it was given are still on disk.
    if let Some(path) = &args.raw {
        write(path, &results);
        eprintln!(
            "wrote {} ({} measurements, {} compression rows)",
            path.display(),
            results.measurements.len(),
            results.compression.len()
        );
    }

    let summed = compact::of(&results);
    write(&args.out, &summed);
    eprintln!(
        "wrote {} (v{}, {} cells over {} measurements)",
        args.out.display(),
        compact::FORMAT,
        summed.cell_count(),
        results.measurements.len()
    );
}

/// One pass over every cell, grouped so that each measurement can be divided by
/// a Base64 measurement taken seconds away from it rather than minutes.
///
/// This is where the noise handling actually lives, and the shape of it matters
/// more than it looks. Dividing a codec's median by Base64's median at the end
/// of a run does *not* cancel machine noise -- it adds it, because two
/// independently jittery numbers make a more jittery quotient. Cancellation
/// needs the two measurements to see the *same* disturbance, which means taking
/// them close together.
///
/// So each group -- one sample at one compression stage -- is measured as:
///
/// ```text
///   base64, every other codec in turn, base64 again
/// ```
///
/// and the denominator is the mean of the two Base64 readings. That brackets
/// every codec's measurement in time, so a machine that is drifting over the
/// round drifts through the numerator and the denominator alike and the ratio
/// holds still. Base64 is the cheapest codec here, so measuring it twice per
/// group costs little.
fn measure_group(
    cells: &mut [Cell],
    group: &[usize],
    baseline: usize,
    codecs: &[codecs::Codec],
    record: bool,
) {
    let base_cell = group
        .iter()
        .copied()
        .find(|&i| cells[i].codec == baseline && !cells[i].native)
        .expect("every group has a baseline cell");

    let (enc_before, dec_before) = time_cell(&mut cells[base_cell], codecs);
    let mut readings: Vec<(usize, f64, f64)> = Vec::with_capacity(group.len());
    for &i in group {
        if i == base_cell {
            continue;
        }
        let (e, d) = time_cell(&mut cells[i], codecs);
        readings.push((i, e, d));
    }
    let (enc_after, dec_after) = time_cell(&mut cells[base_cell], codecs);

    if !record {
        return;
    }
    let enc_base = (enc_before + enc_after) / 2.0;
    let dec_base = (dec_before + dec_after) / 2.0;
    for (i, e, d) in readings {
        let cell = &mut cells[i];
        cell.encode_rounds.push(e);
        cell.decode_rounds.push(d);
        cell.encode_rel_rounds.push(e / enc_base);
        cell.decode_rel_rounds.push(d / dec_base);
    }
    let base = &mut cells[base_cell];
    base.encode_rounds.push(enc_base);
    base.decode_rounds.push(dec_base);
    // The baseline against itself. Recorded rather than special-cased, so that
    // every row in the report is the same kind of number.
    base.encode_rel_rounds.push(1.0);
    base.decode_rel_rounds.push(1.0);
}

/// Times one cell's encode and decode: the whole pipeline in each direction.
fn time_cell(cell: &mut Cell, codecs: &[codecs::Codec]) -> (f64, f64) {
    let codec = &codecs[cell.codec];
    // A native codec is handed the stage's level; every other one was handed
    // bytes the stage had already compressed, and takes none. Which of the two
    // it is was decided when the cell was built, so what is left inside the
    // clock is a branch on a `bool` that predicts perfectly, against a call
    // that allocates a string.
    let native = cell.native.then(|| {
        let n = codec
            .native
            .as_ref()
            .expect("native cell has a native path");
        let level = match cell.stage {
            Stage::None => None,
            Stage::Zstd(l) => Some(l),
        };
        (n, level)
    });
    let decode = match native {
        Some((n, _)) => n.decode,
        None => codec.decode,
    };

    let mut scratch = String::with_capacity(cell.escaped.len());
    let enc = time_once(|| {
        let encoded = match native {
            Some((n, level)) => (n.encode)(&cell.input, level),
            None => (codec.encode)(&cell.input),
        };
        scratch.clear();
        json::escape_into(&encoded, &mut scratch);
        std::hint::black_box(scratch.len());
    });
    let dec = time_once(|| {
        let unescaped = json::unescape(&cell.escaped).expect("unescape");
        let bytes = decode(&unescaped).expect("decode");
        std::hint::black_box(bytes.len());
    });
    (enc, dec)
}

/// The zstd contexts and buffers the compression stage is measured with, kept
/// for the whole run rather than built per measurement.
///
/// `zstd::bulk::compress` and `zstd::bulk::decompress` are one-line
/// conveniences that build a `ZSTD_CCtx` or `ZSTD_DCtx`, use it once and drop
/// it. On a field-sized payload that setup *is* the measurement -- 14 us to
/// decompress a 92-byte record that takes 27 ns once the context is kept -- and
/// 55 of the 88 samples here are under 200 bytes. Measuring it that way makes
/// the stage look enormously expensive, and since the site adds the stage to
/// both sides of every ratio, an enormous constant drags every codec's figure
/// towards 1.0 and hides what the codecs actually differ by.
///
/// A caller who compresses more than once reuses a context; this crate's own
/// README says the point is to measure what a codec does, not how well someone
/// optimised the thing around it. So: one decompressor for the whole run, since
/// a decompression context carries no level and there is nothing to key it by,
/// and one compressor per level, since a compression context is built around
/// its level's window and changing that is what building a new one is for.
/// Output buffers are kept for the same reason -- an allocation per call is not
/// a property of zstd either.
struct Stages {
    compressors: BTreeMap<i32, zstd::bulk::Compressor<'static>>,
    decompressor: zstd::bulk::Decompressor<'static>,
    into: Vec<u8>,
}

impl Stages {
    fn new(levels: impl Iterator<Item = i32>, widest: usize) -> Stages {
        Stages {
            compressors: levels
                .map(|l| (l, zstd::bulk::Compressor::new(l).expect("zstd compressor")))
                .collect(),
            decompressor: zstd::bulk::Decompressor::new().expect("zstd decompressor"),
            into: Vec::with_capacity(zstd::zstd_safe::compress_bound(widest).max(widest)),
        }
    }
}

/// The compression stages, timed once per (sample, level) rather than once per
/// codec: it is the same bytes doing the same work whichever codec follows, and
/// measuring it six times would only add six times the noise.
fn measure_compression(
    compression: &mut [CompressionCell],
    sources: &[Vec<u8>],
    stages: &mut Stages,
    record: bool,
) {
    for cell in compression.iter_mut() {
        let Stage::Zstd(level) = cell.stage else {
            // Not compressing costs nothing, and is recorded as costing
            // nothing, so a reader adding the two columns together gets the
            // right answer without a special case.
            if record {
                cell.compress_rounds.push(0.0);
                cell.decompress_rounds.push(0.0);
            }
            continue;
        };
        let source = &sources[cell.sample];
        let compressed = &cell.output;
        let c = stages
            .compressors
            .get_mut(&level)
            .expect("a compressor per level");
        let into = &mut stages.into;
        let enc = time_once(|| {
            into.clear();
            std::hint::black_box(c.compress_to_buffer(source, into).expect("zstd"));
        });
        // The output size comes from the frame a real caller would have: zstd
        // writes the content size into an ordinary frame by default, so this is
        // a hint the pipeline genuinely carries and not one the harness
        // invented. `decompress_to_buffer` bounds itself by the capacity.
        let d = &mut stages.decompressor;
        let dec = time_once(|| {
            into.clear();
            into.reserve(source.len().max(1));
            std::hint::black_box(d.decompress_to_buffer(compressed, into).expect("zstd"));
        });
        if record {
            cell.compress_rounds.push(enc);
            cell.decompress_rounds.push(dec);
        }
    }
}

// --- odds and ends ------------------------------------------------------

struct Args {
    manifest: PathBuf,
    out: PathBuf,
    /// Where to also write the per-sample rows, if anywhere. Off by default:
    /// nothing reads them, and they are twenty times the size of what does.
    raw: Option<PathBuf>,
    profiles: PathBuf,
    rounds: usize,
    groups: Vec<String>,
}

impl Args {
    fn parse() -> Args {
        let mut args = Args {
            manifest: "../corpus/data/manifest.json".into(),
            out: "../results.json".into(),
            raw: None,
            profiles: "../profiles/profiles.toml".into(),
            rounds: 5,
            groups: Vec::new(),
        };
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < argv.len() {
            // Every option here takes exactly one value, so the flag and its
            // argument are read together rather than through a closure that
            // would have to borrow the cursor.
            let flag = argv[i].clone();
            let mut value = || {
                i += 1;
                argv.get(i).cloned().unwrap_or_else(|| {
                    eprintln!("{flag}: missing value");
                    std::process::exit(2);
                })
            };
            match flag.as_str() {
                "--manifest" => args.manifest = value().into(),
                "--out" => args.out = value().into(),
                "--raw" => args.raw = Some(value().into()),
                "--profiles" => args.profiles = value().into(),
                "--rounds" => args.rounds = value().parse().expect("--rounds takes a number"),
                "--groups" => {
                    args.groups = value().split(',').map(|s| s.trim().to_string()).collect()
                }
                "--help" | "-h" => {
                    eprintln!(
                        "usage: b2t-runner [--manifest PATH] [--out PATH] [--profiles PATH]\n\
                         \x20                 [--raw PATH] [--rounds N] [--groups a,b,c]"
                    );
                    std::process::exit(0);
                }
                other => {
                    eprintln!("unknown argument: {other}");
                    std::process::exit(2);
                }
            }
            i += 1;
        }
        assert!(args.rounds >= 1, "--rounds must be at least 1");
        assert!(
            args.raw.as_deref() != Some(args.out.as_path()),
            "--raw and --out name the same file; one of them would be the other"
        );
        args
    }
}

/// The profiles are carried into results.json rather than read by the site
/// separately, so that a downloaded results file is self-contained: the
/// weightings a number was presented under travel with the number.
fn read_profiles(path: &Path) -> serde_json::Value {
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!(
            "note: no profiles at {} -- the site will offer none",
            path.display()
        );
        return serde_json::json!({});
    };
    match toml_lite::parse(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("warning: {}: {e}", path.display());
            serde_json::json!({})
        }
    }
}

/// Writes one file, whole or not at all: a run that took minutes to produce
/// these numbers should not be able to leave half of them somewhere.
fn write(path: &Path, value: &impl Serialize) {
    let text = serde_json::to_string(value).expect("results serialise");
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).unwrap_or_else(|e| panic!("{}: {e}", tmp.display()));
    std::fs::rename(&tmp, path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn run(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn cpu_model() -> String {
    std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".into())
}

/// Which revision of each codec produced these numbers. Without it a results
/// file says what happened but not to what.
fn codec_revisions() -> BTreeMap<String, String> {
    ["base91z", "base85n", "base94max", "base65t"]
        .iter()
        .map(|name| {
            let dir = format!("../codecs/{name}");
            let rev = std::process::Command::new("git")
                .args(["-C", &dir, "rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".into());
            (name.to_string(), rev)
        })
        .collect()
}

fn iso_now() -> String {
    // Seconds since the epoch, formatted as a UTC timestamp. Written by hand
    // rather than by pulling in a date library for one line.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil-from-days, Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Just enough TOML for the profile file: tables, nested tables, strings and
/// numbers. A dependency for this would be more code to audit than the parser.
mod toml_lite {
    use serde_json::{Map, Value};

    pub fn parse(text: &str) -> Result<Value, String> {
        let mut root = Map::new();
        let mut path: Vec<String> = Vec::new();
        for (lineno, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let at = || format!("line {}", lineno + 1);
            if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                path = header.split('.').map(|p| unquote(p.trim())).collect();
                table_at(&mut root, &path);
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("{}: expected key = value", at()))?;
            let table = table_at(&mut root, &path);
            table.insert(unquote(key.trim()), scalar(value.trim(), &at)?);
        }
        Ok(Value::Object(root))
    }

    fn strip_comment(line: &str) -> &str {
        let mut in_string = false;
        for (i, c) in line.char_indices() {
            match c {
                '"' => in_string = !in_string,
                '#' if !in_string => return &line[..i],
                _ => {}
            }
        }
        line
    }

    fn unquote(s: &str) -> String {
        s.trim_matches('"').to_string()
    }

    fn table_at<'a>(
        root: &'a mut Map<String, Value>,
        path: &[String],
    ) -> &'a mut Map<String, Value> {
        let mut cur = root;
        for part in path {
            cur = cur
                .entry(part.clone())
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
                .expect("profile path is a table");
        }
        cur
    }

    fn scalar(s: &str, at: &dyn Fn() -> String) -> Result<Value, String> {
        if s.starts_with('"') {
            return Ok(Value::String(unquote(s)));
        }
        if s == "true" || s == "false" {
            return Ok(Value::Bool(s == "true"));
        }
        s.parse::<f64>()
            .map(Value::from)
            .map_err(|_| format!("{}: cannot read {s:?}", at()))
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn reads_a_profile_file() {
            let v = super::parse(
                "[profile.binary]\nlabel = \"Binary data\"  # a comment\n\
                 [profile.binary.weights]\nbinary = 3\nimage = 1.5\n",
            )
            .unwrap();
            assert_eq!(v["profile"]["binary"]["label"], "Binary data");
            assert_eq!(v["profile"]["binary"]["weights"]["binary"], 3.0);
            assert_eq!(v["profile"]["binary"]["weights"]["image"], 1.5);
        }
    }
}
