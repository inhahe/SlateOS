#!/usr/bin/env python3
"""Tests for `guest.py`, the host's end of the guest agent's protocol.

Run: `python scripts/test-guest.py` (0 = pass, 1 = fail).

The agent itself is tested inside the kernel (`guestagent::self_test`); this
tests the other end against a fake agent on a local socket, so it needs no
QEMU and no guest: requests framed as the agent reads them (the line, then the
fields its numbers measure, any byte in a field), each reply read back whole
-- its field included, by length -- an `err` raised as the agent's refusal,
and the exit codes `run`, `try` and `sh` hand on. Nothing here starts or stops
a process.
"""

from __future__ import annotations

import importlib.util
import os
import socket
import sys
import tempfile
import threading

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("guest", os.path.join(HERE, "guest.py"))
guest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guest)

failures: list[str] = []


def check(what: str, got, want) -> None:
    if got == want:
        print(f"  ok   {what}")
    else:
        print(f"  FAIL {what}: got {got!r}, want {want!r}")
        failures.append(what)


class FakeAgent:
    """One connection at a time: reads a request as the agent does, records
    it, and answers with the next canned reply."""

    def __init__(self, replies: list[bytes]):
        self.replies = list(replies)
        self.requests: list[tuple[str, list[str], list[bytes]]] = []
        self.server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.server.bind(("127.0.0.1", 0))
        self.server.listen(4)
        self.port = self.server.getsockname()[1]
        self.thread = threading.Thread(target=self.serve, daemon=True)
        self.thread.start()

    def serve(self) -> None:
        while self.replies:
            try:
                conn, _ = self.server.accept()
            except OSError:
                return
            with conn:
                line = guest.read_line(conn)
                words = line.decode("ascii").split(" ")
                verb, numbers = words[0], words[1:]
                lengths = {
                    "ping": [],
                    "put": numbers[1:],
                    "get": numbers,
                    "sh": numbers,
                    "run": numbers[1:2] + numbers[3:],
                }[verb]
                fields = [guest.read_exact(conn, int(n)) for n in lengths]
                self.requests.append((verb, numbers, fields))
                conn.sendall(self.replies.pop(0))

    def close(self) -> None:
        self.server.close()


def main() -> int:
    print("framing")
    check("a put's line and fields",
          guest.encode("put", ["644", 2, 3], [b"/a", b"\n\xff\n"]),
          b"put 644 2 3\n/a\n\xff\n")
    check("a ping has no fields", guest.encode("ping", [], []), b"ping\n")
    check("pong has no field", guest.parse_reply_line(b"pong 1"), ("pong", [1], 0))
    check("ok's last number is its field", guest.parse_reply_line(b"ok 5"), ("ok", [5], 5))
    check("exit: the code, then the field", guest.parse_reply_line(b"exit -3 7"), ("exit", [-3, 7], 7))
    for bad in (b"ok", b"ok -1", b"ok x"):
        try:
            guest.parse_reply_line(bad)
            check(f"{bad!r} is refused", "accepted", "refused")
        except (ConnectionError, ValueError):
            check(f"{bad!r} is refused", "refused", "refused")

    print("against a fake agent")
    agent = FakeAgent([
        b"pong 1\n",
        b"ok 0\n",
        b"ok 4\n\x00\n\xffz",
        b"exit 3 7\nout\nerr",
        b"timeout 2\n..",
        b"ok 1 5\nfail\n",
        b"err 12\nno such file",
    ])
    try:
        port = agent.port
        check("ping", guest.ping(port), True)

        with tempfile.TemporaryDirectory() as tmp:
            local = os.path.join(tmp, "prog")
            with open(local, "wb") as f:
                f.write(b"\x7fELF\x00bytes")
            guest.put(port, local, "/tmp/prog", "755")
        check("put sends the mode in octal, the path and the data",
              agent.requests[-1], ("put", ["755", "9", "10"], [b"/tmp/prog", b"\x7fELF\x00bytes"]))

        check("get hands back the field whole, any byte in it",
              guest.get(port, "/etc/x"), b"\x00\n\xffz")

        code, output = guest.run(port, 30, "file", ["/tmp/prog", "-v"])
        check("run: the program's exit code and its output", (code, output), (3, b"out\nerr"))
        check("run: seconds, grants and each argument framed",
              agent.requests[-1], ("run", ["30", "4", "2", "9", "2"], [b"file", b"/tmp/prog", b"-v"]))

        check("a run out of time is 124, as timeout's", guest.run(port, 1, "-", ["/x"]), (124, b".."))

        check("sh: the status and the output", guest.shell(port, "false"), (1, b"fail\n"))

        try:
            guest.get(port, "/nope")
            check("err is raised as the agent's refusal", "returned", "raised")
        except guest.AgentError as e:
            check("err is raised as the agent's refusal", str(e), "no such file")
    finally:
        agent.close()

    check("no answer is no pong", guest.ping(1, wait=0.5), False)

    print()
    if failures:
        print(f"{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("all guest tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
