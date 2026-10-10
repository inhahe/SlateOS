#!/usr/bin/env python3
"""Run tload and capture the frames it draws, for scripts/tload-diff.sh.

Usage:
    tload-frames.py [--deadline SECS] pipe FRAMES -- PROGRAM [ARGS...]
    tload-frames.py [--deadline SECS] pty ROWS COLS FRAMES [AFTER ROWS2 COLS2] -- PROGRAM [ARGS...]

`pipe`: PROGRAM writes to a pipe, which has no window size, so each frame
is `ESC [ H` and 25 * 80 - 1 bytes.

`pty`: PROGRAM is given a pseudo-terminal of ROWS x COLS as its last
argument -- tload's [tty] operand -- and draws there. With AFTER, once that
many frames have been read the terminal is made ROWS2 x COLS2 and PROGRAM
is sent SIGWINCH; the rest are read at the new size. The terminal's size
is set before the signal, so the program finds it when it asks.

FRAMES frames are read, then PROGRAM -- the one process this started -- is
killed. Standard output gets what was drawn, byte for byte, then a last
line, `--status: N` for an exit before the frames were all read or
`--status: killed`. PROGRAM's standard error is this program's own.

`--deadline` bounds the wait for the frames (90 seconds by default): a
program that draws nothing -- upstream's tload, given a load or a scale it
cannot draw, spins without drawing -- is killed when it runs out.
"""
import fcntl
import os
import select
import signal
import struct
import sys
import termios
import time

DEADLINE_SECS = 90
HOME = b"\x1b[H"


def set_size(fd: int, rows: int, cols: int) -> None:
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))


def frame_len(rows: int, cols: int) -> int:
    return len(HOME) + rows * cols - 1


def main(argv: list[str]) -> int:
    if "--" not in argv:
        print(__doc__, file=sys.stderr)
        return 2
    sep = argv.index("--")
    head, prog = argv[1:sep], argv[sep + 1:]
    deadline_secs = DEADLINE_SECS
    if head[:1] == ["--deadline"] and len(head) >= 2:
        deadline_secs = float(head[1])
        head = head[2:]
    if not head or not prog:
        print(__doc__, file=sys.stderr)
        return 2
    mode, nums = head[0], [int(x) for x in head[1:]]

    if mode == "pipe":
        (frames,) = nums
        sizes = [(25, 80)]
        rd, wr = os.pipe()
        keep_open = None
    elif mode == "pty":
        rows, cols, frames = nums[0], nums[1], nums[2]
        sizes = [(rows, cols)]
        after = nums[3] if len(nums) >= 6 else None
        if after is not None:
            sizes.append((nums[4], nums[5]))
        rd, wr = os.openpty()  # rd: the master, read from; wr: the slave
        set_size(rd, rows, cols)
        prog = prog + [os.ttyname(wr)]
        # Held open here for the whole run, so that reading the master never
        # ends in a hang-up before the program has opened the terminal.
        keep_open = wr
    else:
        print(f"tload-frames.py: unknown mode {mode!r}", file=sys.stderr)
        return 2

    pid = os.fork()
    if pid == 0:
        os.close(rd)
        if mode == "pipe":
            os.dup2(wr, 1)
            os.close(wr)
        else:
            os.close(wr)
        try:
            os.execvp(prog[0], prog)
        finally:
            os._exit(127)
    if keep_open is None:
        os.close(wr)

    got = bytearray()
    status = None
    deadline = time.monotonic() + deadline_secs
    target = frame_len(*sizes[0]) * (frames if after_of(mode, nums) is None else after_of(mode, nums))

    def pump(want: int) -> bool:
        nonlocal status
        while len(got) < want and time.monotonic() < deadline:
            r, _, _ = select.select([rd], [], [], 0.05)
            if rd in r:
                try:
                    chunk = os.read(rd, 65536)
                except OSError:
                    chunk = b""
                if chunk:
                    got.extend(chunk)
                    continue
            done, st = os.waitpid(pid, os.WNOHANG)
            if done:
                status = st
                # Whatever it wrote before it went.
                while True:
                    r, _, _ = select.select([rd], [], [], 0.05)
                    if rd not in r:
                        break
                    try:
                        chunk = os.read(rd, 65536)
                    except OSError:
                        break
                    if not chunk:
                        break
                    got.extend(chunk)
                return False
        return len(got) >= want

    ok = pump(target)
    if ok and len(sizes) > 1:
        rows2, cols2 = sizes[1]
        set_size(rd, rows2, cols2)
        os.kill(pid, signal.SIGWINCH)
        rest = frames - after_of(mode, nums)
        pump(target + frame_len(rows2, cols2) * rest)

    if status is None:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
        report = "killed"
    elif os.WIFEXITED(status):
        report = str(os.WEXITSTATUS(status))
    else:
        report = f"signal {os.WTERMSIG(status)}"
    sys.stdout.buffer.write(bytes(got))
    sys.stdout.buffer.write(f"\n--status: {report}\n".encode())
    return 0


def after_of(mode: str, nums: list[int]):
    return nums[3] if mode == "pty" and len(nums) >= 6 else None


if __name__ == "__main__":
    sys.exit(main(sys.argv))
