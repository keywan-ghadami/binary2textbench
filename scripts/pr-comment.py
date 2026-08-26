#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

"""Posts the benchmark report onto the pull request, in one comment.

    python3 scripts/pr-comment.py report.md

One comment, updated in place on every run rather than added to. A benchmark
that leaves a new comment per push buries the conversation it is supposed to
inform, and by the fifth push nobody reads any of them. The comment is found
again by a hidden marker, so renaming the report or changing its wording does
not orphan it.

Everything it needs comes from the environment GitHub Actions already sets:
GITHUB_TOKEN, GITHUB_REPOSITORY and GITHUB_EVENT_PATH. It is a no-op rather
than an error when any of them is missing, or when the event is not a pull
request -- a push build has nowhere to post, and that is not a failure.

A pull request from a fork gets a read-only token, so the API call is refused.
That is reported and shrugged off: the job summary carries the same report and
is readable by anyone who can see the run.
"""

from __future__ import annotations

import json
import os
import sys
import urllib.error
import urllib.request

MARKER = "<!-- binary2textbench-report -->"

# GitHub rejects an issue comment body over 65536 characters. Leave room for
# the marker and the truncation notice.
LIMIT = 64_000


def api(url: str, token: str, method: str = "GET", payload: dict | None = None) -> dict:
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if data:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read() or "{}")


def truncate(body: str) -> str:
    if len(body) <= LIMIT:
        return body
    cut = body[:LIMIT].rsplit("\n", 1)[0]
    return cut + (
        "\n\n_Report truncated to fit a comment. The whole of it is in the job "
        "summary and in the run's artifact._\n"
    )


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    report = sys.argv[1]

    token = os.environ.get("GITHUB_TOKEN") or ""
    repo = os.environ.get("GITHUB_REPOSITORY") or ""
    event_path = os.environ.get("GITHUB_EVENT_PATH") or ""
    if not (token and repo and event_path and os.path.exists(event_path)):
        print("not running under Actions with a token; nothing posted", file=sys.stderr)
        return 0

    event = json.loads(open(event_path).read())
    pr = event.get("pull_request") or {}
    number = pr.get("number")
    if not number:
        print("not a pull request event; nothing posted", file=sys.stderr)
        return 0

    body = MARKER + "\n" + truncate(open(report).read())
    base = f"https://api.github.com/repos/{repo}"

    try:
        # Find the comment this job left last time. Paginated, because a busy
        # pull request can have more than one page of comments and the marker
        # is as likely to be on the first page as the fifth.
        existing = None
        page = 1
        while page <= 10 and existing is None:
            comments = api(f"{base}/issues/{number}/comments?per_page=100&page={page}", token)
            if not comments:
                break
            for c in comments:
                if MARKER in (c.get("body") or ""):
                    existing = c
                    break
            page += 1

        if existing:
            api(f"{base}/issues/comments/{existing['id']}", token, "PATCH", {"body": body})
            print(f"updated comment {existing['id']} on #{number}", file=sys.stderr)
        else:
            api(f"{base}/issues/{number}/comments", token, "POST", {"body": body})
            print(f"posted a new comment on #{number}", file=sys.stderr)
    except urllib.error.HTTPError as e:
        if e.code in (401, 403, 404):
            # A fork's token is read-only, and a repository can be configured
            # to withhold write permissions from workflows entirely. Neither is
            # a broken benchmark.
            print(
                f"cannot comment on #{number} ({e.code}): the token has no write "
                "access -- expected on a pull request from a fork. The report is "
                "in the job summary.",
                file=sys.stderr,
            )
            return 0
        raise
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
