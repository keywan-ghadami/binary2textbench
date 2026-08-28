#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Turns results.json into the Markdown a pull request gets to read.

    python3 scripts/report.py results.json [--baseline base-results.json]

With one file it reports the state of things. With two it reports the change,
which is what a pull request actually wants to know -- and the two halves are
treated differently on purpose:

**Sizes are exact.** They are deterministic and identical across runs, so any
difference is a real difference and is reported as one, in bytes and per cent,
with no threshold.

**Speeds are not**, and they are reported at a coarser grain because of it.
Size moves are broken out per corpus category; speed moves are not, because the
machine will not support it. Measured here across three runs of identical code:

    spread between runs        p50     p90     p99    worst
    per codec/stage/category  4.14%  12.21%  22.72%  26.10%
    per codec/stage           2.03%   4.43%   5.31%   5.31%

A per-category speed table would have been reporting the runner, not the code:
at that grain a quarter of the value is noise. Aggregated over the whole corpus
it is five per cent, and a threshold above that is worth printing. So the speed
section works per (codec, stage) and calls nothing a change until it clears both
the spread the run measured for itself and SPEED_FLOOR below.

Calling a 2 % move a regression on a machine that moves 5 % by itself would
train everyone to ignore the report, which is the one failure mode that matters
here. Nothing fails a build either way: it reports; a human decides.

**The comment is kept short on purpose.** It is read in a scroll past, not
studied, and every line that says the same thing on every run is a reason to
stop reading the ones that do not. So this prints what moved and what it moved
by, and little else: provenance goes in an HTML comment -- in the source, out of
the rendering -- tables carry the delta rather than the delta beside the
absolute it is a delta of, and a column that is identical on both sides is
dropped rather than printed as a column of zeroes. The standing figures, every
stage and every category and weightable, are on the page, which is where a
reader who came to study them was going anyway.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

# The file on disk holds sums per (codec, stage, category) -- see compact.py,
# which explains why the per-sample rows are not in it. `as_v3` converts an old
# per-sample file on the fly, so an artifact from before the change still
# reports and prints the same thing.
from compact import as_v3, cells_of, comp_of

# A speed change has to clear both the measured spread and this floor before it
# is called anything. 8 % sits above the 5.31 % worst case measured between runs
# of identical code (see the module docstring) with enough headroom that a
# quieter or busier runner does not start manufacturing regressions. Re-measure
# it if the corpus or the runner changes: it is calibration, not a preference.
#
# The page applies the same floor when it decides whether two bars are tied. If
# this moves, move it there too, or -- better -- have the runner write it into
# meta so there is one number instead of two.
SPEED_FLOOR = 0.08

# How many rows each table prints before the rest becomes a count. Both are
# sorted largest-move-first, so what is cut is what moved least; the artifact
# has every row for anyone who wants them. Speed is capped lower because it is
# already the coarser of the two: at six codecs and five stages an everything-
# moved change would otherwise print thirty rows saying the same thing.
SIZE_ROWS = 20
SPEED_ROWS = 12


def load(path: Path) -> dict:
    return as_v3(json.loads(path.read_text()))


def reported(results: dict):
    """The cells the report shows: one per (codec, stage, category).

    A codec that carries its own compression has two cells at every stage --
    one as it ships, deciding for itself at that level, and one with the
    decision forced off and the stage's compressor in front of it like everybody
    else. The first is the row, because it is what a caller of that codec gets.
    Taking both, which is what iterating the cells straight does, would report a
    codec twice its real size.
    """
    cells = list(cells_of(results))
    has_native = {c["codec"] for c in cells if c["native"]}
    for c in cells:
        if c["native"] == (c["codec"] in has_native):
            yield c


def by_category(results: dict) -> dict:
    """Aggregate to (codec, stage, category): the level a reader can act on.

    Per-sample rows are too many to read and per-codec totals hide which kind
    of input moved, which is usually the whole story.
    """
    out: dict = defaultdict(lambda: {
        "input": 0, "json": 0, "raw": 0, "escapes": 0,
        "enc": 0.0, "iqr": 0.0, "dec": 0.0,
    })
    for c in reported(results):
        e = out[(c["codec"], c["stage"], c["cat"])]
        e["input"] += c["input"]
        e["json"] += c["json"]
        e["raw"] += c["raw"]
        e["escapes"] += c["esc"]
        # Already weighted by bytes where it was summed; dividing by the bytes
        # below is what turns it back into a ratio.
        e["enc"] += c["enc_raw_w"]
        e["dec"] += c["dec_raw_w"]
        e["iqr"] += c["enc_raw_q"]
    for e in out.values():
        if e["input"]:
            e["enc"] /= e["input"]
            e["dec"] /= e["input"]
            e["iqr"] /= e["input"]
    return out


