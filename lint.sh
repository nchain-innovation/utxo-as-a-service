#!/bin/bash

flake8 --ignore=E501,E131,E402,E722 python/src

mypy --check-untyped-defs --ignore-missing-imports python/src
