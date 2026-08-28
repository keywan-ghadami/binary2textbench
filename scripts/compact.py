#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""results.json, rewritten at the grain it is read.

    python3 scripts/compact.py results.json -o site/results.json
    python3 scripts/compact.py results.json --check

The runner writes one row per (sample, codec, stage): eighty-eight samples over
six codecs and five stages is three thousand and eighty rows, and 1.5 MB.
Neither reader shows one of them. Both sum straight to a cell keyed by codec,
stage, group and category, which is also the level the page's weight sliders act
on. So that is what this writes.

Four things make it small. Three of them lose nothing:

**A value is not stored along a dimension it does not vary over.** `input` is
identical for every codec and every stage, so it sits in `corpus`, once per
(group, category). `coded_in`, `comp_out` and the compression cost do not vary
by codec, so they sit in `stage_sizes`. The absolute encode and decode
nanoseconds are only ever read summed over all categories, so they sit in
`codec_times`. That one change is most of the difference.

**Ratios are stored as ratios**, not as ratio x bytes. The reader divides by the
bytes either way; the stored number is a third as long.

**`json_bytes` is `raw + esc`**, checked over every row before the column is
dropped, and `expand` puts it back.

The fourth is a real loss and is declared: ratios keep REL decimals, and the
five hundred and something override cells are not written -- see PRUNE_OVERRIDE.

**`group` is a dimension.** It has to be: `balanced` weights `binary_short`
separately from `binary`, and with only a category to key on there was nothing
for that weight to attach to. Both readers now resolve a weight by trying
`{cat}_{group}` before `{cat}`.

**No display copy.** `codecs[].note`, `profiles[].label` and `profiles[].note`
are words for a person to read; they live beside the thing that renders them.
`origin` and `path` go with the per-sample rows they described.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

FORMAT = 4

# Decimals kept on a stored ratio. The page prints three and the comment two,
# so this is the display precision with no margin above it -- chosen knowingly
# for the size, not because the digits were meaningless. Raise it to 4 and the
# file grows by about six kilobytes.
REL = 3

# Decimals kept on an absolute nanosecond sum. One, not zero: this corpus has
# samples of eleven bytes whose timings sit at 31.0048 ns, and rounding sums of
# those to whole nanoseconds lost a tenth of a per cent.
NS = 1

# A codec that carries its own compression is measured twice per stage: once as
# it ships, once with its own compression forced off so the stage's compressor
# sits in front of it like everybody else. Both readers show the first and
# neither reads the second, so the second is not written. This is the one place
# a measurement is dropped rather than restructured; the runner's full output
# still has it.
PRUNE_OVERRIDE = True

RATIOS = ("enc_raw", "enc_q", "dec_raw", "dec_q",
          "enc_tot", "enc_tot_q", "dec_tot", "dec_tot_q")
# Only stored where the codec decides its own compression. Elsewhere they equal
# their _raw counterpart and are left off the end of the row.
SPARSE = ("enc_cod", "dec_cod")
CELL_COLS = ["codec", "stage", "grp", "cat", "native", "raw", "esc"] + \
            list(RATIOS) + list(SPARSE)


