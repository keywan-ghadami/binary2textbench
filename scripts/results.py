# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Reads results.json, in the shape the runner writes it.

    from results import load
    r = load(Path("results.json"))

The file holds sums per (codec, stage, group, category) rather than the
per-sample rows they were formed from, because that is the level both readers
show and the level the page's weight sliders act on. `runner-rust/src/compact.rs`
does the summing and says what each column is and what it costs; this is the
other half of it, and the only place in Python that knows the row layout.

Columns are read through the `*_cols` lists rather than by position, so a file
with an extra column at the end of a row still opens. A file from a runner old
enough to write per-sample rows does not: it is refused by version, with the
one thing that fixes it, because converting it here would mean keeping a second
implementation of the summing alive to serve files nobody has any more.
"""

from __future__ import annotations

import json
from collections import defaultdict
from pathlib import Path

FORMAT = 4


def _rows(d: dict, cols_key: str, rows_key: str):
    cols = d[cols_key]
    for r in d[rows_key]:
        yield {c: (r[i] if i < len(r) else None) for i, c in enumerate(cols)}


def shape(d: dict) -> dict:
    """The whole file as named dicts, with indexes resolved to strings.

    `pair` is the (group, category) key both readers weight by.
    """
    if d.get("v") != FORMAT:
        raise SystemExit(
            f"results.json is v{d.get('v')}, not v{FORMAT}: it was written by a "
            f"runner that predates this format. Measure again -- "
            f"cd runner-rust && cargo run --release -- --out ../results.json")

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
