# binary2textbench

One corpus, one process, one set of numbers for the binary-to-text encodings
that keep getting compared to each other by hand.

Six encodings are measured: **Base64** as the baseline, **classic basE91** and
**Ascii85** as the established alternatives, and **Base91z**, **Base85N** and
**Base94Max**. They are built from source and run in one process, so what is
compared is six encodings and not six languages.

## What is measured

The whole path a payload takes, because that is what a caller pays for:

```
encode:  bytes → [zstd] → codec → JSON escaping → the string on the wire
decode:  the string → JSON unescaping → codec → [zstd] → bytes
```

**The JSON step is inside the clock.** An encoding whose alphabet contains `"`
or `\` pays for it twice — once in the characters it adds and once in the work
of adding them — and leaving the second cost out would flatter it. There is one
escaper and every codec goes through it, so what differs between codecs is how
much work they hand it, not how well they optimised it.

Two sizes are reported. **The size inside a JSON string is the primary one**,
because that is where these strings actually end up; the raw encoded length is
reported too, and the gap between them is the escaping.

Compression is zstd at `-5`, `1`, `9` and `19`, plus no compression at all as
its own row — for a short payload or an incompressible one, none is what a
caller should actually do, and the report should be able to say so.

**The compression setting applies to every row, including the ones that carry
their own.** Base91z decides per payload whether to compress; picking `zstd 9`
tells it to decide at level 9, exactly as it tells the other five codecs to run
level 9 in front of them. Picking `none` puts no compressor in front of
anybody, and Base91z then uses its own default — which is what a caller who
passes no level gets from it. So there is one row per codec and the dropdown
means one thing.

Forcing Base91z's decision off and running it behind an external compressor
like the others is measured too, and is in `results.json` marked
`native: false`. It is not a row on the page: it is an override for a caller
who already knows their data is incompressible, and putting it beside the real
thing only invites the question of which one to read.

**The compression stage is measured with a context a caller would reuse.**
`zstd::bulk::compress` and `zstd::bulk::decompress` build a `ZSTD_CCtx` or
`ZSTD_DCtx`, use it once and drop it, and on a field-sized payload that setup
*is* the measurement — 14 µs to decompress a 92-byte record that takes 27 ns
with the context kept, and 55 of the 88 samples are under 200 bytes. Since the
site adds the stage cost to both sides of every ratio, an enormous constant
drags every codec's figure towards 1.0 and hides what the codecs differ by. So
the runner keeps one decompressor for the whole run and one compressor per
level, for the same reason there is one JSON escaper: what should differ
between codecs is how much work they hand the thing around them, not how well
someone optimised it.

## Numbers from a noisy machine

This runs on a shared cloud runner. A neighbour, a frequency change or a
scheduler decision moves a measurement by tens of percent, and none of that is
a property of an encoding. Three things are done about it:

**Every speed figure is divided by a Base64 figure measured beside it.** Not at
the end of the run — that makes things *worse*, because two independently
jittery numbers make a more jittery quotient. Each group of measurements (one
sample, one compression stage) is timed as `base64, every other codec, base64
again`, and the denominator is the mean of the two Base64 readings. That
brackets each codec's measurement in time, so a drifting machine drifts through
numerator and denominator alike.

Measured on this corpus, across three independent runs:

| | median | p90 | worst |
|---|---|---|---|
| absolute ns/op | 3.73 % | 9.15 % | 29.39 % |
| ratio, formed after the run | 3.25 % | 8.49 % | 23.64 % |
| **ratio, bracketed per round** | **2.46 %** | **6.39 %** | **21.01 %** |

**The median across rounds is reported, with the interquartile range beside
it.** A single stalled round moves a median by nothing and a mean by a lot, and
the IQR is what says whether a difference between two codecs means anything.

**Sizes are exact.** They are deterministic and identical across runs, so any
size difference is a real difference and not noise. That is why a pull request
report leads with them.

## The corpus

88 samples in three groups, listed in `corpus/data/manifest.json` with a
SHA-256 each.

- **core** — thirteen real files, one per input class: binary containers, an
  uncompressed tar, JSON pretty and minified, JavaScript, CSS and Python
  source, the CommonMark specification, a changelog, a JPEG and a PNG. Nothing
  is vendored: every file is pulled from a pinned archive and checked against a
  recorded hash, so a rerun either reproduces the same bytes or fails loudly.
- **short** — 55 field-level samples under 200 bytes: identifiers, digests,
  tokens, one record of JSON. Most encoded payloads in a real system are this
  size, and it is where fixed overhead decides the answer.
- **synthetic** — 20 generated samples that vary one property at a time, which
  is what the weighting profiles are built on: text with 0 %, 0.1 %, 1 % and
  10 % of bytes that make it untransportable as text, at three lengths each; a
  mixed payload; and the incompressible cases — session identifiers,
  ciphertext, already-compressed blobs, and pure noise.
- **silesia** — the 202 MiB corpus compression work has been reported against
  since 2003. Off by default; it takes minutes rather than seconds.

`corpus/synthetic.py` documents what each class is for and what "problematic
byte" means precisely.

## Weighting

A single overall score is a claim about whose data is being encoded, and there
is no answer to that which is right for everyone. So the report does not pick
one. `profiles/profiles.toml` holds six starting points — balanced, a few
problematic characters, many problematic characters, mixed content, binary
data, compressed or random data — and the site lets a reader move every weight
afterwards, because the reader knows their traffic and this file does not.

Adding a profile needs no code change.

## Running it

```sh
scripts/link-codecs.sh                  # or pass three paths explicitly
python3 corpus/manifest.py --groups=core,short,synthetic
cd runner-rust && cargo run --release -- --groups core,short,synthetic
```

That writes `results.json` at the repository root; open `site/index.html`
beside it. Both are what CI uploads as an artifact.

`results.json` holds sums per (codec, stage, group, category) -- the level both
the page and the report read, and the level the weight sliders act on. The
per-sample rows they are formed from are twenty times the size and nothing
shows one of them, so they are written only where they are asked for:
`--raw per-sample.json`. `runner-rust/src/compact.rs` says what each column is
and what the shape costs.

`cargo test` checks the codecs against known vectors and against `base64(1)`,
Python's `base64.a85encode` and Python's `json.dumps` — the baseline and the
escaper decide every other number, so being wrong there would be invisible.

## The page

Above the size and speed charts, the page ranks the codecs on both figures at
once: one row per codec, the size bar and the speed bar beside each other, and
a slider for how much of the ranking is size rather than speed. Each bar is
that codec against the best figure in its own column, which is what puts a per
cent and a multiple on one scale; the score is those two shares combined as a
weighted geometric mean, so halving the size and halving the time count as the
same size of win. Two scores whose gap is inside the measured spread are marked
tied rather than ordered — the rule the pull-request comment already applies to
whether a run changed anything.

`site/` is deployed to <https://bench.ghadami.de> as a Cloudflare Worker with
static assets — `wrangler.jsonc` is the whole configuration, and there is no
build step to set:

| setting | value |
|---|---|
| Build command | *(empty)* |
| Deploy command | `npx wrangler deploy` |

There cannot be a build step. The page needs `results.json` beside it, and that
file is not built but measured — by a run that needs a Rust toolchain, three
codec checkouts and a couple of minutes of quiet CPU. A build container has
none of those, and numbers taken in one would be worthless. So the scheduled
benchmark commits `site/results.json` and the deploy serves what is committed.

On Cloudflare Pages instead, the equivalent is an empty build command with
`site` as the output directory.

## In CI

`action.yml` is a composite action each codec repository calls, so the CI logic
lives here once instead of in four places. A pull request in any of the codec
repositories is measured against its own base branch and the deltas are written
to the job summary; the central workflow runs weekly and on demand against any
combination of refs. Results are artifacts only — the latest state, no history
in the repository.
