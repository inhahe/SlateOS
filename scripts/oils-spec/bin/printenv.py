#!/bin/python3
"""Print each named environment variable, or `None` -- the spec helper, ported.

As Oils' Python 2 `spec/bin/printenv.py` did: the value's bytes as they are.
Port for SlateOS (scripts/oils-spec/sh_spec.py)."""
import os
import sys

for name in sys.argv[1:]:
    value = os.environb.get(os.fsencode(name))
    sys.stdout.buffer.write((b'None' if value is None else value) + b'\n')
