#!/bin/python3
"""Print up to 1024 bytes from each descriptor named -- the spec helper, ported.

Each after `N: `, as Oils' Python 2 `spec/bin/read_from_fd.py` did. The shell
under test is the one that opens the descriptors. Port for SlateOS
(scripts/oils-spec/sh_spec.py)."""
import os
import sys

for arg in sys.argv[1:]:
    fd = int(arg)
    try:
        data = os.read(fd, 1024)
    except OSError as e:
        sys.stdout.buffer.flush()
        print('FATAL: Error reading from fd %d: %s' % (fd, e), file=sys.stderr)
        sys.exit(1)
    sys.stdout.buffer.write(b'%d: ' % fd)
    sys.stdout.buffer.write(data)
