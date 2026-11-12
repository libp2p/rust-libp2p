#!/bin/bash
set -e

# cd to this script directory
cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

./timer-tests/run.sh
./webtransport-tests/run.sh
