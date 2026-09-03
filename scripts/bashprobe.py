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

## Three ways to end, and why they must not share an exit code

The sentence above -- "a harness bug must never be reported as a bash
disagreement" -- was true of what this module *printed* and false of what it
*exited with*.  Every failure here used to be `SystemExit(<string>)`, which
is exit 1 with no traceback, and exit 1 is precisely how a gate says "I
looked and the code is wrong".  So a host with no WSL reported the kernel as
disagreeing with bash, in a file whose whole purpose is to keep harness
noise out of the verdict.

The three outcomes are now distinct, because they are three different facts:

| what happened | exit | what it means |
|---|---|---|
| bash ran, cases compared | 0 / 1 | agreement / a real disagreement |
| **bash could not be run at all** | **2** | "I could not look."  Not a pass. |
| bash ran and answered wrongly | traceback | the harness is broken |

Exit 2 is the convention `run_checker --may-skip=2` understands (see
`scripts/run-checker.sh` -> "The fourth outcome"), which is what lets these
four gates be wired into the boot test on a host with no WSL without either
lying about the code or stopping the build.

The split between rows 2 and 3 is drawn at **whether bash exited 0**.  If it
did, bash ran and we failed to read it, which is our bug and must be loud.
If it did not -- no `wsl.exe`, no `Ubuntu` distro, distro will not start --
nothing was measured and there is nothing to report but that.

