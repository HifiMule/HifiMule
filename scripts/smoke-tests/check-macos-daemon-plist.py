#!/usr/bin/env python3
"""Verify the installed daemon's embedded Mach-O Info.plist."""

import plistlib
import subprocess
import sys


def embedded_plist(executable: str) -> dict:
    output = subprocess.check_output(
        ["otool", "-s", "__TEXT", "__info_plist", executable], text=True
    )
    words = []
    for line in output.splitlines()[2:]:
        columns = line.split()
        if not columns:
            continue
        for word in columns[1:]:
            data = bytes.fromhex(word)
            words.append(data[::-1] if len(data) == 4 else data)
    return plistlib.loads(b"".join(words).rstrip(b"\0"))


if __name__ == "__main__":
    description = embedded_plist(sys.argv[1]).get("NSLocalNetworkUsageDescription")
    if not isinstance(description, str) or not description.strip():
        raise SystemExit("Daemon is missing its embedded local-network usage description")
