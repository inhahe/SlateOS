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
    _assert_word_probe_is_exact()


# (line, expected words) -- the three properties `words()` must have.  These
# are deliberately cases whose answer is not in doubt, so a failure here is
# unambiguously a harness bug and never a fact about bash.
_WORD_PROBE_SELF_TEST = [
    # Byte-exactness through the separator.  This is the one that was broken:
    # a newline *inside* a word used to be indistinguishable from the
    # separator, so a word bash built correctly came back as None and every
    # caller printed it as `<bash error>`.
    (r"$'a\nb'", [b"a\nb"]),
    (r"$'a\nb' c", [b"a\nb", b"c"]),
    # Arity at zero, which `printf` alone cannot express.
    ("$EMPTY", []),
    ('"$EMPTY"', [b""]),
]


def _assert_word_probe_is_exact():
    """Prove `words()` itself before trusting a single one of its answers.

    `assert_transport_is_faithful` used to stop at the here-doc, which proved
    only that bytes reach bash -- not that the *answers* come back intact.
    The subtle half of this harness is the return path, and that is exactly
    where it was wrong for as long as no case happened to contain a newline.
    """
    for line, want in _WORD_PROBE_SELF_TEST:
        got = words(line)
        if got != want:
            raise SystemExit(
                "THE WORD PROBE IS BROKEN -- every result below would be a lie.\n"
                f"  line: {line!r}\n"
                f"  want: {want!r}\n"
                f"  got : {got!r}"
                + ("\n  (None means the probe gave up, not that bash errored.)"
                   if got is None else "")
            )


def words(line: str, setup: str = "HOME=/root; USER=root; EMPTY=''"):
    """The exact word list bash produces for `line`, or None if bash errored.

    Uses `set --` so the count is bash's own, then emits the words separated
    by NUL.  Returns a list of `bytes` whose *length* is authoritative,
    including the empty list.

    **The separator must be NUL, not a newline.**  This function used to print
    each word as `[%s]` on its own line and reject any line not wrapped in
    brackets.  A word *containing* a newline -- `$'a\\nb'`, which is perfectly
    legal and is precisely what ANSI-C quoting is for -- spans two lines, so
    the wrapper check failed and the function returned None.  Every caller
    prints None as `<bash error>`, so a case bash handled correctly was
    reported as a case bash *rejected*.  That is the worst direction for a
    probe to fail in: it does not look like a broken harness, it looks like a
    fact about bash, and it was one step from being written into a kernel
    self-test rung as "bash rejects `$'a\\nb'`".

    NUL is safe as the delimiter precisely because bash cannot put one in a
    word -- a bash string is NUL-terminated, so `$'a\\0b'` yields `a`.  A byte
    that cannot occur in the data is the only kind of separator that needs no
    escaping, which is the same reason `find -print0` exists.

    The count still comes from `$#` and the loop still runs once per word, so
    the zero-word/one-empty-word distinction that `printf` cannot express is
    preserved: a `for` over zero arguments produces no output at all.
    """
    script = (
        f"{setup}\n"
        f"set -- {line}\n"
        'printf "%s\\n" "$#"\n'
        'for w in "$@"; do printf "%s\\0" "$w"; done\n'
    ).encode()
    r = run(script)
    if r.returncode != 0:
        return None
    count, sep, rest = r.stdout.partition(b"\n")
    if not sep:
        return None
    try:
        n = int(count)
    except ValueError:
        return None
    # Every word is NUL-*terminated*, so n words leave a trailing empty field.
    # Checking for it is what catches a truncated or over-long read rather
    # than silently returning a short list.
    fields = rest.split(b"\0")
    if len(fields) != n + 1 or fields[-1] != b"":
        return None
    return fields[:n]
