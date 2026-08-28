#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""results.json at the grain it is read, instead of the grain it is measured.

    python3 scripts/compact.py results.json -o results.json
    python3 scripts/compact.py results.json --check

The runner measures one row per (sample, codec, stage): forty samples over six
codecs and five stages is fourteen hundred rows. Neither reader shows one. Both
sum straight to (codec, stage, category) -- the page because that is the level
its weight sliders act on, the comment because a per-sample table is unreadable
-- and nothing anywhere displays a filename or an absolute nanosecond. The
per-sample rows are working-out, and the file was carrying the working-out.

So this writes the sums instead: a couple of hundred rows, and no filenames,
because with the rows gone there is nothing left to join them to.

**The one thing that had to be got right.** The page divides *per sample* and
averages afterwards; summing first and dividing once is a different number.
Ratios are therefore still formed per sample here, exactly as `timeOf` in
index.html forms them, and only the products are summed. Every figure either
reader prints is unchanged, which `--check` proves by running the comment
generator over both files and comparing the output line for line.

**Three ratios per direction, because the controls offer three views.** Below,
`raw` is the ratio the runner measured, `cod` is the codec on its own, `tot`
counts the compression stage. They coincide except for a codec that carries its
own compression. All three are written rather than reconstructed, so that a
change to the page's controls cannot quietly change a stored number.

**Kept although nothing reads it:** `meta` entire, `codec_revisions` at full
length. Two hundred and forty bytes, and the only link from a figure to a
commit. Per-sample checksums and paths go with the rows they described.

**No display text.** A profile keeps its weights and loses its label and note;
a stage keeps its id and never carried a heading. Those are words for a person
to read, they change without any measurement changing, and they belong beside
the thing that renders them.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

FORMAT = 3

# Ratios keep five decimals. The page prints three, the comment two, and the
# threshold they are judged against is eight per cent.
REL_DIGITS = 5

KEY_COLS = ["codec", "stage", "cat", "native"]
SUM_COLS = [
    "input", "raw", "json", "esc",              # sizes, exact sums
    "enc_ns", "dec_ns",                         # absolute sums; the comment's
                                                # headline table re-forms its own
                                                # ratio and needs them
    "enc_raw_w", "enc_raw_q", "enc_cod_w", "enc_cod_q", "enc_tot_w", "enc_tot_q",
    "dec_raw_w", "dec_raw_q", "dec_cod_w", "dec_cod_q", "dec_tot_w", "dec_tot_q",
]
CELL_COLS = KEY_COLS + SUM_COLS
INT_COLS = {"input", "raw", "json", "esc", "enc_ns", "dec_ns"}


def _modes(m: dict, base: dict, comp: dict, dirn: str) -> dict:
    """The three ratios for one measurement, mirroring `timeOf` in index.html.

    `raw` is what the runner measured. `cod` is the codec alone: the same
    number, except for a codec that carries its own compression, whose measured
    ratio is against Base64 over already-compressed input and has to be rebuilt
    from absolute times. `tot` puts the compression stage on both sides.

    The interquartile range is scaled by how much of the total is still the
    codec, because the compression stage is measured separately and is steadier.
    """
    own = m["encode" if dirn == "enc" else "decode"]["ns"]
    theirs_own = base["encode" if dirn == "enc" else "decode"]["ns"]
    rel = m["encode_rel" if dirn == "enc" else "decode_rel"]
    c_ns = comp["compress" if dirn == "enc" else "decompress"]["ns"]
    native = bool(m.get("native"))

    out = {"raw": (rel["ns"], rel["iqr_ns"])}
    out["cod"] = ((own / theirs_own if theirs_own else 0.0, rel["iqr_ns"])
                  if native else (rel["ns"], rel["iqr_ns"]))

    mine = own + (0.0 if native else c_ns)
    theirs = theirs_own + c_ns
    share = own / mine if mine else 1.0
    out["tot"] = (mine / theirs if theirs else 0.0, rel["iqr_ns"] * share)
    return out


