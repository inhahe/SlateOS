#!/usr/bin/env python3
"""Run a program with a pseudo-terminal as its standard input.

For scripts/pidof-diff.sh. Usage: pidof-ttyrun.py OUT ERR PROGRAM [ARGS...]

PROGRAM's standard input is the terminal, its standard output OUT and its
standard error ERR; this exits with its status, 128 + N when signal N ended
it. pidof and killall5 send what they have to say to syslog unless standard
input is a terminal, which is the case this makes.
"""
import os
import sys


def main(argv: list[str]) -> int:
    if len(argv) < 4:
        print(__doc__, file=sys.stderr)
        return 2
    out, err, prog = argv[1], argv[2], argv[3:]
    master, slave = os.openpty()
    pid = os.fork()
    if pid == 0:
        os.close(master)
        os.dup2(slave, 0)
        os.close(slave)
        flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
        os.dup2(os.open(out, flags, 0o644), 1)
        os.dup2(os.open(err, flags, 0o644), 2)
        try:
            os.execvp(prog[0], prog)
        finally:
            os._exit(127)
    os.close(slave)
    _, status = os.waitpid(pid, 0)
    os.close(master)
    if os.WIFSIGNALED(status):
        return 128 + os.WTERMSIG(status)
    return os.WEXITSTATUS(status)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
