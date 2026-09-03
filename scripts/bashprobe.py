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

As of 2026-09-03 an absent WSL is a **declined verdict (exit 2)** rather than
the exit 1 it used to be, which `run_checker` reads as "this gate found a
violation" -- a lie in the worst direction, since it says the rule is broken
when in truth it was never checked.  That is the first of three things that
must be true before these four can be wired with `--may-skip` and unpinned
from `scripts/check-gates-are-wired.py`; see `known-issues.md ->
TD-B-THE-FOUR-BASH-ORACLES-ARE-PINNED-NOT-WIRED` for the other two.
`bashprobe.py --self-test` proves all three transport outcomes on a host with
no WSL at all -- which is the only kind of host any of this runs on today, and
so was the only place the old code was never exercised.

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
import sys

WSL = ["wsl", "-d", "Ubuntu", "--", "bash", "-s"]

#: Exit code meaning "I could not look", as distinct from "I looked and found
#: something wrong".  `scripts/run-checker.sh` reads 2 as a declined verdict:
#: at a call site carrying `--may-skip` it is a loud skip, and anywhere else it
#: aborts the run.  See design-decisions.md §753.
EXIT_COULD_NOT_LOOK = 2


class NoBash(RuntimeError):
    """WSL is not here, so no question can be put to bash at all.

    Sharply distinct from a *broken* transport.  This means the oracle was
    never consulted; that means the oracle answered and the answer cannot be
    trusted.  They must not share an exit code -- see `_decline` below.
    """


def _decline(reason: str) -> "None":
    """Exit 2 with a spoken reason, never a `usage:` line.

    Both halves of that sentence are load-bearing, and neither is obvious:

    * **Exit 2, not 1.** `raise SystemExit("...")` exits **1**, which
      `run_checker` reads as "this gate found a violation" -- so an absent WSL
      used to be reported in the same channel as a real disagreement with bash.
      That is a lie in the direction that matters: it says the rule is broken
      when in fact it was never checked.

    * **The reason must not begin `usage:`.** `run_checker` refuses to read a
      decline whose first line looks like an argparse usage banner, because
      argparse *also* exits 2 and a mistyped invocation would otherwise be
      taken as a legitimate decline and skip on every host forever (lane B's
      reply in `requests/a-b-yes-to-the-self-test-rule-and-one-half-it-does-
      not-cover.md`).  So this function exists partly to keep the wording out
      of the hands of each call site.
    """
    print(reason, file=sys.stderr)
    sys.exit(EXIT_COULD_NOT_LOOK)


def run(script: bytes):
    """Run `script` under bash with the bytes delivered verbatim.

    Raises `NoBash` if `wsl.exe` is not on PATH.  Left as an exception rather
    than exiting here because `run` is stubbed out by this module's own
    self-checks, and a function that can exit the process is a poor thing to
    stub.
    """
    try:
        return subprocess.run(WSL, input=script, capture_output=True)
    except OSError as exc:
        # FileNotFoundError on a machine with no WSL at all; also covers the
        # rarer permission and image-format failures, all of which mean the
        # same thing to a caller: the oracle was not reached.
        raise NoBash(f"{WSL[0]}: {exc}") from exc