def totals(results: dict) -> dict:
    """Whole-corpus totals per (codec, stage).

    `enc` and `iqr` are the byte-weighted per-round ratios against Base64, which
    is what the change report compares. `enc_ns` and `dec_ns` are the absolute
    times, which the standing table needs because it has to add the compression
    stage on and re-form the ratio itself.
    """
    out: dict = defaultdict(lambda: {"input": 0, "json": 0, "raw": 0, "escapes": 0,
                                     "enc": 0.0, "iqr": 0.0,
                                     "enc_ns": 0.0, "dec_ns": 0.0,
                                     "native": False})
    for c in reported(results):
        e = out[(c["codec"], c["stage"])]
        # Whether the stage's cost has to be added to this row, or is already
        # inside it. Constant across a (codec, stage), by `reported`.
        e["native"] = c["native"]
        e["input"] += c["input"]
        e["json"] += c["json"]
        e["raw"] += c["raw"]
        e["escapes"] += c["esc"]
        e["enc"] += c["enc_raw_w"]
        e["iqr"] += c["enc_raw_q"]
        e["enc_ns"] += c["enc_ns"]
        e["dec_ns"] += c["dec_ns"]
    for e in out.values():
        if e["input"]:
            e["enc"] /= e["input"]
            e["iqr"] /= e["input"]
    return out


def compression_cost(results: dict) -> dict:
    """What the compression stage costs, per stage, over the whole corpus.

    Measured once per sample and shared by every codec behind it, so adding it
    to each codec's own time is what makes a row an end-to-end figure.
    """
    out: dict = defaultdict(lambda: {"c": 0.0, "d": 0.0})
    for row in comp_of(results):
        out[row["stage"]]["c"] += row["comp_ns"]
        out[row["stage"]]["d"] += row["decomp_ns"]
    return out


def pct(new: float, old: float) -> float:
    return (new / old - 1.0) * 100.0 if old else 0.0


# The cases a caller actually chooses between, weakest first: nothing at all,
# zstd at its fastest setting, and zstd at the everyday default. Levels 9 and 19
# are measured too and are on the page -- three tables is what fits in a comment
# while still showing how the field closes as the compressor does more of the
# work.
HEADLINE_STAGES = ("none", "zstd:-5", "zstd:1")

# What is worth saying about a particular level, and nothing more. The level
# itself is never written here: it is read out of the stage id below, because a
# hand-written heading is a heading that can say -1 over a table of level 1, and
# did.
STAGE_ASIDE = {
    "zstd:-5": "its fastest setting",
    "zstd:1": "the everyday default",
}


def stage_title(stage: str) -> str:
    """A stage id as a heading. The number comes from the id, always."""
    if stage == "none":
        return "Uncompressed"
    kind, _, level = stage.partition(":")
    if kind == "zstd" and level:
        aside = STAGE_ASIDE.get(stage)
        # A real minus sign for a negative level; the id spells it with a hyphen.
        shown = level.replace("-", "\u2212")
        return f"With zstd {shown} in front" + (f" ({aside})" if aside else "")
    return stage


def _row(label: str, e: dict, cc: dict, base_enc: float, base_dec: float) -> dict:
    """One codec at one stage, as the two numbers a reader came for.

    Size is a share of the *original* bytes, not of whatever the compressor left
    -- "my megabyte becomes this much JSON" is the question, and a ratio against
    the compressed intermediate answers a question nobody asked.

    Time counts the compression stage too, because the reader is choosing a
    pipeline and not a subroutine. It is the same stage for every codec in the table,
    so where it dominates every row lands near 100 % -- which is itself
    the answer: at that setting the encoder is not what costs you time.
    """
    return {
        "label": label,
        "json": e["json"] / e["input"],
        "raw": e["raw"] / e["input"],
        "escapes": e["escapes"],
        "enc": (e["enc_ns"] + cc["c"]) / base_enc if base_enc else 0.0,
        "dec": (e["dec_ns"] + cc["d"]) / base_dec if base_dec else 0.0,
    }


def headline_rows(results: dict, stage: str) -> list[dict]:
    t = totals(results)
    cc = compression_cost(results).get(stage, {"c": 0.0, "d": 0.0})
    base = t.get(("base64", stage))
    if not base:
        return []
    base_enc = base["enc_ns"] + cc["c"]
    base_dec = base["dec_ns"] + cc["d"]

    # A codec that carries its own compression already has the stage's cost
    # inside its measurement, so nothing is added to it; every other row has
    # the stage added on. Both are at the stage the table is for, so the
    # comparison is between two pipelines doing the same job at the same
    # setting.
    rows = [_row(codec, e, {"c": 0.0, "d": 0.0} if e["native"] else cc,
                 base_enc, base_dec)
            for (codec, st), e in t.items()
            if st == stage]

    rows.sort(key=lambda r: r["json"])
    return rows


