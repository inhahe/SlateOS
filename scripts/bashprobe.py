#!/usr/bin/env python3
"""Faithful transport to real bash, plus an arity-exact word probe.

The oracle behind `check-shellquote-vs-bash.py`,
`check-kshell-pipeline-vs-bash.py` and `check-kshell-rungs-vs-bash.py`.
kshell's quoting rules are not ours to invent -- they are bash's -- so the
only way to know whether a rule written into `kernel/src/shellquote.rs` is
right is to ask bash.  This module is the part that makes the asking
trustworthy.

**Requires WSL** (`wsl -d Ubuntu`), which is why none of the four is wired
into `scripts/boot-test.sh`: the boot test has to run on a host carrying only
the Rust toolchain and QEMU.  They are run by hand when a quoting rule is
written or changed; their verdicts are then carried forward into the kernel's
own self-test rungs, which is what makes the evidence survive onto a host
that has no bash at all.

Two things this fixes over the naive `bash -c <script>` in an argv element:

1.  **Transport.**  Passing the script as an argument means it is serialised
    into a Windows command line by `subprocess`, and then re-parsed by
    `wsl.exe` before Linux ever sees it.  Backslashes do not survive that
    round trip intact -- `a\\\\b` arrived as `a\\b`, which silently turns
    every backslash case into a test of a different input than the one
    written down.  Feeding the script on **stdin** to `bash -s` removes both
    layers: the bytes are copied verbatim.

2.  **Arity.**  `printf '%s\\n' $EMPTY` prints one blank line when bash
    passed it *zero* words, because printf reruns its format for at least
    one round.  So the obvious probe cannot tell "no words" from "one empty
    word" -- exactly the distinction kshell's `split_words` gets wrong.
    `set -- <line>` then `$#` and `[%s]` per word is exact.

`assert_transport_is_faithful()` is called before any case runs: if the
bytes are not arriving intact there is no point comparing results, and a
harness bug must never be reported as a bash disagreement.
"""
import subprocess

WSL = ["wsl", "-d", "Ubuntu", "--", "bash", "-s"]


def run(script: bytes):
    """Run `script` under bash with the bytes delivered verbatim."""
    return subprocess.run(WSL, input=script, capture_output=True)


def assert_transport_is_faithful():
    """Prove bytes reach bash unaltered before trusting any comparison."""
    # A *quoted* here-doc delimiter turns off every form of processing, so
    # whatever comes back is exactly what arrived.  It has to be a here-doc
    # rather than a single-quoted string because the probe must contain a
    # `'` -- and a `'` cannot appear inside `'...'` at all.  (Getting that
    # wrong once already made this function report a faithful transport as
    # broken, which is the same class of mistake it exists to catch.)
    probe = rb"""a\b a\\b a\\\b "x" 'y' $z `w` %s ~ {a,b}"""
    r = run(b"cat <<'PROBE_EOF'\n" + probe + b"\nPROBE_EOF\n")
    if r.returncode != 0 or r.stdout != probe + b"\n":
        raise SystemExit(
            "TRANSPORT IS NOT FAITHFUL -- every result below would be a lie.\n"
            f"  sent: {probe!r}\n"
            f"  back: {r.stdout!r}\n"
            f"  err : {r.stderr!r}"
        )


def words(line: str, setup: str = "HOME=/root; USER=root; EMPTY=''"):
    """The exact word list bash produces for `line`, or None if bash errored.

    Uses `set --` so the count is bash's own, then prints each word wrapped
    in a byte that cannot be confused with a separator.  Returns a list of
    `bytes` whose *length* is authoritative, including the empty list.
    """
    script = (
        f"{setup}\n"
        f"set -- {line}\n"
        'printf "%s\\n" "$#"\n'
        'for w in "$@"; do printf "[%s]\\n" "$w"; done\n'
    ).encode()
    r = run(script)
    if r.returncode != 0:
        return None
    lines = r.stdout.split(b"\n")
    try:
        n = int(lines[0])
    except (ValueError, IndexError):
        return None
    out = []
    for raw in lines[1 : 1 + n]:
        if not (raw.startswith(b"[") and raw.endswith(b"]")):
            return None
        out.append(raw[1:-1])
    if len(out) != n:
        return None
    return out
