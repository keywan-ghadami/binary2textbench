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
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path

# A speed change has to clear both the measured spread and this floor before it
# is called anything. 8 % sits above the 5.31 % worst case measured between runs
# of identical code (see the module docstring) with enough headroom that a
# quieter or busier runner does not start manufacturing regressions. Re-measure
# it if the corpus or the runner changes: it is calibration, not a preference.
SPEED_FLOOR = 0.08


def load(path: Path) -> dict:
    return json.loads(path.read_text())


def by_category(results: dict) -> dict:
    """Aggregate to (codec, stage, category): the level a reader can act on.

    Per-sample rows are too many to read and per-codec totals hide which kind
    of input moved, which is usually the whole story.
    """
    cat = {s["name"]: s["category"] for s in results["corpus"]["samples"]}
    out: dict = defaultdict(lambda: {
        "input": 0, "json": 0, "raw": 0, "escapes": 0,
        "enc": 0.0, "encw": 0.0, "iqr": 0.0, "dec": 0.0,
    })
    for m in results["measurements"]:
        k = (m["codec"], m["stage"], cat[m["sample"]])
        e = out[k]
        n = m["input_bytes"]
        e["input"] += n
        e["json"] += m["json_bytes"]
        e["raw"] += m["encoded_bytes"]
        e["escapes"] += m["escapes"]
        e["enc"] += m["encode_rel"]["ns"] * n
        e["dec"] += m["decode_rel"]["ns"] * n
        e["iqr"] += m["encode_rel"]["iqr_ns"] * n
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
                                     "enc_ns": 0.0, "dec_ns": 0.0})
    for m in results["measurements"]:
        e = out[(m["codec"], m["stage"])]
        n = m["input_bytes"]
        e["input"] += n
        e["json"] += m["json_bytes"]
        e["raw"] += m["encoded_bytes"]
        e["escapes"] += m["escapes"]
        e["enc"] += m["encode_rel"]["ns"] * n
        e["iqr"] += m["encode_rel"]["iqr_ns"] * n
        e["enc_ns"] += m["encode"]["ns"]
        e["dec_ns"] += m["decode"]["ns"]
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
    for row in results["compression"]:
        out[row["stage"]]["c"] += row["compress"]["ns"]
        out[row["stage"]]["d"] += row["decompress"]["ns"]
    return out


def pct(new: float, old: float) -> float:
    return (new / old - 1.0) * 100.0 if old else 0.0


# The two cases a caller actually chooses between. Every zstd level is in the
# artifact and on the page; two tables is what fits in a comment and answers the
# question somebody opens it with.
HEADLINE_STAGES = (("none", "Uncompressed"), ("zstd:1", "With zstd −1 in front"))


