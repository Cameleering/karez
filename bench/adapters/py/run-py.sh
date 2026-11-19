#!/bin/sh
# Runs the Python adapter with the benchmark virtualenv (BENCH_VENV, default ~/venv).
exec "${BENCH_VENV:-$HOME/venv}/bin/python" "$(dirname "$0")/pyadapter.py" "$@"
