#!/usr/bin/env bash
set -euo pipefail

SCRIPTPATH="$( cd "$(dirname "$0")" ; pwd -P )"
cd "$SCRIPTPATH/.."

cargo fmt --all
