#!/usr/bin/env python3

import json
import re
import sys
from pathlib import Path


# Use this script by piping the output of clippy into it:
# > cargo clippy --workspace --all-features --all-targets --message-format=json -- -W unreachable_pub | python scripts/fix-unreachable-pub.py
#
# You might have to run this in a loop as restricting the visibility of one item can trigger the warning for another item.

def fix_unreachable_pub_warning(warning):
    file_path = Path(warning["spans"][0]["file_name"])

    try:
        line = warning["spans"][0]["line_start"]

        # Don't modify generated code
        if "generated" in str(file_path):
            return

        with file_path.open() as f:
            lines = f.readlines()

        pub_pattern = re.compile(r"(\s*)pub(\s)(.*)")
        match = pub_pattern.match(lines[line - 1])

        if match:
            indentation = match.group(1)
            space = match.group(2)
            rest_of_line = match.group(3)
            lines[line - 1] = f"{indentation}pub(crate){space}{rest_of_line}"

        with file_path.open("w") as f:
            f.writelines(lines)
    except Exception as e:
        print(f"Failed to apply suggestion in {file_path}: {e}")


def main():
    for line in sys.stdin:
        # Ignore other compiler messages
        if "unreachable_pub" not in line:
            continue

        warning = json.loads(line.strip())

        # Don't modify code that is not in the current workspace
        if str(Path.cwd()) not in str(warning['target']['src_path']):
            return

        m = warning["message"]

        if m is None:
            continue

        code = m['code']

        if code is None:
            continue

        fix_unreachable_pub_warning(m)


if __name__ == "__main__":
    main()
