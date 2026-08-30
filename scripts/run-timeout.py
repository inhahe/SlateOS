#!/usr/bin/env python3
"""run-timeout.py — run a command with a hard timeout and guaranteed
process-tree cleanup.

The motivating problem: wrapping `cargo test` in coreutils `timeout` only
kills `cargo` itself; the spawned test binaries survive as orphans and can
linger for hours (a deadlocked test never exits on its own). The fix is
`proctree.Tree`, which holds the child and every descendant it spawns in one
killable unit — a Job Object on Windows, a process group on POSIX — so a
timeout, a Ctrl-C, or this runner's own death tears down the whole tree
atomically and nothing is ever orphaned. See `proctree.py` for the mechanism.

This module is the *streaming* front end to that: the child keeps this
process's stdout and stderr, so its output appears live. For the batch case —
feed stdin, capture the output, enforce a deadline — call
`proctree.run_captured` directly rather than `subprocess.run(timeout=…)`,
whose timeout can silently become infinite (again, see `proctree.py`).

While the child runs, the runner prints a heartbeat line every
`--poll` seconds ("[run-timeout] still running, Ns elapsed") so a long
build never looks like a silent hang, and it reports a clear final status.

**Do not pipe this into `tail`, `head`, `grep` or anything else when you are
backgrounding it.** Those buffer, so nothing reaches the output file until the
command *finishes* -- which turns the heartbeat into exactly the silence it
exists to prevent, and makes a perfectly healthy 20-minute build look wedged.
The temptation is then to kill and restart it, throwing away the real work it
had done. Redirect the whole stream and narrow it when you *read* it, not as it
is produced. (Observed: a `--bench` boot test backgrounded through `| tail -40`
sat at zero bytes for five minutes with `rustc` visibly alive at 2.4 GB.)

Usage:
    python run-timeout.py [--poll SECS] <timeout_secs> <command> [args...]

Exit codes:
    <child code>  child completed on its own (its exit code is passed through)
    124           timed out — the whole process tree was killed
    125           failed to launch the child
    130           interrupted (Ctrl-C) — tree killed
"""

import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import proctree  # noqa: E402  (needs the path above)

POLL_DEFAULT = 15.0
EXIT_TIMEOUT = 124
EXIT_LAUNCH = 125
EXIT_INTERRUPT = 130

IS_WINDOWS = proctree.IS_WINDOWS


def _log(msg: str) -> None:
    print(f"[run-timeout] {msg}", flush=True)


def main(argv: list[str]) -> int:
    poll = POLL_DEFAULT
    args = argv[1:]
    if args and args[0] == "--poll":
        if len(args) < 2:
            _log("--poll requires a value")
            return EXIT_LAUNCH
        try:
            poll = float(args[1])
        except ValueError:
            _log(f"invalid --poll value: {args[1]!r}")
            return EXIT_LAUNCH
        args = args[2:]

    if len(args) < 2:
        _log("usage: run-timeout.py [--poll SECS] <timeout_secs> <command> [args...]")
        return EXIT_LAUNCH

    try:
        timeout = float(args[0])
    except ValueError:
        _log(f"invalid timeout: {args[0]!r}")
        return EXIT_LAUNCH
    command = args[1:]

    _log(f"launching (timeout {timeout:g}s): {' '.join(command)}")
    try:
        tree = proctree.Tree(command, warn=lambda m: _log(f"warning: {m}"))
    except OSError as e:
        _log(f"failed to launch: {e}")
        return EXIT_LAUNCH

    start = time.monotonic()
    deadline = start + timeout
    stop_heartbeat = threading.Event()

    def _heartbeat() -> None:
        while not stop_heartbeat.wait(poll):
            elapsed = time.monotonic() - start
            if elapsed >= timeout:
                return
            _log(f"still running, {elapsed:.0f}s elapsed")

    hb = threading.Thread(target=_heartbeat, daemon=True)
    hb.start()

    # The `with` is the safety net: however this block is left — return, raise,
    # or an interrupt that arrives between statements — the job handle closes
    # and the tree goes with it.
    with tree:
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    stop_heartbeat.set()
                    _log(f"TIMEOUT after {timeout:g}s -- killing process tree")
                    tree.kill()
                    try:
                        tree.proc.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        _log("warning: tree did not exit within 10s of kill")
                    return EXIT_TIMEOUT
                try:
                    code = tree.proc.wait(timeout=min(remaining, 1.0))
                except subprocess.TimeoutExpired:
                    continue
                stop_heartbeat.set()
                elapsed = time.monotonic() - start
                status = "PASS" if code == 0 else f"FAIL (exit {code})"
                _log(f"child exited: {status}, {elapsed:.0f}s elapsed")
                return code
        except KeyboardInterrupt:
            stop_heartbeat.set()
            _log("interrupted -- killing process tree")
            tree.kill()
            return EXIT_INTERRUPT


if __name__ == "__main__":
    sys.exit(main(sys.argv))
