#!/bin/bash

# Requires: uv sync --all-groups

# Without this, the script exits with mypy's status alone and a flake8 failure
# is silently discarded — which is how three findings sat in python/src while
# CI stayed green.
set -euo pipefail

# Both linters run even when the first one fails, so one invocation reports
# everything rather than making you fix flake8 to discover mypy. `|| status=1`
# is a compound command, so `set -e` does not short-circuit here.
status=0

# W503 (line break before binary operator) is mutually exclusive with W504;
# one of the two must be disabled. PEP 8 recommends breaking before the
# operator, which is also what the multi-line conditions here already do.
uv run flake8 --ignore=E501,E131,E402,E722,W503 python/src || status=1

uv run mypy --check-untyped-defs --ignore-missing-imports python/src || status=1

exit "$status"
