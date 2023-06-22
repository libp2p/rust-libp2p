#!/bin/bash
set -e

# cd to this script directory
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

./webtransport-tests/run.sh