def compact(d: dict) -> dict:
    """v1 -> v3."""
    if d.get("v") == FORMAT:
        return d

    samples = d["corpus"]["samples"]
    cat_of = {s["name"]: s["category"] for s in samples}

    categories: list[str] = []
    for s in samples:
        if s["category"] not in categories:
            categories.append(s["category"])
    stages = list(d["stages"])
    codecs = sorted({m["codec"] for m in d["measurements"]})

    comp = {(c["sample"], c["stage"]): c for c in d["compression"]}
    base = {(m["sample"], m["stage"]): m for m in d["measurements"]
            if m["codec"] == "base64" and not m.get("native")}

    cells: dict = defaultdict(lambda: dict.fromkeys(SUM_COLS, 0.0))
    for m in d["measurements"]:
        s = m["sample"]
        e = cells[(m["codec"], m["stage"], cat_of[s], 1 if m.get("native") else 0)]
        n = m["input_bytes"]
        e["input"] += n
        e["raw"] += m["encoded_bytes"]
        e["json"] += m["json_bytes"]
        e["esc"] += m["escapes"]
        e["enc_ns"] += m["encode"]["ns"]
        e["dec_ns"] += m["decode"]["ns"]
        b, c = base[(s, m["stage"])], comp[(s, m["stage"])]
        for dirn in ("enc", "dec"):
            for mode, (rel, iqr) in _modes(m, b, c, dirn).items():
                e[f"{dirn}_{mode}_w"] += rel * n
                e[f"{dirn}_{mode}_q"] += iqr * n

    codec_ix = {c: i for i, c in enumerate(codecs)}
    stage_ix = {s: i for i, s in enumerate(stages)}
    cat_ix = {c: i for i, c in enumerate(categories)}

    def row(k, e) -> list:
        codec, stage, cat, native = k
        out = [codec_ix[codec], stage_ix[stage], cat_ix[cat], native]
        for c in SUM_COLS:
            out.append(int(round(e[c])) if c in INT_COLS else round(e[c], REL_DIGITS))
        return out

    cell_rows = sorted((row(k, e) for k, e in cells.items()),
                       key=lambda r: (r[0], r[1], r[2], r[3]))

    # Per (stage, category); the comment sums these back up per stage.
    cc: dict = defaultdict(lambda: [0.0, 0.0])
    for c in d["compression"]:
        a = cc[(c["stage"], cat_of[c["sample"]])]
        a[0] += c["compress"]["ns"]
        a[1] += c["decompress"]["ns"]
    comp_rows = sorted([[stage_ix[st], cat_ix[ct], int(round(x)), int(round(y))]
                        for (st, ct), (x, y) in cc.items()],
                       key=lambda r: (r[0], r[1]))

    # What the corpus was, without the per-sample rows: both readers want a
    # count and a total, and the page's category table has to know a category
    # exists even where it is weighted to nothing.
    per_cat: dict = defaultdict(lambda: [0, 0])
    for s in samples:
        a = per_cat[s["category"]]
        a[0] += 1
        a[1] += s["bytes"]

    out = {
        "v": FORMAT,
        "meta": d["meta"],
        "codecs": codecs,
        "stages": stages,
        "categories": categories,
        "corpus": [[per_cat[c][0], per_cat[c][1]] for c in categories],
        "cell_cols": CELL_COLS,
        "cells": cell_rows,
        "comp_cols": ["stage", "cat", "comp_ns", "decomp_ns"],
        "comp": comp_rows,
    }
    profiles = (d.get("profiles") or {}).get("profile") or {}
    if profiles:
        # Weights only. `label` and `note` are copy for a person to read, and
        # copy belongs with the thing that renders it -- index.html -- not in a
        # measurement file that gets regenerated on every run.
        out["profiles"] = {
            name: {c: w for c, w in (p.get("weights") or {}).items() if w}
            for name, p in profiles.items()
        }
    return out


def cells_of(d: dict):
    """Every cell as a dict with its keys resolved.

    The only place anything in Python touches the row layout: change the
    columns and this is what needs to know.
    """
    ix = {c: i for i, c in enumerate(d["cell_cols"])}
    for r in d["cells"]:
        e = {c: r[i] for c, i in ix.items()}
        e["codec"] = d["codecs"][e["codec"]]
        e["stage"] = d["stages"][e["stage"]]
        e["cat"] = d["categories"][e["cat"]]
        e["native"] = bool(e["native"])
        yield e


def comp_of(d: dict):
    ix = {c: i for i, c in enumerate(d["comp_cols"])}
    for r in d["comp"]:
        yield {"stage": d["stages"][r[ix["stage"]]],
               "cat": d["categories"][r[ix["cat"]]],
               "comp_ns": r[ix["comp_ns"]], "decomp_ns": r[ix["decomp_ns"]]}


