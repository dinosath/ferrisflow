#!/usr/bin/env python3
"""Validate the commit subject against the Conventional Commits format."""

from __future__ import annotations

import re
import sys
from pathlib import Path


SUBJECT_PATTERN = re.compile(
    r"^[a-z][a-z0-9-]*(?:\([a-zA-Z0-9][a-zA-Z0-9._/-]*\))?!?: .+\S$"
)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check-conventional-commit.py <commit-message-file>", file=sys.stderr)
        return 2

    message_path = Path(sys.argv[1])
    lines = message_path.read_text(encoding="utf-8").splitlines()
    subject = lines[0].strip() if lines else ""
    if SUBJECT_PATTERN.fullmatch(subject):
        return 0

    print(
        "Commit subject must use Conventional Commits format: "
        "<type>[optional scope][!]: <description>\n"
        "Example: feat(auth): add OIDC sign-in",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