def _size_cell(r: dict) -> str:
    """The size, and what escaping added to it where it added anything."""
    if r["escapes"] == 0:
        return f"{r['json'] * 100:.1f} %"
    return f"{r['json'] * 100:.1f} % ({r['raw'] * 100:.1f} % raw)"


def state_report(r: dict) -> list[str]:
    out = [
        "Size is the encoded payload as a share of the **original** bytes, with "
        "the pre-escape length in brackets where an alphabet needs escaping. "
        "Time is against Base64 doing the same job at the same setting, "
        "compression included: **under 100 % is faster**. Best per column in bold.",
        "",
    ]
    for stage in HEADLINE_STAGES:
        rows = headline_rows(r, stage)
        if not rows:
            continue
        title = stage_title(stage)
        # Bold what the reader sees. Two rows that both print 44.4 % differ
        # somewhere in the fourth decimal, and marking one of them the winner
        # over a difference the table does not show is worse than marking
        # neither -- so the comparison is on the rendered text.
        sizes = [_size_cell(x) for x in rows]
        encs = [f"{x['enc'] * 100:.0f} %" for x in rows]
        decs = [f"{x['dec'] * 100:.0f} %" for x in rows]
        best_size = sizes[min(range(len(rows)), key=lambda i: rows[i]["json"])]
        best_enc = encs[min(range(len(rows)), key=lambda i: rows[i]["enc"])]
        best_dec = decs[min(range(len(rows)), key=lambda i: rows[i]["dec"])]

        def cell(text: str, best: str) -> str:
            return f"**{text}**" if text == best else text

        out.append(f"**{title}**")
        out.append("")
        out.append("| codec | size in JSON | encode speed | decode speed |")
        out.append("|---|--:|--:|--:|")
        for x, size, enc, dec in zip(rows, sizes, encs, decs):
            out.append(
                f"| `{x['label']}` | {cell(size, best_size)} "
                f"| {cell(enc, best_enc)} | {cell(dec, best_dec)} |"
            )
        out.append("")
        # Under the table it qualifies rather than in a preamble every table has
        # to be read past. It is only the uncompressed table where the surprise
        # is worth spending a line on.
        if stage == "none":
            out.append(
                "_`base91z` decides its own compression, so its time already "
                "contains it where every other row has the stage added on. Here "
                "it is therefore the one row that may already have compressed: "
                "that is what a caller gets with no compressor in front._"
            )
            out.append("")
    return out


