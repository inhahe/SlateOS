#!/bin/python3
"""List this process's open descriptors -- the spec helper, ported.

From /proc/self/fd, one `FD TARGET` line each, as Oils' Python 2
`spec/bin/show_fd_table.py` did: in directory order, and including the
descriptor the listing itself opened. Port for SlateOS
(scripts/oils-spec/sh_spec.py)."""
import os
import sys

out = sys.stdout.buffer
d = '/proc/self/fd/'
for fd in os.listdir(d):
    path = os.path.join(d, fd)
    try:
        target = os.readlink(os.fsencode(path))
    except OSError as e:
        # Python 2 formatted the error with the path it was given, a str.
        out.write(('%s [Errno %d] %s: %r\n' % (fd, e.errno, e.strerror, path)).encode())
    else:
        out.write(fd.encode() + b' ' + target + b'\n')
