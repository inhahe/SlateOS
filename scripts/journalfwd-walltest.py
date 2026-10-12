#!/usr/bin/env python3
"""Run one `wall` implementation against a utmp of the harness's making, and
report what each terminal received -- `scripts/journalfwd-diff.sh`'s helper.

It must run as root in private mount and UTS namespaces (`unshare -rmu`): it
mounts a tmpfs over /run, which puts the system's utmp and logind sessions
out of reach -- so nothing is ever broadcast to a real terminal -- and writes
its own utmp at /run/utmp; and it may set the host name.

Usage: journalfwd-walltest.py SHAPE HOSTNAME|- PROGRAM [ARGS...]

SHAPE names the utmp to build:

  basic    one user's session on the first terminal
  mixed    every kind of entry systemd skips or writes, in one file: a dead
           session, an empty user, a login prompt, a line given as a
           /dev/ path, a line that is not a terminal (/dev/null), one that
           does not exist, and a remote session
  notty    only the line that is not a terminal
  missing  no utmp file at all
  empty    a utmp file with no entries

Prints what each of its three pseudo-terminals received -- the clock time in
the banner replaced by HH:MM:SS, the weekday, date and zone kept -- then the
program's standard output, standard error and status.
"""
import os
import re
import select
import socket
import struct
import subprocess
import sys

USER_PROCESS = 7
LOGIN_PROCESS = 6
DEAD_PROCESS = 8


def record(ut_type, line, user, pid=1, addr=(0, 0, 0, 0), host=b""):
    """One glibc x86-64 `struct utmp`: 384 bytes."""
    rec = bytearray(384)
    struct.pack_into("<h", rec, 0, ut_type)
    struct.pack_into("<i", rec, 4, pid)
    rec[8:8 + min(len(line), 32)] = line[:32]
    rec[44:44 + min(len(user), 32)] = user[:32]
    rec[76:76 + min(len(host), 256)] = host[:256]
    struct.pack_into("<4I", rec, 348, *addr)
    return bytes(rec)


def utmp(shape, names):
    """The records for SHAPE, or None for no file at all."""
    short = [n[len("/dev/"):].encode() for n in names]
    if shape == "basic":
        return [record(USER_PROCESS, short[0], b"alice")]
    if shape == "mixed":
        return [
            record(USER_PROCESS, short[0], b"alice"),
            record(DEAD_PROCESS, short[1], b"bob"),
            record(USER_PROCESS, short[1], b""),
            record(LOGIN_PROCESS, short[2], b"LOGIN"),
            record(USER_PROCESS, names[2].encode(), b"carol"),
            record(USER_PROCESS, b"null", b"dave"),
            record(USER_PROCESS, b"nosuchtty", b"erin"),
            record(USER_PROCESS, short[0], b"remote", addr=(0x0100007F, 0, 0, 0), host=b"far"),
        ]
    if shape == "notty":
        return [record(USER_PROCESS, b"null", b"dave")]
    if shape == "missing":
        return None
    if shape == "empty":
        return []
    raise SystemExit(f"unknown shape {shape}")


def main():
    shape, host, prog = sys.argv[1], sys.argv[2], sys.argv[3:]
    subprocess.run(["mount", "-t", "tmpfs", "tmpfs", "/run"], check=True)
    if host != "-":
        socket.sethostname(host)
    ptys = []
    for _ in range(3):
        m, s = os.openpty()
        ptys.append((m, s, os.ttyname(s)))
    recs = utmp(shape, [p[2] for p in ptys])
    if recs is not None:
        with open("/run/utmp", "wb") as fh:
            fh.write(b"".join(recs))
    out = subprocess.run(prog, stdin=subprocess.DEVNULL, capture_output=True, timeout=30, check=False)
    for i, (m, _s, _name) in enumerate(ptys):
        data = b""
        while select.select([m], [], [], 0.2)[0]:
            chunk = os.read(m, 65536)
            if not chunk:
                break
            data += chunk
        data = re.sub(rb"(\(\w{3} \d{4}-\d\d-\d\d )\d\d:\d\d:\d\d", rb"\1HH:MM:SS", data)
        print(f"pty{i}: {data!r}")
    print("stdout:", out.stdout)
    print("stderr:", out.stderr)
    print("status:", out.returncode)


if __name__ == "__main__":
    main()
