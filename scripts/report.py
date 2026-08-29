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
machine will not support it. Measured across three runs of identical code:

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
stop reading the ones that do not. So provenance goes in an HTML comment -- in
the source, out of the rendering -- tables carry the delta rather than the delta
beside the absolute it is a delta of, and a column identical on both sides is
dropped rather than printed as zeroes. The standing figures, every stage and
every category and weightable, are on the page.
"""

from __future__ import annotations

import argparse
import sys
from collections import defaultdict
from pathlib import Path

# results.json holds sums per (codec, stage, group, category); results.py is the
# only module here that knows the row layout, and runner-rust/src/compact.rs
# explains why the per-sample rows are not in the file.
from results import load, pair_label

# A speed change has to clear both the measured spread and this floor before it
# is called anything. 8 % sits above the 5.31 % worst case measured between runs
# of identical code (see above) with enough headroom that a quieter or busier
# runner does not manufacture regressions. Re-measure if the corpus or the
# runner changes: it is calibration, not a preference.
#
# The page applies the same floor when deciding whether two bars are tied. If
# this moves, move it there too, or have the runner write meta.speed_floor.
SPEED_FLOOR = 0.08

# Rows printed before the rest becomes a count. Both tables are sorted
# largest-move-first, so what is cut is what moved least. Speed is capped lower
# because it is already the coarser of the two.
SIZE_ROWS = 20
SPEED_ROWS = 12


def by_category(r: dict) -> dict:
    """(codec, stage, pair) -> the figures for one cell.

    One cell per key already, so nothing is accumulated here: `enc_raw` and
    `enc_q` are stored as ratios and are used as they stand.
    """
    out = {}
    for c in r["cells"]:
        out[(c["codec"], c["stage"], c["pair"])] = {
            "input": c["input"], "json": c["json"], "raw": c["raw"],
            "escapes": c["esc"], "enc": c["enc_raw"], "iqr": c["enc_q"],
        }
    return out


def totals(r: dict) -> dict:
    """Whole-corpus totals per (codec, stage).

    `enc` and `iqr` are the byte-weighted ratios against the baseline, which is
    what the change report compares. `enc_ns` and `dec_ns` are absolute times,
    which the standing table needs because it adds the compression stage on and
    re-forms the ratio itself.
    """
    out: dict = defaultdict(lambda: {"input": 0, "json": 0, "raw": 0,
                                     "escapes": 0, "enc": 0.0, "iqr": 0.0,
                                     "enc_ns": 0.0, "dec_ns": 0.0,
                                     "native": False})
    for c in r["cells"]:
        e = out[(c["codec"], c["stage"])]
        # Whether the stage's cost has to be added to this row or is already
        # inside it. Constant across a (codec, stage).
        e["native"] = c["native"]
        n = c["input"]
        e["input"] += n
        e["json"] += c["json"]
        e["raw"] += c["raw"]
        e["escapes"] += c["esc"]
        e["enc"] += c["enc_raw"] * n
        e["iqr"] += c["enc_q"] * n
    for (codec, stage), e in out.items():
        t = r["times"].get((codec, stage, e["native"]))
        if t:
            e["enc_ns"] = t["enc_ns"]
            e["dec_ns"] = t["dec_ns"]
        if e["input"]:
            e["enc"] /= e["input"]
            e["iqr"] /= e["input"]
    return out


def compression_cost(r: dict) -> dict:
    """What the compression stage costs, per stage, over the whole corpus.

    Measured once per sample and shared by every codec behind it, so adding it
    to each codec's own time is what makes a row an end-to-end figure.
    """
    out: dict = defaultdict(lambda: {"c": 0.0, "d": 0.0})
    for (stage, _grp, _cat), row in r["stage_sizes"].items():
        out[stage]["c"] += row["comp_ns"]
        out[stage]["d"] += row["decomp_ns"]
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
# hand-written heading is one that can say -1 over a table of level 1, and did.
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

    Size is a share of the *original* bytes, not of whatever the compressor
    left: "my megabyte becomes this much JSON" is the question, and a ratio
    against the compressed intermediate answers a question nobody asked.

    Time counts the compression stage too, because the reader is choosing a
    pipeline and not a subroutine. It is the same stage for every codec in the
    table, so where it dominates every row lands near 100 % -- which is itself
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


def headline_rows(r: dict, stage: str) -> list[dict]:
    t = totals(r)
    cc = compression_cost(r).get(stage, {"c": 0.0, "d": 0.0})
    baseline = r["meta"]["baseline"]
    base = t.get((baseline, stage))
    if not base:
        return []
    base_enc = base["enc_ns"] + cc["c"]
    base_dec = base["dec_ns"] + cc["d"]

    # A codec that carries its own compression already has the stage's cost
    # inside its measurement, so nothing is added to it; every other row has the
    # stage added on. Both are at the stage the table is for, so the comparison
    # is between two pipelines doing the same job at the same setting.
    rows = [_row(codec, e, {"c": 0.0, "d": 0.0} if e["native"] else cc,
                 base_enc, base_dec)
            for (codec, st), e in t.items() if st == stage]
    rows.sort(key=lambda x: x["json"])
    return rows


def _size_cell(x: dict) -> str:
    """The size, and what escaping added to it where it added anything."""
    if x["escapes"] == 0:
        return f"{x['json'] * 100:.1f} %"
    return f"{x['json'] * 100:.1f} % ({x['raw'] * 100:.1f} % raw)"


def state_report(r: dict) -> list[str]:
    out = [
        "Size is the encoded payload as a share of the **original** bytes, with "
        "the pre-escape length in brackets where an alphabet needs escaping. "
        "Time is against the baseline codec doing the same job at the same "
        "setting, compression included: **under 100 % is faster**. Best per "
        "column in bold.",
        "",
    ]
    for stage in HEADLINE_STAGES:
        rows = headline_rows(r, stage)
        if not rows:
            continue
        # Bold what the reader sees. Two rows that both print 44.4 % differ
        # somewhere the table does not show, and marking one of them the winner
        # over a difference nobody can read is worse than marking neither.
        sizes = [_size_cell(x) for x in rows]
        encs = [f"{x['enc'] * 100:.0f} %" for x in rows]
        decs = [f"{x['dec'] * 100:.0f} %" for x in rows]
        best_size = sizes[min(range(len(rows)), key=lambda i: rows[i]["json"])]
        best_enc = encs[min(range(len(rows)), key=lambda i: rows[i]["enc"])]
        best_dec = decs[min(range(len(rows)), key=lambda i: rows[i]["dec"])]

        def cell(text: str, best: str) -> str:
            return f"**{text}**" if text == best else text

        out.append(f"**{stage_title(stage)}**")
        out.append("")
        out.append("| codec | size in JSON | encode speed | decode speed |")
        out.append("|---|--:|--:|--:|")
        for x, size, enc, dec in zip(rows, sizes, encs, decs):
            out.append(f"| `{x['label']}` | {cell(size, best_size)} "
                       f"| {cell(enc, best_enc)} | {cell(dec, best_dec)} |")
        out.append("")
        # Under the table it qualifies rather than in a preamble every table has
        # to be read past, and only where the surprise is worth a line.
        if stage == "none":
            native = [c["codec"] for c in r["cells"] if c["native"]]
            if native:
                name = sorted(set(native))[0]
                out.append(
                    f"_`{name}` decides its own compression, so its time already "
                    f"contains it where every other row has the stage added on. "
                    f"Here it is therefore the one row that may already have "
                    f"compressed: that is what a caller gets with no compressor "
                    f"in front._")
                out.append("")
    return out


def diff_report(new: dict, old: dict) -> list[str]:
    """Head against base. Sizes exactly, speeds only past the measured noise."""
    n, o = by_category(new), by_category(old)
    shared = sorted(set(n) & set(o))
    label = pair_label([k[2] for k in shared])

    out: list[str] = []

    # --- size: per category, exactly ---------------------------------
    moves = [(k, n[k], o[k]) for k in shared
             if n[k]["json"] != o[k]["json"] or n[k]["raw"] != o[k]["raw"]]

    out.append("#### Size (exact)")
    out.append("")
    if not moves:
        out.append("No size changed, on any codec, at any stage, for any category.")
    else:
        # The escaped-character column is dropped when it would be a column of
        # zeroes, which is every change that leaves the alphabet alone.
        show_esc = any(a["escapes"] != b["escapes"] for _, a, b in moves)
        out.append("| codec | stage | category | Δ JSON bytes |"
                   + (" Δ escaped |" if show_esc else ""))
        out.append("|---|---|---|--:|" + ("--:|" if show_esc else ""))
        moves.sort(key=lambda x: -abs(x[1]["json"] - x[2]["json"]))
        for (codec, stage, pair), a, b in moves[:SIZE_ROWS]:
            d = a["json"] - b["json"]
            row = (f"| `{codec}` | `{stage}` | {label[pair]} | "
                   f"{d:+,} ({pct(a['json'], b['json']):+.2f} %) |")
            if show_esc:
                row += f" {a['escapes'] - b['escapes']:+,} |"
            out.append(row)
        if len(moves) > SIZE_ROWS:
            out.append("")
            out.append(f"_{len(moves) - SIZE_ROWS} further cells moved less; the "
                       f"page has all of them._")
    out.append("")

    # --- speed: per codec and stage, over the whole corpus ------------
    # Deliberately coarser than the size table; see the module docstring for
    # the measurement that settled the grain.
    tn, to = totals(new), totals(old)
    speed, quiet = [], 0
    for k in sorted(set(tn) & set(to)):
        a, b = tn[k], to[k]
        gap = a["enc"] - b["enc"]
        noise = max((a["iqr"] + b["iqr"]) / 2, SPEED_FLOOR * max(b["enc"], 1e-9))
        if abs(gap) > noise:
            speed.append((k, a, b, gap))
        elif gap:
            quiet += 1

    out.append("#### Speed (past the measured noise)")
    out.append("")
    if not speed:
        tail = (f" ({quiet} codec/stage combination(s) changed by less, which on "
                f"a shared runner is not a result)") if quiet else ""
        out.append(f"Nothing moved further than this machine's own noise{tail}.")
    else:
        # The threshold is per row and varies, but it varies in the third
        # decimal and nobody decides anything on it. The rule is worth a line
        # under the table; the number is not worth a column beside every row.
        out.append("| codec | stage | encode vs baseline | change |")
        out.append("|---|---|--:|--:|")
        speed.sort(key=lambda x: -abs(x[3]))
        for (codec, stage), a, b, gap in speed[:SPEED_ROWS]:
            arrow = "slower" if gap > 0 else "faster"
            out.append(f"| `{codec}` | `{stage}` | {b['enc']:.2f}× → "
                       f"{a['enc']:.2f}× | "
                       f"**{pct(a['enc'], b['enc']):+.1f} % {arrow}** |")
        out.append("")
        rest = []
        if len(speed) > SPEED_ROWS:
            rest.append(f"{len(speed) - SPEED_ROWS} further combination(s) moved less")
        if quiet:
            rest.append(f"{quiet} moved by less than the threshold")
        note = ("; " + ", ".join(rest)) if rest else ""
        out.append(f"_Called a change past the larger of the run's own spread and "
                   f"{SPEED_FLOOR * 100:.0f} %{note}._")
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
    """The whole comment as a string, separated from `main` so it can be run
    over two files and compared rather than eyeballed."""
    meta = new["meta"]
    count = sum(v["samples"] for v in new["corpus"].values())
    total = sum(v["bytes"] for v in new["corpus"].values())

    lines = [f"## {title}", ""]
    lines.append(f"{count} samples · {total:,} bytes · {meta['rounds']} rounds · "
                 f"`{meta['cpu']}`")
    lines.append("")

    if old:
        # Short, but not droppable: it is the reason the two columns are
        # comparable at all.
        lines.append("Head vs. base, both measured in this job.")
        lines.append("")
        lines += diff_report(new, old)
        lines.append("")
        lines.append("<details><summary>What each encoding costs on this head"
                     "</summary>")
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
    revs = " ".join(f"{k}={v}" for k, v in
                    sorted((meta.get("codec_revisions") or {}).items()))
    lines.append(f"<!-- {revs} · {meta['rustc']} · {meta['generated']} -->")

    # Sections append their own trailing blank, so joining them leaves runs of
    # two and three. Markdown renders a run the same as a single one, which is
    # exactly why nobody notices they are there -- and they are still lines in a
    # comment meant to be short.
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

    base = (load(args.baseline)
            if args.baseline and args.baseline.exists() else None)
    sys.stdout.write(build(load(args.results), base, args.title))


if __name__ == "__main__":
    main()