Row 3 deliberately raises rather than exiting, so the traceback reaches
`run_checker`, which refuses to skip *any* gate whose output contains one
even at a `--may-skip=2` call site.  A broken harness therefore cannot be
waved through by the same channel that exists to wave through a missing one.
"""
import subprocess
import sys

WSL = ["wsl", "-d", "Ubuntu", "--", "bash", "-s"]

#: Exit code for "I could not look", per `run_checker --may-skip=<rc>`.
NO_BASH = 2


class HarnessBroken(RuntimeError):
    """The comparison machinery is wrong, so no result from it means anything.

    Raised, never exited, so the traceback reaches `run_checker` and blocks
    the skip channel.  `ProbeError` is the same class of fact one layer down
    (bash answered a single case unreadably); this one means the harness
    failed its own self-test before any case was scored.
    """


def _no_bash(why: str):
    """Report "I could not look" and exit 2 -- without a traceback.

    No traceback on purpose: this is not a defect, it is the absence of an
    instrument, and a traceback here would both mislead a reader and (by
    `run-checker.sh`'s traceback rule) turn a legitimate skip into an abort.
    """
    print(
        f"NO BASH TO ASK -- {why}\n"
        f"  tried: {' '.join(WSL)}\n"
        "  Nothing was checked. This is NOT a pass: a real disagreement\n"
        "  between kshell and bash would look exactly like what you just saw.\n"
        "  Install WSL with an Ubuntu distro to make this gate able to fail.",
        file=sys.stderr,
    )
    raise SystemExit(NO_BASH)


def run(script: bytes):
    """Run `script` under bash with the bytes delivered verbatim."""
    return subprocess.run(WSL, input=script, capture_output=True)


def _assert_bash_is_reachable():
    """Prove there is a bash to ask before concluding anything about answers.

    Runs the smallest possible script.  Anything that stops bash exiting 0 --
    `wsl.exe` absent (`FileNotFoundError`), the distro missing or refusing to
    start (non-zero exit) -- is "I could not look" and exits 2.

    A *successful* exit carrying the wrong bytes is not handled here: that is
    bash running and the transport lying, which is the caller's job to catch
    and is a harness bug, not a missing instrument.
    """
    try:
        r = run(b"printf 'reachable'\n")
    except OSError as e:
        # FileNotFoundError when wsl.exe is not on PATH; OSError covers the
        # other ways a launch can fail without bash ever having existed.
        _no_bash(f"could not launch it: {e}")
    if r.returncode != 0:
        # wsl.exe writes this in UTF-16LE, which is mojibake as UTF-8 and
        # empty-looking after a naive decode -- so decode it as both and keep
        # whichever produced something, rather than printing a blank reason.
        err = r.stderr or b""
        msg = err.decode("utf-16-le", "replace") if b"\x00" in err[:40] else \
            err.decode("utf-8", "replace")
        _no_bash(
            f"it exited {r.returncode} without running bash: "
            f"{' '.join(msg.split()) or '(it said nothing)'}"
        )


def assert_transport_is_faithful():
    """Prove bytes reach bash unaltered before trusting any comparison.

    Exits 2 (never raises) when there is no bash to ask; raises
    `HarnessBroken` when bash answered and the answer was wrong.  See the
    module docstring for why those two must not share an exit code.
    """
    _assert_bash_is_reachable()
    # A *quoted* here-doc delimiter turns off every form of processing, so
    # whatever comes back is exactly what arrived.  It has to be a here-doc
    # rather than a single-quoted string because the probe must contain a
    # `'` -- and a `'` cannot appear inside `'...'` at all.  (Getting that
    # wrong once already made this function report a faithful transport as
    # broken, which is the same class of mistake it exists to catch.)
    probe = rb"""a\b a\\b a\\\b "x" 'y' $z `w` %s ~ {a,b}"""
    r = run(b"cat <<'PROBE_EOF'\n" + probe + b"\nPROBE_EOF\n")
    if r.returncode != 0 or r.stdout != probe + b"\n":
        # Reachability was proved above, so bash exists and this is the
        # transport mangling bytes -- our bug, and it must carry a traceback
        # rather than the exit 1 that would read as "kshell disagrees".
        raise HarnessBroken(
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


def _assert_framing_failure_is_loud():
    """Prove a corrupt answer raises rather than returning None.

    A guard is only worth having if it has been seen to fire, so this makes
    the module fire it on itself every run instead of on the word of whoever
    wrote it.  Each case below is a *successful* bash exit (returncode 0)
    carrying output that cannot be framed -- exactly the shape that used to
    return None and be scored by the callers as `SKIP (bash rejected)`.

    Stubbing `run` is the whole point: these outputs cannot be provoked from
    real bash, which is why the bug was invisible.  A defect that only shows
    up when the transport misbehaves needs the transport made to misbehave.
    """
    global run
    real_run = run

    class _Fake:
        def __init__(self, out):
            self.returncode = 0
            self.stdout = out
            self.stderr = b""

    # (stdout bash "returned", what is wrong with it)
    corrupt = [
        (b"", "empty output -- no count line at all"),
        (b"2", "a count with no newline, i.e. a truncated read"),
        (b"not-a-number\na\0b\0", "a first line that is not a count"),
        (b"3\na\0b\0", "count says 3, payload frames 2 -- a short read"),
        (b"1\na\0b\0", "count says 1, payload frames 2 -- an over-long read"),
        (b"1\na", "a final word with no NUL terminator"),
    ]
    try:
        for out, why in corrupt:
            run = lambda _script, _o=out: _Fake(_o)  # noqa: E731
            try:
                got = words("x")
            except ProbeError:
                continue  # fired, as it must
            raise HarnessBroken(
                "THE PROBE'S OWN GUARD DOES NOT FIRE -- it would score a "
                "broken harness as data.\n"
                f"  corrupt output: {out!r}\n"
                f"  what is wrong : {why}\n"
                f"  words() returned {got!r} instead of raising ProbeError."
                + ("\n  None is the value callers print as a bash rejection "
                   "and\n  check-shellquote-vs-bash.py skips without counting "
                   "it." if got is None else "")
            )
    finally:
        run = real_run


def _assert_word_probe_is_exact():
    """Prove `words()` itself before trusting a single one of its answers.

    `assert_transport_is_faithful` used to stop at the here-doc, which proved
    only that bytes reach bash -- not that the *answers* come back intact.
    The subtle half of this harness is the return path, and that is exactly
    where it was wrong for as long as no case happened to contain a newline.
    """
    _assert_framing_failure_is_loud()
    for line, want in _WORD_PROBE_SELF_TEST:
        got = words(line)
        if got != want:
            raise HarnessBroken(
                "THE WORD PROBE IS BROKEN -- every result below would be a lie.\n"
                f"  line: {line!r}\n"
                f"  want: {want!r}\n"
                f"  got : {got!r}"
                + ("\n  (None means bash itself rejected the line -- see stderr"
                   "\n   by running the same script by hand. A framing failure"
                   "\n   would have raised ProbeError instead of reaching here.)"
                   if got is None else "")
            )


class ProbeError(RuntimeError):
    """The probe could not read bash's answer, though bash gave one.

    Distinct from `words()` returning None, which means bash *rejected* the
    line -- a fact about bash, and legitimate data.  This means the harness
    is broken, which is not data at all and must never be scored.

    The distinction exists because the two were the same value for as long as
    this file existed, and the callers do not treat them the same way by
    accident -- they treat them the same way because they cannot tell them
    apart.  `check-shellquote-vs-bash.py` prints `SKIP (bash rejected)` and
    `continue`s on None, which is correct for a syntax error and catastrophic
    for a framing bug: a skip is not a failure, so a malfunctioning probe
    would drop cases silently and still print `0 disagreements with bash`.
    That is a worse failure than the newline bug this module was fixed for
    last -- that one at least announced itself as `<bash error>`.
    """


def words(line: str, setup: str = "HOME=/root; USER=root; EMPTY=''"):
    """The exact word list bash produces for `line`, or None if bash errored.

    Uses `set --` so the count is bash's own, then emits the words separated
    by NUL.  Returns a list of `bytes` whose *length* is authoritative,
    including the empty list.

    Raises `ProbeError` -- never returns None -- when bash exited *successfully*
    but its output could not be framed.  Only a non-zero exit is reported as
    None, because only a non-zero exit is bash declining the line.  Every other
    outcome is this module failing to read an answer that was given, and the
    caller must not be allowed to mistake it for one.

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
    # `line` MUST be the last thing in the script, and the emitter MUST be a
    # function defined above it.
    #
    # The obvious layout -- `set -- <line>` with the printfs underneath -- is
    # broken for any line ending in a continuation.  A trailing backslash is a
    # backslash-newline, so `set -- a\` splices the *next line of this script*
    # onto itself and the probe measures its own source code: bash returned
    # the words `aprintf`, `%s\n`, `0` for the case `a\`, having consumed the
    # count printf entirely.  With no count line the result was unframeable,
    # which before ProbeError existed meant None, which
    # `check-shellquote-vs-bash.py` printed as `SKIP (bash rejected)` and did
    # not count -- so its trailing-backslash case had never once been tested,
    # in a file whose entire subject is backslashes.
    #
    # Putting `line` last means a continuation can only ever splice EOF onto
    # itself, which bash resolves by dropping the backslash-newline. There is
    # no line left to eat, so the failure mode has no fuel rather than being
    # detected after the fact.
    script = (
        'emit() { printf "%s\\n" "$#"; '
        'for w in "$@"; do printf "%s\\0" "$w"; done; }\n'
        f"{setup}\n"
        f"emit {line}\n"
    ).encode()
    r = run(script)
    # The ONLY None in this function. bash was handed the line and declined
    # it; that is a fact about bash and the caller is right to record it.
    if r.returncode != 0:
        return None

    def broken(why: str):
        return ProbeError(
            f"{why}\n"
            f"  line  : {line!r}\n"
            f"  stdout: {r.stdout!r}\n"
            f"  stderr: {r.stderr!r}\n"
            "  bash exited 0, so it answered and we failed to read it."
        )

    count, sep, rest = r.stdout.partition(b"\n")
    if not sep:
        raise broken("THE WORD PROBE IS BROKEN -- no count line in bash's output.")
    try:
        n = int(count)
    except ValueError as exc:
        raise broken(
            f"THE WORD PROBE IS BROKEN -- first line {count!r} is not a count."
        ) from exc
    # Every word is NUL-*terminated*, so n words leave a trailing empty field.
    # Checking for it is what catches a truncated or over-long read rather
    # than silently returning a short list.
    fields = rest.split(b"\0")
    if len(fields) != n + 1 or fields[-1] != b"":
        raise broken(
            f"THE WORD PROBE IS BROKEN -- bash counted {n} word(s) but the "
            f"payload framed as {len(fields) - 1}."
        )
    return fields[:n]
