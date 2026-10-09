#!/usr/bin/env python3
"""Run a program on a pseudo-terminal of its own, for curses-diff.sh.

Usage: curses-ptyrun.py ROWS COLS OUT ERR PROG [ARG...]

The terminal is ROWS by COLS and is PROG's controlling terminal: PROG leads a
new session (`pty.fork`). Its standard input and output are the terminal; its
standard error goes to the file ERR. Everything the terminal is sent is
written to the file OUT once PROG has exited, and its exit status is then
appended to ERR as "exit N" (N negative for a signal).

A session leader's process group has no parent outside it in the session, so
it is orphaned, and the kernel discards the stop that `SIGTSTP` would cause:
a program that suspends itself here goes straight on, which is what makes a
run of curses' suspend-and-resume deterministic.

Exit status: 0 when PROG ran (whatever it returned), 1 on a usage or I/O
error.
"""

import fcntl
import os
import pty
import struct
import sys
import termios


def main(argv):
    if len(argv) < 6:
        sys.stderr.write("usage: curses-ptyrun.py ROWS COLS OUT ERR PROG [ARG...]\n")
        return 1
    rows, cols = int(argv[1]), int(argv[2])
    out_path, err_path, prog = argv[3], argv[4], argv[5:]
    with open(err_path, "wb") as err:
        pid, master = pty.fork()
        if pid == 0:
            try:
                fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
                os.dup2(err.fileno(), 2)
                os.execv(prog[0], prog)
            finally:
                os._exit(127)
        chunks = []
        while True:
            try:
                data = os.read(master, 65536)
            except OSError:
                # EIO: the terminal's last holder has closed it.
                break
            if not data:
                break
            chunks.append(data)
        _, status = os.waitpid(pid, 0)
        os.close(master)
    with open(out_path, "wb") as out:
        out.write(b"".join(chunks))
    with open(err_path, "ab") as err:
        err.write(b"exit %d\n" % os.waitstatus_to_exitcode(status))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
