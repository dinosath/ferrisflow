#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: check-conventional-commit.sh <commit-message-file>" >&2
    exit 2
fi

convco check --from-stdin < "$1"
