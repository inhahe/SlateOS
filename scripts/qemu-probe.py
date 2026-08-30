#!/usr/bin/env python3
"""Ask a throwaway QEMU a question, and always get the process back.

    python scripts/qemu-probe.py --device ati-vga,model=rv100 -- "info pci"
    python scripts/qemu-probe.py --device ati-vga,model=xyzzy      # what does a bad model do?
    python scripts/qemu-probe.py -- "info qtree"

Why this exists
---------------
"Does this QEMU support that device, and what do its BARs look like?" is a
one-liner you reach for mid-task.  The obvious spelling of it --

    printf 'info pci\\nquit\\n' | qemu-system-x86_64 -monitor stdio ...

leaks a QEMU process **about two thirds of the time** (measured: 4 wedges in
6 runs; see below).  Four such orphans were found on this host alive 16-21
hours after the fact, one of them having burned 729 seconds of CPU -- on the
same machine the benchmark suite measures on, where a background spinner is
exactly the contamination `bench-history.py` spends its effort trying to
detect.  Under MSYS they are not even easy to kill: `kill "$!"` uses the
Cygwin PID and does not reliably TerminateProcess a native Windows
`qemu-system-x86_64.exe`.

The mechanism (measured, not guessed)
-------------------------------------
It is a **race on stdin EOF**, which is why it is intermittent and why it
survives "but I did send `quit`".  A pipeline like the one above hands QEMU
the bytes and then immediately closes the write end.  QEMU's stdio monitor
sees the commands and the EOF at essentially the same moment, and if the EOF
wins, the chardev is torn down with the queued `quit` still unexecuted.  The
monitor is now gone, nothing else will ever ask the VM to stop, and QEMU runs
until the machine reboots.

    stdin closed immediately (the naive idiom):  4 of 6 runs wedged
    stdin held open, `quit` sent after a settle: 0 of 4 runs wedged

So this script never closes stdin before the guest has exited.  It writes the
commands, waits `--settle` seconds, writes `quit`, and only then waits for the
process -- keeping the pipe open throughout so the EOF can never overtake the
command.

Belt and braces
---------------
The settle is what makes the *common* case exit cleanly in ~0.5s; it is not
what makes the guarantee.  The guarantee is `proctree.Tree`: QEMU runs inside
a Job Object with `KILL_ON_JOB_CLOSE`, so if it ignores `quit` anyway, hangs
in firmware, or spawns children, the deadline tears the whole tree down -- and
so does this script dying for any reason, including Ctrl-C.  That is the part
`timeout` cannot do (it kills only the direct child).

The failure mode is therefore a non-zero exit and a message, never a survivor.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import proctree  # noqa: E402  (needs the path above)

# Same candidates, in the same order, as scripts/boot-test.sh:1243.
QEMU_CANDIDATES = (
    "qemu-system-x86_64",
    "/c/Program Files/qemu/qemu-system-x86_64.exe",
    "C:/Program Files/qemu/qemu-system-x86_64.exe",
)

DRAIN_GRACE = 5.0  # seconds to collect output after the process is gone

# Matches a CSI sequence: ESC '[' <params> <final byte>.
CSI_RE = re.compile(r"\x1b\[([0-9;]*)([A-Za-z])")


def render_terminal(text: str) -> str:
    """Replay the monitor's terminal control codes into plain text.

    QEMU's monitor thinks it is talking to a terminal even when stdout is a
    pipe, so it echoes each command *character by character*, rewinding with
    CSI D (cursor-left) and CSI K (erase-to-end-of-line) as it goes.  Raw,
    `info version` arrives as

        i^[[K^[[Din^[[K^[[D^[[Dinf^[[K...

    which is unreadable and -- more importantly -- ungreppable, so a caller
    cannot pick a BAR address out of a probe.  Replaying the codes against a
    line buffer collapses that back to what a human would have seen.
    """
    lines: list[str] = []
    line: list[str] = []
    col = 0
    i = 0
    while i < len(text):
        m = CSI_RE.match(text, i)
        if m:
            params, final = m.group(1), m.group(2)
            try:
                n = int(params) if params else 1
            except ValueError:
                n = 1
            if final == "D":
                col = max(0, col - n)
            elif final == "C":
                col = min(len(line), col + n)
            elif final == "K":
                # 0/absent = erase to end of line (the only variant QEMU uses).
                if params in ("", "0"):
                    del line[col:]
                elif params == "1":
                    line[:col] = [" "] * col
                else:
                    line.clear()
                    col = 0
            i = m.end()
            continue
        ch = text[i]
        i += 1
        if ch == "\n":
            lines.append("".join(line).rstrip())
            line, col = [], 0
        elif ch == "\r":
            col = 0
        elif ch == "\b":
            col = max(0, col - 1)
        elif ch == "\x1b":
            pass  # a stray/unrecognised escape introducer: drop it
        else:
            if col < len(line):
                line[col] = ch
            else:
                line.append(ch)
            col += 1
    if line:
        lines.append("".join(line).rstrip())
    return "\n".join(lines) + "\n"


def find_qemu() -> str:
    for cand in QEMU_CANDIDATES:
        if "/" not in cand and os.sep not in cand:
            found = shutil.which(cand)
            if found:
                return found
        elif os.path.exists(cand):
            return cand
    sys.exit(
        "error: qemu-system-x86_64 not found (looked on PATH and in "
        "'C:/Program Files/qemu')"
    )


def main() -> None:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument(
        "--device",
        action="append",
        default=[],
        metavar="SPEC",
        help="add '-device SPEC' (repeatable)",
    )
    ap.add_argument("--machine", default="q35", help="machine type (default q35)")
    ap.add_argument("--memory", default="512M", help="guest RAM (default 512M)")
    ap.add_argument(
        "--timeout",
        type=float,
        default=30.0,
        help="hard deadline in seconds; the whole tree is killed on expiry "
        "(default 30)",
    )
    ap.add_argument(
        "--settle",
        type=float,
        default=0.4,
        help="seconds between the commands and 'quit', so the monitor has "
        "executed them before it is asked to exit (default 0.4)",
    )
    ap.add_argument(
        "--qemu-arg",
        action="append",
        default=[],
        metavar="ARG",
        help="pass an extra raw argument to QEMU (repeatable)",
    )
    ap.add_argument(
        "--raw",
        action="store_true",
        help="print QEMU's bytes verbatim instead of replaying its terminal "
        "control codes (for debugging this script itself)",
    )
    ap.add_argument(
        "monitor",
        nargs="*",
        help="monitor commands to run (default: 'info pci'). 'quit' is always "
        "appended for you -- do not pass it.",
    )
    args = ap.parse_args()

    cmds = args.monitor or ["info pci"]

    cmd = [
        find_qemu(),
        "-machine",
        args.machine,
        "-m",
        args.memory,
        "-display",
        "none",
        "-nodefaults",
        "-monitor",
        "stdio",
    ]
    for dev in args.device:
        cmd += ["-device", dev]
    cmd += args.qemu_arg

    out_chunks: list[bytes] = []
    err_chunks: list[bytes] = []
    timed_out = False

    with proctree.Tree(
        cmd,
        warn=lambda m: print(f"[qemu-probe] warning: {m}", file=sys.stderr),
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ) as tree:
        proc = tree.proc

        # Drain on threads: we cannot use communicate(), because it closes
        # stdin -- the very thing that causes the leak this script exists to
        # prevent.
        def drain(stream, sink):
            try:
                sink.append(stream.read())
            except (OSError, ValueError):
                pass

        readers = [
            threading.Thread(target=drain, args=(proc.stdout, out_chunks), daemon=True),
            threading.Thread(target=drain, args=(proc.stderr, err_chunks), daemon=True),
        ]
        for t in readers:
            t.start()

        try:
            # The leading newline draws out the first monitor prompt; without
            # it the first command can be swallowed with the banner and the
            # probe silently reports nothing.
            proc.stdin.write(("\n" + "\n".join(cmds) + "\n").encode())
            proc.stdin.flush()
            time.sleep(args.settle)
            proc.stdin.write(b"quit\n")
            proc.stdin.flush()
        except OSError:
            pass  # guest already gone; the wait below reports the truth

        deadline = time.time() + args.timeout
        while proc.poll() is None and time.time() < deadline:
            time.sleep(0.05)

        if proc.poll() is None:
            timed_out = True
            tree.kill()  # Job Object teardown: guest + any descendants
            proc.wait(timeout=DRAIN_GRACE)

        # Safe now: the process is gone, so closing stdin cannot race a
        # pending command, and the readers are at EOF.
        try:
            proc.stdin.close()
        except OSError:
            pass
        for t in readers:
            t.join(DRAIN_GRACE)

        returncode = proc.returncode

    raw_out = b"".join(out_chunks).decode("utf-8", "replace")
    sys.stdout.write(raw_out if args.raw else render_terminal(raw_out))
    err = b"".join(err_chunks).decode("utf-8", "replace")
    if err.strip():
        sys.stderr.write(err if args.raw else render_terminal(err))
    sys.stdout.flush()

    if timed_out:
        sys.exit(
            f"\n[qemu-probe] error: qemu did not exit within {args.timeout:g}s. "
            f"The process tree was killed, so nothing leaked. A device that "
            f"makes QEMU spin (bad -device spelling, firmware retry loop) does "
            f"this; try --qemu-arg=-no-reboot or a longer --settle."
        )
    sys.exit(returncode or 0)


if __name__ == "__main__":
    main()
