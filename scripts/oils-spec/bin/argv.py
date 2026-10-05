#!/bin/python3
"""Print the arguments the way Oils' Python 2 `spec/bin/argv.py` did.

Upstream's is `print(sys.argv[1:])` under Python 2: the list's repr, of byte
strings, e.g. `['a', 'b\\xc3\\xa9']`. 772 spec cases compare against exactly
that. Python 3's repr of the same arguments as bytes has the same quoting and
the same escapes behind a `b`, so dropping the `b` gives Python 2's text.
Port for SlateOS, whose only Python is 3 (scripts/oils-spec/sh_spec.py).
"""
import os
import sys

items = b', '.join(repr(os.fsencode(a))[1:].encode('ascii') for a in sys.argv[1:])
sys.stdout.buffer.write(b'[' + items + b']\n')