def assert_transport_is_faithful():
    """Prove bytes reach bash unaltered before trusting any comparison.

    Three outcomes, not two, and the third is why this function is longer than
    its one comparison:

    | | means | what happens |
    |---|---|---|
    | bytes arrive intact | the oracle is usable | return |
    | `wsl` never ran, or ran and produced nothing | **no oracle here** | exit 2, declined |
    | bash ran and the bytes came back altered | **the oracle is lying** | exit 1, a finding |

    The last two must never be merged. A lying transport that exited 2 would
    be *skipped* at any call site carrying `--may-skip` -- a harness bug
    silently converted into "nothing to check here", which is the exact defect
    this module's guards exist to prevent, one level up.
    """
    # A *quoted* here-doc delimiter turns off every form of processing, so
    # whatever comes back is exactly what arrived.  It has to be a here-doc
    # rather than a single-quoted string because the probe must contain a
    # `'` -- and a `'` cannot appear inside `'...'` at all.  (Getting that
    # wrong once already made this function report a faithful transport as
    # broken, which is the same class of mistake it exists to catch.)
    probe = rb"""a\b a\\b a\\\b "x" 'y' $z `w` %s ~ {a,b}"""
    try:
        r = run(b"cat <<'PROBE_EOF'\n" + probe + b"\nPROBE_EOF\n")
    except NoBash as exc:
        _decline(f"no bash to ask: {exc}. This gate compares kshell's quoting "
                 f"against real bash via `{' '.join(WSL)}`, and there is no "
                 f"WSL on this machine, so nothing was compared.")

    # `wsl` failing *and* saying nothing on stdout means the shell never
    # started -- an unregistered distro, a WSL that needs its first-run setup,
    # a kernel update pending. Declined, because there is no answer to doubt.
    #
    # The `not r.stdout` half is what keeps this from swallowing the case
    # below: a transport that mangles bytes still produces output, so it
    # cannot land here however badly it is broken.
    if r.returncode != 0 and not r.stdout:
        err = (r.stderr.decode("utf-8", "replace").strip()
               or f"(no output; exit {r.returncode})")
        _decline(f"bash did not start: {err.splitlines()[0]} -- `"
                 f"{' '.join(WSL)}` exited {r.returncode} without running "
                 f"anything, so nothing was compared.")

    if r.returncode != 0 or r.stdout != probe + b"\n":
        # Deliberately exit 1 (a finding), NOT 2. Bash ran and answered; the
        # answer came back altered, which is a harness bug and must be
        # reported, not skipped. Exiting 2 here would let a `--may-skip` call
        # site read a corrupted oracle as "nothing to check".
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
            raise SystemExit(
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
            raise SystemExit(
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


def _selftest() -> int:
    """Prove the three transport outcomes are three, on a host with no WSL.

    This module's guards have always been self-firing (`_assert_framing_
    failure_is_loud` stubs `run` and insists the guard raises), but the
    *transport* half had no such treatment: every path through
    `assert_transport_is_faithful` needed a real bash to reach, so on the
    machines where it matters most -- the ones without WSL -- the only exercised
    path was the one that crashed.

    What is checked is the exit code, not the prose, because the exit code is
    the entire interface `run_checker` has to this file:

      * absent WSL      -> 2, declined, and the message must not start `usage:`
      * bash never ran  -> 2, likewise
      * bash lied       -> 1, a finding, and emphatically NOT 2

    That last one is the case worth having a test for. It is one keystroke from
    the case above it, it would be caught by no compiler, and getting it wrong
    converts a broken oracle into a silent skip at any `--may-skip` call site.
    """
    global run, _assert_word_probe_is_exact
    real_run = run
    real_word_probe = _assert_word_probe_is_exact
    checks = bad = 0

    # Local: only the self-test needs them, and this module is imported by
    # four gates that do not.
    import contextlib
    import io

    def case(label: str, want_code: int, stub) -> str:
        """Run the transport check under `stub` and grade its exit code.

        Returns what it wrote to stderr, so the wording cases below can grade
        the same run rather than provoking a second one -- two runs of "the
        same" case is how a test starts asserting about something other than
        what it reports on.
        """
        nonlocal checks, bad
        global run, _assert_word_probe_is_exact
        checks += 1
        run = stub
        # Stubbed out for the whole of this function: none of these cases is
        # about the word probe, which already fires its own guards on itself
        # every run (`_assert_framing_failure_is_loud`). Left in, it would make
        # the *true negative* below unreachable without re-implementing bash --
        # and a true negative you cannot reach is the case that stops the rest
        # from meaning anything.
        _assert_word_probe_is_exact = lambda: None  # noqa: E731
        said = io.StringIO()
        try:
            with contextlib.redirect_stderr(said):
                assert_transport_is_faithful()
        except SystemExit as exc:
            got = exc.code if isinstance(exc.code, int) else 1
        except Exception as exc:  # noqa: BLE001 -- any escape is a failure
            print(f"selftest FAIL: {label}: escaped as "
                  f"{type(exc).__name__}: {exc}", file=sys.stderr)
            bad += 1
            return said.getvalue()
        else:
            got = 0
        if got != want_code:
            print(f"selftest FAIL: {label}: exit {got}, want {want_code}",
                  file=sys.stderr)
            bad += 1
        else:
            print(f"ok   {label} -> exit {got}")
            first = said.getvalue().strip().splitlines()
            if first:
                print(f"       said: {first[0][:96]}")
        return said.getvalue()

    def check(label: str, ok: bool):
        nonlocal checks, bad
        checks += 1
        if ok:
            print(f"ok   {label}")
        else:
            print(f"selftest FAIL: {label}", file=sys.stderr)
            bad += 1

    class _R:
        def __init__(self, code, out=b"", err=b""):
            self.returncode, self.stdout, self.stderr = code, out, err

    def _no_wsl(_script):
        raise NoBash("wsl: [WinError 2] The system cannot find the file "
                     "specified")

    # The probe string this module actually sends, and the exact corruption it
    # was written to detect: `wsl.exe` re-parsing the command line collapsed
    # `a\\b` to `a\b`, so every backslash case silently tested a different
    # input than the one written down. A fixture that merely differs from the
    # probe would pass against a comparison broken in any direction; this one
    # differs the way the real bug differed.
    _PROBE = rb"""a\b a\\b a\\\b "x" 'y' $z `w` %s ~ {a,b}"""
    _COLLAPSED = _PROBE.replace(b"\\\\", b"\\") + b"\n"

    try:
        said = case("no wsl.exe at all is a decline",
                    EXIT_COULD_NOT_LOOK, _no_wsl)
        # Both properties run_checker requires of a decline, graded on the
        # message that decline actually printed.
        check("...and it does not open with a usage: banner",
              not said.lstrip().startswith("usage:"))
        check("...and it says what could not be done",
              "no bash" in said.lower() or "nothing was compared" in said)

        said = case("wsl present, distro missing, is a decline",
                    EXIT_COULD_NOT_LOOK,
                    lambda _s: _R(1, b"", b"There is no distribution with the "
                                          b"supplied name.\n"))
        check("...and it repeats what wsl said, so the log names the cause",
              "no distribution" in said)

        # wsl's own usage banner must not become OUR first line: run_checker
        # refuses a decline opening `usage:` (argparse also exits 2), so a
        # decline that parroted it would abort every run instead of skipping.
        said = case("a decline that quotes wsl's usage banner still declines",
                    EXIT_COULD_NOT_LOOK,
                    lambda _s: _R(1, b"", b"usage: wsl [options]\n"))
        check("...without opening with `usage:` itself",
              not said.lstrip().startswith("usage:"))

        case("wsl failing silently is still a decline", EXIT_COULD_NOT_LOOK,
             lambda _s: _R(-1, b"", b""))

        # The two that must not drift into the arm above. Output came back, so
        # bash answered; a wrong answer is a harness bug and has to be
        # reported, not skipped.
        case("the historical backslash collapse is a finding, not a decline",
             1, lambda _s: _R(0, _COLLAPSED))
        case("a nonzero exit WITH output is a finding, not a decline", 1,
             lambda _s: _R(2, b"partial output\n"))

        # The true negative, without which every case above is satisfied by a
        # function that refuses everything.
        case("an intact round trip is neither", 0,
             lambda _s: _R(0, _PROBE + b"\n"))

        # And the real `run`, against a real absent executable. Every case
        # above stubs `run` out wholesale, so none of them touches the
        # OSError -> NoBash conversion inside it -- which is precisely the line
        # that fires on a machine with no WSL, i.e. the only line here that has
        # a job to do in production. A mutation deleting that conversion
        # survived the first version of this self-test for exactly that reason.
        global WSL
        real_wsl = WSL
        try:
            WSL = ["no-such-program-eb4f1c", "-s"]
            run = real_run
            checks += 1
            try:
                run(b"true\n")
            except NoBash as exc:
                print("ok   the real run() converts a missing executable "
                      "into NoBash")
                check("...and names the program that is missing",
                      real_wsl[0] in str(exc) or "no-such-program-eb4f1c"
                      in str(exc))
            except OSError as exc:
                print(f"selftest FAIL: the real run() let OSError escape: "
                      f"{exc}", file=sys.stderr)
                bad += 1
            else:
                print("selftest FAIL: the real run() returned a result for a "
                      "program that does not exist", file=sys.stderr)
                bad += 1
        finally:
            WSL = real_wsl
    finally:
        run = real_run
        _assert_word_probe_is_exact = real_word_probe

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:] or "--selftest" in sys.argv[1:]:
        sys.exit(_selftest())
    print("bashprobe is a library, not a command. Run one of the four "
          "check-*-vs-bash.py gates, or `bashprobe.py --self-test`.",
          file=sys.stderr)
    sys.exit(1)
