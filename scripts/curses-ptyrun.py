#!/usr/bin/env python3
"""Run a program on a pseudo-terminal of its own, for curses-diff.sh.

Usage: curses-ptyrun.py [--then ACTION]... ROWS COLS OUT ERR PROG [ARG...]

The terminal is ROWS by COLS and is PROG's controlling terminal: PROG leads a
new session (`pty.fork`). Its standard input and output are the terminal; its
standard error goes to the file ERR. Everything the terminal is sent is
written to the file OUT once PROG has exited, and its exit status is then
appended to ERR as "exit N" (N negative for a signal).

PROG is found on `PATH` when it has no slash, so that its `argv[0]` can be
the bare name.

Each `--then` STEP is done in turn once PROG has been quiet for a moment
-- nothing written since the last thing it wrote or the last step, a frame
drawn and the program waiting -- so that what it does is not raced against
what it draws; the first waits for PROG to have written something at all.
(A step that makes PROG draw nothing new is then followed by the next all
the same.) A step is one or more actions, separated by `;`, done together:

  keys:TEXT     TEXT typed at the terminal, with \\n, \\t, \\e, \\\\ and \\xHH
                (\\x3b for a `;`)
  resize:RxC    the terminal made R rows by C columns (the kernel then
                sends the foreground group SIGWINCH)
  signal:NAME   the signal sent to PROG (INT, TERM, HUP, WINCH, ...)
  copy:SRC:DST  the file DST given SRC's contents, in place -- a file PROG
                rereads changing under it

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
import select
import signal
import struct
import sys
import termios
import time

# How long the program must have been quiet before the next action.
QUIET = 0.3


def unescape(text):
    """The escapes of a keys: action, as bytes."""
    out = bytearray()
    raw = text.encode("utf-8", "surrogateescape")
    i = 0
    while i < len(raw):
        c = raw[i]
        if c != 0x5C or i + 1 >= len(raw):
            out.append(c)
            i += 1
            continue
        nxt = raw[i + 1]
        simple = {0x6E: 0x0A, 0x74: 0x09, 0x65: 0x1B, 0x5C: 0x5C}
        if nxt in simple:
            out.append(simple[nxt])
            i += 2
        elif nxt == 0x78 and i + 3 < len(raw):
            # Two hex digits follow the x.
            out.append(int(raw[i + 2 : i + 4].decode("ascii"), 16))
            i += 4
        else:
            out.append(c)
            i += 1
    return bytes(out)


def act(step, pid, master):
    """Do one --then step's actions."""
    for action in step.split(";"):
        kind, _, arg = action.partition(":")
        if kind == "keys":
            os.write(master, unescape(arg))
        elif kind == "resize":
            rows, cols = (int(v) for v in arg.split("x"))
            fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        elif kind == "signal":
            os.kill(pid, getattr(signal, "SIG" + arg))
        elif kind == "copy":
            src, _, dst = arg.partition(":")
            with open(src, "rb") as f:
                data = f.read()
            # "wb" truncates the file it opens rather than replacing it, so
            # a bind mount of DST still shows it.
            with open(dst, "wb") as f:
                f.write(data)
        else:
            raise ValueError("unknown action: " + action)


def main(argv):
    actions = []
    args = argv[1:]
    while len(args) >= 2 and args[0] == "--then":
        actions.append(args[1])
        args = args[2:]
    if len(args) < 5:
        sys.stderr.write(__doc__.split("\n\n")[1] + "\n")
        return 1
    rows, cols = int(args[0]), int(args[1])
    out_path, err_path, prog = args[2], args[3], args[4:]
    with open(err_path, "wb") as err:
        pid, master = pty.fork()
        if pid == 0:
            try:
                fcntl.ioctl(1, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
                os.dup2(err.fileno(), 2)
                os.execvp(prog[0], prog)
            finally:
                os._exit(127)
        chunks = []
        last_event = None
        while True:
            timeout = None
            if actions and last_event is not None:
                timeout = max(0.0, QUIET - (time.monotonic() - last_event))
            ready, _, _ = select.select([master], [], [], timeout)
            if not ready:
                act(actions.pop(0), pid, master)
                last_event = time.monotonic()
                continue
            try:
                data = os.read(master, 65536)
            except OSError:
                # EIO: the terminal's last holder has closed it.
                break
            if not data:
                break
            chunks.append(data)
            last_event = time.monotonic()
        _, status = os.waitpid(pid, 0)
        os.close(master)
    with open(out_path, "wb") as out:
        out.write(b"".join(chunks))
    with open(err_path, "ab") as err:
        err.write(b"exit %d\n" % os.waitstatus_to_exitcode(status))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
