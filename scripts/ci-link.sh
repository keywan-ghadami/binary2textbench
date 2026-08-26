#!/bin/sh
# Points runner-rust at one particular checkout of one codec, and at whatever
# the workflow gave for the other two.
#
#     ci-link.sh <codec-name> <codec-path> <base91z> <base85n> <base94max>
#
# The codec under test overrides whatever path was given for it, which is what
# lets the same action measure a pull request's head and then its base branch
# by being called twice with different first arguments.
set -eu

name=${1:-}
under_test=${2:-}
b91z=${3:-}
b85n=${4:-}
b94m=${5:-}

case "$name" in
    base91z)   b91z=$under_test ;;
    base85n)   b85n=$under_test ;;
    base94max) b94m=$under_test ;;
    "")        ;;
    *)         echo "ci-link.sh: unknown codec '$name'" >&2; exit 2 ;;
esac

for pair in "base91z:$b91z" "base85n:$b85n" "base94max:$b94m"; do
    codec=${pair%%:*}
    path=${pair#*:}
    if [ -z "$path" ]; then
        echo "ci-link.sh: no checkout given for $codec" >&2
        echo "  pass ${codec}-path, or name it as the codec under test" >&2
        exit 2
    fi
done

here=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec "$here/scripts/link-codecs.sh" "$b91z" "$b85n" "$b94m"
