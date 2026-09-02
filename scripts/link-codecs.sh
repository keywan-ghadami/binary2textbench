#!/bin/sh
# Points runner-rust at the codec checkouts to measure.
#
# The runner's Cargo.toml uses path dependencies under codecs/, and this script
# is what fills that directory in. Which revision of each codec gets measured is
# therefore a property of the run and not of this repository -- CI checks out a
# pull request's head and links that, a developer links whatever is on their
# disk, and neither has to edit a manifest to do it.
#
#     scripts/link-codecs.sh                       # siblings of this repository
#     scripts/link-codecs.sh /path/to/base91z ...  # explicit, in the order below
#
# Order: base91z, base85n, base94max, base65t.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$here/codecs"

link() {
    name=$1
    src=$2
    if [ ! -d "$src" ]; then
        echo "$0: no checkout of $name at $src" >&2
        echo "  clone it there, or pass the four paths explicitly" >&2
        exit 1
    fi
    abs=$(CDPATH= cd -- "$src" && pwd)
    ln -sfn "$abs" "$here/codecs/$name"
    echo "  $name -> $abs"
}

if [ $# -eq 4 ]; then
    link base91z "$1"
    link base85n "$2"
    link base94max "$3"
    link base65t "$4"
elif [ $# -eq 0 ]; then
    parent=$(dirname "$here")
    link base91z "$parent/base91z"
    link base85n "$parent/base85n"
    link base94max "$parent/base94max"
    link base65t "$parent/base65t"
else
    echo "usage: $0 [base91z-path base85n-path base94max-path base65t-path]" >&2
    exit 2
fi
