#!/usr/bin/env sh
# Creates the Python virtualenv used by the external provider workers.
set -eu
cd "$(dirname "$0")/../workers"
python3 -m venv .venv
.venv/bin/pip install --quiet --upgrade pip
.venv/bin/pip install --quiet -r requirements.txt
.venv/bin/playwright install chromium
echo "worker ready: $(pwd)/.venv/bin/python"
