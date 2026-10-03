#!/bin/python3
"""Write to stdout and stderr and exit as told -- the spec helper, ported.

The first argument (default STDOUT) on stdout, the second (default STDERR) on
stderr, and exit with the third (default 0), as Oils' Python 2
`spec/bin/stdout_stderr.py` did, for testing redirects. Port for SlateOS
(scripts/oils-spec/sh_spec.py)."""
import os
import sys

args = [os.fsencode(a) for a in sys.argv]
stdout = args[1] if len(args) > 1 else b'STDOUT'
stderr = args[2] if len(args) > 2 else b'STDERR'
status = int(args[3]) if len(args) > 3 else 0
sys.stdout.buffer.write(stdout + b'\n')
sys.stdout.buffer.flush()
sys.stderr.buffer.write(stderr + b'\n')
sys.stderr.buffer.flush()
sys.exit(status)