def compact(d: dict) -> dict:
    """v1 -> v4."""
    if d.get("v") == FORMAT:
        return d

    S = {s["name"]: s for s in d["corpus"]["samples"]}
    groups = list(d["corpus"]["groups"])
    cats: list[str] = []
    for s in d["corpus"]["samples"]:
        if s["category"] not in cats:
            cats.append(s["category"])
    stages = list(d["stages"])
    codecs = [c["name"] for c in d["codecs"]]
    gi = {g: i for i, g in enumerate(groups)}
    ci = {c: i for i, c in enumerate(cats)}
    si = {s: i for i, s in enumerate(stages)}
    ki = {c: i for i, c in enumerate(codecs)}

    bad = [m for m in d["measurements"]
           if m["json_bytes"] != m["encoded_bytes"] + m["escapes"]]
    if bad:
        sys.exit(f"json_bytes is not raw+esc on {len(bad)} row(s); the column "
                 f"cannot be dropped. Add it back to CELL_COLS.")

    comp = {(c["sample"], c["stage"]): c for c in d["compression"]}
    baseline = d["meta"]["baseline"]
    base = {(m["sample"], m["stage"]): m for m in d["measurements"]
            if m["codec"] == baseline and not m["native"]}
    missing = {(m["sample"], m["stage"]) for m in d["measurements"]} - set(base)
    if missing:
        sys.exit(f"no {baseline!r} row for {len(missing)} (sample, stage) pair(s); "
                 f"every ratio is against it, so there is nothing to divide by.")

    native_codecs = {m["codec"] for m in d["measurements"] if m["native"]}

    def keep(m: dict) -> bool:
        if not PRUNE_OVERRIDE:
            return True
        return m["native"] if m["codec"] in native_codecs else not m["native"]

    corpus: dict = defaultdict(lambda: [0, 0])
    for s in d["corpus"]["samples"]:
        a = corpus[(s["group"], s["category"])]
        a[0] += 1
        a[1] += s["bytes"]

    stage_sizes: dict = defaultdict(lambda: [0, 0, 0.0, 0.0])
    for c in d["compression"]:
        s = S[c["sample"]]
        a = stage_sizes[(c["stage"], s["group"], s["category"])]
        a[0] += c["input_bytes"]
        a[1] += c["output_bytes"]
        a[2] += c["compress"]["ns"]
        a[3] += c["decompress"]["ns"]

    times: dict = defaultdict(lambda: [0.0, 0.0])
    cells: dict = defaultdict(lambda: defaultdict(float))
    for m in d["measurements"]:
        if not keep(m):
            continue
        s = S[m["sample"]]
        nat = int(m["native"])
        t = times[(m["codec"], m["stage"], nat)]
        t[0] += m["encode"]["ns"]
        t[1] += m["decode"]["ns"]
        e = cells[(m["codec"], m["stage"], s["group"], s["category"], nat)]
        n = m["input_bytes"]
        e["input"] += n
        e["raw"] += m["encoded_bytes"]
        e["esc"] += m["escapes"]
        b, cp = base[(m["sample"], m["stage"])], comp[(m["sample"], m["stage"])]
        # The three views the page's controls offer, formed per sample because
        # the page forms them per sample: dividing sums instead would be a
        # different number. Only the products are accumulated.
        for dn, tk, ck in (("enc", "encode", "compress"),
                           ("dec", "decode", "decompress")):
            own, oth = m[tk]["ns"], b[tk]["ns"]
            rel, cns = m[f"{tk}_rel"], cp[ck]["ns"]
            e[f"{dn}_raw"] += rel["ns"] * n
            e[f"{dn}_q"] += rel["iqr_ns"] * n
            cod = (own / oth if oth else 0.0) if m["native"] else rel["ns"]
            e[f"{dn}_cod"] += cod * n
            mine = own + (0.0 if m["native"] else cns)
            theirs = oth + cns
            e[f"{dn}_tot"] += (mine / theirs if theirs else 0.0) * n
            e[f"{dn}_tot_q"] += rel["iqr_ns"] * (own / mine if mine else 1.0) * n

    rows = []
    for (codec, stage, g, c, nat), e in sorted(cells.items()):
        n = e["input"] or 1
        r = [ki[codec], si[stage], gi[g], ci[c], nat, int(e["raw"]), int(e["esc"])]
        r += [round(e[col] / n, REL) for col in RATIOS]
        r += [round(e[col] / n, REL) if nat else None for col in SPARSE]
        while r and r[-1] is None:
            r.pop()
        rows.append(r)

    return {
        "v": FORMAT,
        "meta": d["meta"],
        "groups": groups,
        "categories": cats,
        "stages": stages,
        "codecs": codecs,
        "corpus_cols": ["grp", "cat", "samples", "bytes"],
        "corpus": [[gi[g], ci[c], v[0], v[1]] for (g, c), v in sorted(corpus.items())],
        "stage_cols": ["stage", "grp", "cat", "coded_in", "comp_out",
                       "comp_ns", "decomp_ns"],
        "stage_sizes": [[si[st], gi[g], ci[c], a[0], a[1],
                         round(a[2], NS), round(a[3], NS)]
                        for (st, g, c), a in sorted(stage_sizes.items())],
        "time_cols": ["codec", "stage", "native", "enc_ns", "dec_ns"],
        "codec_times": [[ki[k], si[st], nat, round(a[0], NS), round(a[1], NS)]
                        for (k, st, nat), a in sorted(times.items())],
        "cell_cols": CELL_COLS,
        "cells": rows,
        "profiles": {n: {k: w for k, w in (p.get("weights") or {}).items() if w}
                     for n, p in (d["profiles"]["profile"]).items()},
    }


