#!/usr/bin/env bash
# only exit with zero if all commands of the pipeline exit successfully
set -o pipefail
# error on unset variables
set -u

FILENAMES=$(grep --exclude-dir target --include \*.rs -Przl "\#\[.*\]\n\/\/.*")

if [ $? -eq 0 ]
then
	cat << EOF
Please put attributes below comments.

Do:

// Some comment
#[test]
fn test() {

Don't:

#[test]
// Some comment
fn test() {

Files:

$FILENAMES
EOF

	exit 1
fi