def _row(label: str, e: dict, cc: dict, base_enc: float, base_dec: float) -> dict:
    """One codec at one stage, as the two numbers a reader came for.

    Size is a share of the *original* bytes, not of whatever the compressor left
    -- "my megabyte becomes this much JSON" is the question, and a ratio against
    the compressed intermediate answers a question nobody asked.

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


def headline_rows(results: dict, stage: str) -> list[dict]:
    t = totals(results)
    cc = compression_cost(results).get(stage, {"c": 0.0, "d": 0.0})
    base = t.get(("base64", stage))
    if not base:
        return []
    base_enc = base["enc_ns"] + cc["c"]
    base_dec = base["dec_ns"] + cc["d"]

    rows = [_row(codec, e, cc, base_enc, base_dec)
            for (codec, st), e in t.items() if st == stage]

    # Base91z decides for itself whether to compress, so beside a table of
    # codecs behind a compressor it is the honest entry for what it does. Its
    # own compression is inside its time already; nothing is added.
    if stage != "none":
        auto = t.get(("base91z", "auto"))
        if auto:
            rows.append(_row("base91z (auto)", auto, {"c": 0.0, "d": 0.0},
                             base_enc, base_dec))

    rows.sort(key=lambda r: r["json"])
    return rows


def _size_cell(r: dict) -> str:
    """The size, and what escaping added to it where it added anything."""
    if r["escapes"] == 0:
        return f"{r['json'] * 100:.1f} %"
    return f"{r['json'] * 100:.1f} % ({r['raw'] * 100:.1f} % raw)"


def state_report(r: dict) -> list[str]:
    out = [
        "Size is the encoded payload as a share of the **original** bytes -- what "
        "ends up inside the JSON string. Where an alphabet needs escaping, the "
        "length before escaping follows in brackets. Time is against Base64 doing "
        "the same job at the same setting, compression included: **under 100 % is "
        "faster**. Best in each column is bold.",
        "",
    ]
    for stage, title in HEADLINE_STAGES:
        rows = headline_rows(r, stage)
        if not rows:
            continue
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
        out.append("| codec | size in JSON | encode | decode |")
        out.append("|---|--:|--:|--:|")
        for x, size, enc, dec in zip(rows, sizes, encs, decs):
            out.append(
                f"| `{x['label']}` | {cell(size, best_size)} "
                f"| {cell(enc, best_enc)} | {cell(dec, best_dec)} |"
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

    out.append("#### Size — exact, no threshold")
    out.append("")
    if not size_moves:
        out.append("No size changed, on any codec, at any stage, for any category.")
    else:
        out.append("| codec | stage | category | JSON bytes | change | escaped chars |")
        out.append("|---|---|---|--:|--:|--:|")
        size_moves.sort(key=lambda x: -abs(x[1]["json"] - x[2]["json"]))
        for (codec, stage, cat), a, b in size_moves[:40]:
            d = a["json"] - b["json"]
            out.append(
                f"| `{codec}` | `{stage}` | {cat} | {a['json']:,} | "
                f"{d:+,} ({pct(a['json'], b['json']):+.2f} %) | "
                f"{a['escapes']:,} ({a['escapes'] - b['escapes']:+,}) |"
            )
        if len(size_moves) > 40:
            out.append(f"| … | | | | _{len(size_moves) - 40} more_ | |")
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

    out.append("#### Speed — whole corpus, only what clears the measured spread")
    out.append("")
    if not speed_moves:
        out.append(
            f"Nothing moved further than this machine's own noise. "
            f"({quiet} codec/stage combinations changed by less than that; on a "
            f"shared runner such a move is not a result.)"
        )
    else:
        out.append("| codec | stage | encode vs base64 | change | called a change past |")
        out.append("|---|---|--:|--:|--:|")
        speed_moves.sort(key=lambda x: -abs(x[3]))
        for (codec, stage), a, b, gap, noise in speed_moves:
            arrow = "slower" if gap > 0 else "faster"
            out.append(
                f"| `{codec}` | `{stage}` | {b['enc']:.2f}× → {a['enc']:.2f}× "
                f"| **{pct(a['enc'], b['enc']):+.1f} % {arrow}** | ±{noise:.3f} |"
            )
        out.append("")
        out.append(f"_{quiet} other combinations moved by less than the noise floor._")
    out.append("")

    gone = sorted(set(o) - set(n))
    added = sorted(set(n) - set(o))
    if gone or added:
        out.append("")
        if gone:
            out.append(f"_{len(gone)} cell(s) present on the base branch are missing here._")
        if added:
            out.append(f"_{len(added)} cell(s) are new._")
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("results", type=Path)
    ap.add_argument("--baseline", type=Path, default=None)
    ap.add_argument("--title", default="Benchmark")
    args = ap.parse_args()

    new = load(args.results)
    meta = new["meta"]
    corpus = new["corpus"]["samples"]
    total = sum(s["bytes"] for s in corpus)

    lines = [f"## {args.title}", ""]
    lines.append(
        f"{len(corpus)} samples · {total:,} bytes · {meta['rounds']} rounds · "
        f"`{meta['cpu']}`"
    )
    lines.append("")

    if args.baseline and args.baseline.exists():
        old = load(args.baseline)
        lines.append(
            "Head against the base branch, both measured in this job on this "
            "machine — which is why the two are comparable at all."
        )
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
    revs = ", ".join(f"{k} `{v[:12]}`" for k, v in (meta.get("codec_revisions") or {}).items())
    lines.append(f"<sub>{revs} · {meta['rustc']} · {meta['generated']}</sub>")

    sys.stdout.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
