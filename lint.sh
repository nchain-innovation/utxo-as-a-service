#!/bin/bash

# Requires: uv sync --all-groups

uv run flake8 --ignore=E501,E131,E402,E722 python/src

uv run mypy --check-untyped-defs --ignore-missing-imports python/src
