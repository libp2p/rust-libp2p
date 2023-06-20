#!/bin/bash

# Usage: ./scripts/list-external-contributors.sh <TAG>

set -e

TAG=$1

release_date=$(git log -1 --format=%aI --date=iso-strict $TAG)
authors=$(gh api "repos/libp2p/rust-libp2p/commits?since=$release_date" --paginate -q '.[].author.login' | sort -u)
unique_authors=$(echo "$authors" | sort -u)
team_members=$(gh api teams/6797340/members --paginate | jq -r '.[].login' | sort -u)

echo "$unique_authors" | grep -vxF -f <(echo "$team_members") | grep -vF "bot" | grep -vF "web-flow"