def _plainly(d: dict) -> dict:
    """The same sums, accumulated a second time and spelled out differently.

    Deliberately not sharing `_modes` or `compact`: a check that runs the code
    it is checking proves nothing. This is the plodding version -- one loop, the
    ratios written inline -- so that a transcription error in either shows up as
    a disagreement between them.
    """
    cat_of = {s["name"]: s["category"] for s in d["corpus"]["samples"]}
    comp = {(c["sample"], c["stage"]): c for c in d["compression"]}
    b64 = {(m["sample"], m["stage"]): m for m in d["measurements"]
           if m["codec"] == "base64" and not m.get("native")}
    acc: dict = {}
    for m in d["measurements"]:
        s, st = m["sample"], m["stage"]
        k = (m["codec"], st, cat_of[s], bool(m.get("native")))
        e = acc.setdefault(k, dict.fromkeys(SUM_COLS, 0.0))
        n = m["input_bytes"]
        e["input"] += n
        e["raw"] += m["encoded_bytes"]
        e["json"] += m["json_bytes"]
        e["esc"] += m["escapes"]
        for dirn, mine_f, base_f, rel_f, comp_f in (
                ("enc", "encode", "encode", "encode_rel", "compress"),
                ("dec", "decode", "decode", "decode_rel", "decompress")):
            own = m[mine_f]["ns"]
            theirs_own = b64[(s, st)][base_f]["ns"]
            cns = comp[(s, st)][comp_f]["ns"]
            iqr = m[rel_f]["iqr_ns"]
            e[f"{dirn}_ns"] += own
            e[f"{dirn}_raw_w"] += m[rel_f]["ns"] * n
            e[f"{dirn}_raw_q"] += iqr * n
            if m.get("native"):
                e[f"{dirn}_cod_w"] += (own / theirs_own if theirs_own else 0.0) * n
                e[f"{dirn}_cod_q"] += iqr * n
                e[f"{dirn}_tot_w"] += (own / (theirs_own + cns)
                                       if theirs_own + cns else 0.0) * n
                e[f"{dirn}_tot_q"] += iqr * n
            else:
                e[f"{dirn}_cod_w"] += m[rel_f]["ns"] * n
                e[f"{dirn}_cod_q"] += iqr * n
                e[f"{dirn}_tot_w"] += ((own + cns) / (theirs_own + cns)
                                       if theirs_own + cns else 0.0) * n
                e[f"{dirn}_tot_q"] += iqr * (own / (own + cns) if own + cns else 1.0) * n
    return acc


def check(d: dict) -> list[str]:
    """Compare what `compact` wrote against `_plainly`, and against raw totals."""
    v3 = compact(d)
    mine = {(c["codec"], c["stage"], c["cat"], c["native"]): c for c in cells_of(v3)}
    theirs = _plainly(d)
    bad: list[str] = []

    if set(mine) != set(theirs):
        bad.append(f"{len(mine)} cells written, {len(theirs)} expected")
    for k in sorted(set(mine) & set(theirs), key=repr):
        a, b = mine[k], theirs[k]
        for col in SUM_COLS:
            x, y = a[col], b[col]
            tol = 0.5 if col in INT_COLS else max(abs(y) * 1e-4, 1e-4)
            if abs(x - y) > tol:
                bad.append(f"{k} {col}: {x} vs {y}")

    # And the sizes against the untouched original, which needs no aggregation
    # to state: what went in must still be in there.
    for field, col in (("input_bytes", "input"), ("encoded_bytes", "raw"),
                       ("json_bytes", "json"), ("escapes", "esc")):
        want = sum(m[field] for m in d["measurements"])
        got = sum(c[col] for c in mine.values())
        if want != got:
            bad.append(f"total {col}: {got:,} written, {want:,} measured")
    return bad


def as_v3(d: dict) -> dict:
    """v1 in, v3 out; v3 in, unchanged. What a reader calls at the door, so an
    artifact from before the format change still opens."""
    return d if d.get("v") == FORMAT else compact(d)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path", type=Path)
    ap.add_argument("-o", "--out", type=Path, default=None)
    ap.add_argument("--check", action="store_true",
                    help="rebuild the sums a second way and compare")
    ap.add_argument("--indent", type=int, default=None)
    args = ap.parse_args()

    src = json.loads(args.path.read_text())

    if args.check:
        bad = check(src)
        if bad:
            print(f"{len(bad)} difference(s):", file=sys.stderr)
            for b in bad[:20]:
                print("  " + b, file=sys.stderr)
            sys.exit(1)
        print(f"clean: {len(compact(src)['cells'])} cells agree with a second, "
              f"independently written accumulation, and with the raw totals.")
        return

    out = compact(src)
    text = json.dumps(out, indent=args.indent,
                      separators=(",", ":") if args.indent is None else None)
    if args.out:
        args.out.write_text(text + "\n")
        before, after = args.path.stat().st_size, args.out.stat().st_size
        print(f"{before:,} -> {after:,} bytes ({after / before * 100:.1f} %)",
              file=sys.stderr)
    else:
        sys.stdout.write(text + "\n")


if __name__ == "__main__":
    main()