def diff_report(new: dict, old: dict) -> list[str]:
    """Head against base. Sizes exactly, speeds only past the measured noise."""
    n, o = by_category(new), by_category(old)
    shared = sorted(set(n) & set(o))

    out: list[str] = []

    # --- size: per category, exactly ---------------------------------
    size_moves = [(k, n[k], o[k]) for k in shared
                  if n[k]["json"] != o[k]["json"] or n[k]["raw"] != o[k]["raw"]]

    out.append("#### Size (exact)")
    out.append("")
    if not size_moves:
        out.append("No size changed, on any codec, at any stage, for any category.")
    else:
        # The escaped-character column is dropped when it would be a column of
        # zeroes, which is every change that leaves the alphabet alone. A column
        # that says nothing still costs a reader a glance to find that out.
        show_esc = any(a["escapes"] != b["escapes"] for _, a, b in size_moves)
        out.append("| codec | stage | category | Δ JSON bytes |"
                   + (" Δ escaped |" if show_esc else ""))
        out.append("|---|---|---|--:|" + ("--:|" if show_esc else ""))
        size_moves.sort(key=lambda x: -abs(x[1]["json"] - x[2]["json"]))
        for (codec, stage, cat), a, b in size_moves[:SIZE_ROWS]:
            d = a["json"] - b["json"]
            row = (f"| `{codec}` | `{stage}` | {cat} | "
                   f"{d:+,} ({pct(a['json'], b['json']):+.2f} %) |")
            if show_esc:
                row += f" {a['escapes'] - b['escapes']:+,} |"
            out.append(row)
        if len(size_moves) > SIZE_ROWS:
            out.append("")
            out.append(f"_{len(size_moves) - SIZE_ROWS} further cells moved less; "
                       f"the artifact has all of them._")
    out.append("")

    # --- speed: per codec and stage, over the whole corpus ------------
    # Deliberately coarser than the size table; see the module docstring for
    # the measurement that settled the grain.
    tn, to = totals(new), totals(old)
    speed_moves, quiet = [], 0
    for k in sorted(set(tn) & set(to)):
        a, b = tn[k], to[k]
        gap = a["enc"] - b["enc"]
        noise = max((a["iqr"] + b["iqr"]) / 2, SPEED_FLOOR * max(b["enc"], 1e-9))
        if abs(gap) > noise:
            speed_moves.append((k, a, b, gap, noise))
        elif gap:
            quiet += 1

    out.append("#### Speed (past the measured noise)")
    out.append("")
    if not speed_moves:
        tail = (f" ({quiet} codec/stage combination(s) changed by less, which "
                f"on a shared runner is not a result)") if quiet else ""
        out.append(f"Nothing moved further than this machine's own noise{tail}.")
    else:
        # The threshold is per row and varies, but it varies in the third
        # decimal and nobody decides anything on it. The rule is worth a line
        # under the table; the number is not worth a column beside every row.
        out.append("| codec | stage | encode vs base64 | change |")
        out.append("|---|---|--:|--:|")
        speed_moves.sort(key=lambda x: -abs(x[3]))
        for (codec, stage), a, b, gap, noise in speed_moves[:SPEED_ROWS]:
            arrow = "slower" if gap > 0 else "faster"
            out.append(
                f"| `{codec}` | `{stage}` | {b['enc']:.2f}× → {a['enc']:.2f}× "
                f"| **{pct(a['enc'], b['enc']):+.1f} % {arrow}** |"
            )
        out.append("")
        rest = []
        if len(speed_moves) > SPEED_ROWS:
            rest.append(f"{len(speed_moves) - SPEED_ROWS} further combination(s) moved less")
        if quiet:
            rest.append(f"{quiet} moved by less than the threshold")
        note = ("; " + ", ".join(rest)) if rest else ""
        out.append(
            f"_Called a change past the larger of the run's own spread and "
            f"{SPEED_FLOOR * 100:.0f} %{note}._"
        )
    out.append("")

    gone = sorted(set(o) - set(n))
    added = sorted(set(n) - set(o))
    if gone or added:
        bits = []
        if gone:
            bits.append(f"{len(gone)} cell(s) on base are missing here")
        if added:
            bits.append(f"{len(added)} cell(s) are new")
        out.append(f"_{'; '.join(bits)}._")
    return out


def build(new: dict, old: dict | None = None, title: str = "Benchmark") -> str:
    """The whole comment, as a string. Separated from `main` so the converter
    can run it over two files and compare, which is how the format change is
    shown to be exact rather than argued to be."""
    new = as_v3(new)
    old = as_v3(old) if old else None
    meta = new["meta"]
    count = sum(c[0] for c in new["corpus"])
    total = sum(c[1] for c in new["corpus"])

    lines = [f"## {title}", ""]
    lines.append(
        f"{count} samples \u00b7 {total:,} bytes \u00b7 {meta['rounds']} rounds \u00b7 "
        f"`{meta['cpu']}`"
    )
    lines.append("")

    if old:
        # Short, but not droppable: it is the reason the two columns are
        # comparable at all.
        lines.append("Head vs. base, both measured in this job.")
        lines.append("")
        lines += diff_report(new, old)
        lines.append("")
        lines.append("<details><summary>What each encoding costs on this head</summary>")
        lines.append("")
        lines += state_report(new)
        lines.append("")
        lines.append("</details>")
    else:
        lines += state_report(new)

    lines.append("")
    # Provenance stays in the source and out of the rendering. It is what makes
    # a number traceable back to a commit, so it cannot be dropped; it is also
    # identical on every run, so printing it only teaches people to scroll. The
    # page prints it in full, under Provenance, which is where somebody looking
    # for it goes.
    revs = " ".join(f"{k}={v}" for k, v in sorted((meta.get("codec_revisions") or {}).items()))
    lines.append(f"<!-- {revs} \u00b7 {meta['rustc']} \u00b7 {meta['generated']} -->")

    # Sections append their own trailing blank, so joining them leaves runs of
    # two and three. Markdown renders a run the same as a single one, which is
    # exactly why nobody notices they are there -- and they are still lines in a
    # comment that is meant to be short.
    out: list[str] = []
    for line in lines:
        if line == "" and out and out[-1] == "":
            continue
        out.append(line)
    return "\n".join(out).strip("\n") + "\n"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("results", type=Path)
    ap.add_argument("--baseline", type=Path, default=None)
    ap.add_argument("--title", default="Benchmark")
    args = ap.parse_args()

    base = load(args.baseline) if args.baseline and args.baseline.exists() else None
    sys.stdout.write(build(load(args.results), base, args.title))


if __name__ == "__main__":
    main()