# --- reading side, shared by report.py ----------------------------------

def _rows(d: dict, cols_key: str, rows_key: str):
    cols = d[cols_key]
    for r in d[rows_key]:
        yield {c: (r[i] if i < len(r) else None) for i, c in enumerate(cols)}


def shape(d: dict) -> dict:
    """The whole file as named dicts, with indexes resolved to strings.

    The only place in Python that knows the row layout. `pair` is the
    (group, category) key both readers weight by.
    """
    if d.get("v") != FORMAT:
        sys.exit("this results.json is in the runner's per-sample shape. "
                 "Convert it first: python3 scripts/compact.py <file> -o <file>")

    g, c, st, kd = d["groups"], d["categories"], d["stages"], d["codecs"]

    corpus = {}
    for r in _rows(d, "corpus_cols", "corpus"):
        corpus[(g[r["grp"]], c[r["cat"]])] = {"samples": r["samples"],
                                              "bytes": r["bytes"]}
    stage_sizes = {}
    for r in _rows(d, "stage_cols", "stage_sizes"):
        stage_sizes[(st[r["stage"]], g[r["grp"]], c[r["cat"]])] = r
    times = {}
    for r in _rows(d, "time_cols", "codec_times"):
        times[(kd[r["codec"]], st[r["stage"]], bool(r["native"]))] = r

    cells = []
    for r in _rows(d, "cell_cols", "cells"):
        grp, cat = g[r["grp"]], c[r["cat"]]
        e = dict(r)
        e.update(codec=kd[r["codec"]], stage=st[r["stage"]], grp=grp, cat=cat,
                 native=bool(r["native"]), pair=(grp, cat),
                 input=corpus[(grp, cat)]["bytes"],
                 json=r["raw"] + r["esc"])
        # Absent means "same as measured"; only a self-compressing codec differs.
        for a, b in (("enc_cod", "enc_raw"), ("dec_cod", "dec_raw")):
            if e.get(a) is None:
                e[a] = e[b]
        cells.append(e)

    return {"meta": d["meta"], "groups": g, "categories": c, "stages": st,
            "codecs": kd, "corpus": corpus, "stage_sizes": stage_sizes,
            "times": times, "cells": cells,
            "profiles": d.get("profiles", {})}


def load(path: Path) -> dict:
    return shape(json.loads(path.read_text()))


def pair_label(pairs) -> dict:
    """A (group, category) pair as a label: the category alone where it is
    unambiguous, `group/category` where the same category is in two groups."""
    seen: dict = defaultdict(set)
    for grp, cat in pairs:
        seen[cat].add(grp)
    return {(grp, cat): (cat if len(seen[cat]) == 1 else f"{grp}/{cat}")
            for grp, cat in pairs}


def weight_of(profile: dict, grp: str, cat: str) -> float:
    """`{cat}_{group}` first, then `{cat}`. That is what lets `balanced` weight
    `binary_short` apart from `binary`."""
    for k in (f"{cat}_{grp}", cat):
        if k in profile:
            return float(profile[k])
    return 0.0


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path", type=Path)
    ap.add_argument("-o", "--out", type=Path, default=None)
    ap.add_argument("--check", action="store_true",
                    help="recompute every cell straight from the source rows "
                         "and compare")
    ap.add_argument("--indent", type=int, default=None)
    args = ap.parse_args()

    src = json.loads(args.path.read_text())

    if args.check:
        import verify
        bad = verify.check(src, compact(src))
        if bad:
            print(f"{len(bad)} problem(s):", file=sys.stderr)
            for b in bad[:20]:
                print("  " + b, file=sys.stderr)
            sys.exit(1)
        print("clean")
        return

    out = compact(src)
    text = json.dumps(out, indent=args.indent,
                      separators=(",", ":") if args.indent is None else None)
    if args.out:
        args.out.write_text(text + "\n")
        b, a = args.path.stat().st_size, args.out.stat().st_size
        print(f"{b:,} -> {a:,} bytes ({a / b * 100:.1f} %)", file=sys.stderr)
    else:
        sys.stdout.write(text + "\n")


if __name__ == "__main__":
    main()
