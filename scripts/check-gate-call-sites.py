#!/usr/bin/env python3
"""Every gate call site must speak the interface its script currently has.

A gate is invoked as `run_checker <label> "$py" ".../check-foo.py" <args>`.  The
script and the call site are in different files and drift apart silently,
because a call site is not type-checked against an argparse parser and nothing
runs most gates except the boot test.

On 2026-09-10 that drift reddened `origin/main` for all three lanes.
`scripts/check-crate-names.py` was widened to walk the whole workspace and its
hand-rolled `argv` scan was replaced with argparse; the hand-rolled scan had
*used* unknown positionals, argparse rejects them.  The only caller still
passed `apps userspace`, so the gate exited 2 -- "no verdict" -- and
`run_checker` correctly refused the build.  That refusal is the system working,
but it costs a boot test to learn, and the boot test is the slowest thing here.
This check costs well under a second.

What it does NOT do: decide whether a gate's verdict is right.  Only whether
the gate can be reached at all.

Two classes of false positive have to be handled, or the gate is worse than
nothing -- a gate that cries wolf blocks three lanes on a non-finding.  Both
were found by running the candidates rather than by reading them, and both were
live in this tree at the time:

  * **A flag that takes a value.**  `check-self-tests-wired --emit-markers
    $GATED_MARKERS` -- `$GATED_MARKERS` is the flag's value, not a positional.
    Read the `add_argument` action/nargs to decide whether a flag consumes the
    token after it.

  * **A flag dispatched before argparse exists.**  `check-libc-shape
    --self-test` is handled off `sys.argv` near the bottom of that file and
    never reaches a parser; it exits 0 with 24/24 cases passing.  A flag whose
    literal string appears anywhere in the source is therefore accepted.

Implementation note worth keeping: the parser's declared names are read with
`ast`, taking only the *positional* arguments of each `add_argument` call.  A
regex over the call's text also picks up `dest="selftest"` and
`action="store_true"`, which look exactly like declared positionals -- that
mistake made the first version of this sweep report the known-bad case as fine,
i.e. it failed open on the one case it was written for.
"""

from __future__ import annotations

import argparse
import ast
import contextlib
import io
import pathlib
import re
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent

# The files that invoke gates through run_checker.
CALLERS = ("scripts/boot-test.sh", "scripts/hooks/pre-push")

# `run_checker [--may-skip] <label> "$py" <script> <args...>`, where <script> is
# either a quoted literal path or a shell variable holding one.  Both forms are
# live: boot-test.sh writes the path inline, scripts/hooks/pre-push assigns it
# first (`raced="${repo_root:-.}/scripts/raced-globals.py"`) and passes "$raced".
# Matching only the literal form is how this check would claim to cover pre-push
# while reading none of its 84 gate invocations -- and pre-push is the half that
# matters most, because it runs before a push and so before main can go red.
SITE = re.compile(
    r'run_checker\s+(?:--may-skip\s+)?(\S+)\s+"\$\w+"\s+"([^"]+)"([^;\n]*)'
)

# `name="${repo_root:-.}/scripts/thing.py"` -- how pre-push names a checker.
PATH_VAR = re.compile(r'^\s*(?:local\s+)?(\w+)="[^"]*/scripts/([\w.-]+\.py)"', re.M)

# add_argument actions that consume no following token.
NO_VALUE_ACTIONS = {
    "store_true",
    "store_false",
    "store_const",
    "count",
    "help",
    "version",
}

# Shell noise that is not an argument.
NOT_AN_ARG = {"", ";", "then", "&&", "||", "fi", "do"}


class Interface:
    """What a gate script's argparse will actually accept."""

    def __init__(self) -> None:
        self.flags: dict[str, bool] = {}  # flag -> consumes a value
        self.positionals: list[str] = []
        self.has_var_positional = False
        self.parsed = False

    def accepts_flag(self, flag: str) -> bool:
        return flag in self.flags or flag in ("-h", "--help")


def interface_of(src: str) -> Interface:
    """Read a script's argparse interface with ast.

    Only `add_argument`'s own positional arguments are names.  Keywords like
    `dest=` and `action=` are values, and counting them as names is how a
    sweep ends up certifying the bug it was written to catch.
    """
    iface = Interface()
    try:
        tree = ast.parse(src)
    except SyntaxError:
        return iface
    iface.parsed = True

    for node in ast.walk(tree):
        if not (
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Attribute)
            and node.func.attr == "add_argument"
        ):
            continue

        names = [
            a.value
            for a in node.args
            if isinstance(a, ast.Constant) and isinstance(a.value, str)
        ]
        if not names:
            continue

        kw = {k.arg: k.value for k in node.keywords if k.arg}
        action = None
        if isinstance(kw.get("action"), ast.Constant):
            action = kw["action"].value
        nargs = kw.get("nargs")
        nargs_val = nargs.value if isinstance(nargs, ast.Constant) else None

        if names[0].startswith("-"):
            consumes = action not in NO_VALUE_ACTIONS and nargs_val != 0
            for n in names:
                iface.flags[n] = consumes
        else:
            iface.positionals.append(names[0])
            if nargs_val in ("*", "+", argparse.REMAINDER):
                iface.has_var_positional = True

    return iface


def call_sites(
    repo: pathlib.Path,
    unresolved: list[tuple[str, str, str]] | None = None,
) -> list[tuple[str, str, str, list[str]]]:
    """(caller, gate label, script name, argument tokens) for every gate.

    `unresolved` collects call sites whose script path is a variable this
    function could not follow.  They are reported rather than dropped: a site
    skipped in silence is a hole in the coverage that nothing announces.
    """
    if unresolved is None:
        unresolved = []
    out: list[tuple[str, str, str, list[str]]] = []
    for rel in CALLERS:
        path = repo / rel
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        # A gate's arguments often sit on a continuation line.
        text = re.sub(r"\\\r?\n\s*", " ", text)

        # Resolve the variables a caller may hold a checker's path in.
        var_to_script = {v: s for v, s in PATH_VAR.findall(text)}

        for m in SITE.finditer(text):
            raw = m.group(2)
            if raw.endswith(".py"):
                script = raw.rsplit("/", 1)[-1]
            elif raw.startswith("$"):
                script = var_to_script.get(raw.lstrip("${").rstrip("}"))
                if script is None:
                    # Unresolvable: say so rather than skipping in silence.
                    unresolved.append((rel, m.group(1), raw))
                    continue
            else:
                continue
            tokens = [
                t.strip('"') for t in m.group(3).split() if t.strip('"') not in NOT_AN_ARG
            ]
            out.append((rel, m.group(1), script, tokens))
    return out


def check_site(
    tokens: list[str], iface: Interface, src: str
) -> list[str]:
    """Walk a call site the way argparse would.  Returns problem descriptions."""
    problems: list[str] = []
    positionals_given: list[str] = []
    i = 0
    while i < len(tokens):
        tok = tokens[i]
        if tok.startswith("-") and tok != "-":
            name, _, inline = tok.partition("=")
            if iface.accepts_flag(name):
                # A valued flag eats the next token unless it was given inline.
                if iface.flags.get(name, False) and not inline:
                    i += 1
            elif name in src:
                # Dispatched before argparse is built -- check-libc-shape does
                # this with --self-test.  Reachable, so not a finding.
                pass
            else:
                known = sorted(f for f in iface.flags if f.startswith("-"))
                problems.append(
                    f"passes {name}, which its parser does not declare "
                    f"(it knows {known or 'no flags'}) and which appears "
                    f"nowhere in the script"
                )
        else:
            positionals_given.append(tok)
        i += 1

    if positionals_given and not iface.positionals:
        problems.append(
            f"passes positional {positionals_given}, but the parser declares "
            f"no positional argument -- argparse exits 2 and the gate never "
            f"reaches a verdict"
        )
    return problems


def report(repo: pathlib.Path, verbose: bool = False) -> int:
    unresolved: list[tuple[str, str, str]] = []
    sites = call_sites(repo, unresolved)
    if not sites:
        # A sweep that parses nothing would otherwise pass every tree,
        # including a broken one.
        print(
            "check-gate-call-sites: parsed no run_checker call sites from "
            f"{', '.join(CALLERS)}, which cannot be right -- the pattern this "
            "script matches has probably changed",
            file=sys.stderr,
        )
        return 2

    findings = 0
    checked = 0
    handrolled = 0

    for caller, label, raw in unresolved:
        # Not a finding against the tree -- a gap in this check.  Printed so it
        # cannot be mistaken for coverage.
        #
        # DO NOT silence these the way `check-gates-are-wired` does. That gate grew
        # an exclusion on 2026-09-11 for pre-push gate 20, whose call site builds its
        # script path from a variable (`suite="...scripts/test-$stem.py"`), and the
        # exclusion is correct THERE because that gate asks "is every gate run?" and
        # a test suite is not a gate.
        #
        # This gate asks a different question -- "does every call site speak its
        # script's current interface?" -- and a call that runs a script with flags is
        # in scope whatever the script is. So for gate 20 the honest answer is the one
        # printed below: this check cannot verify that call site. Excluding it would
        # be claiming coverage rather than losing it, which is the difference between
        # the two gates and not an inconsistency between them.
        #
        # Harmless today for a narrower reason worth writing down: gate 20's call
        # passes NO flags, so there is no interface for it to disagree with. The note
        # earns its place against the edit that adds one.
        print(
            f"{caller}: gate {label} names its checker as {raw}, which this "
            "check could not resolve to a path; that call site is NOT covered"
        )
    for caller, label, script, tokens in sites:
        if not tokens:
            continue
        path = repo / "scripts" / script
        if not path.exists():
            print(f"{caller}: gate {label} invokes scripts/{script}, which does not exist")
            findings += 1
            continue
        src = path.read_text(encoding="utf-8", errors="replace")
        if "argparse" not in src:
            # A hand-rolled `sys.argv` scan does not exit 2 on a flag it does
            # not know -- it ignores it.  That is the more dangerous half of
            # this drift, not the lesser one: `--self-test` passed to a script
            # that has stopped handling `--self-test` runs the *main* check and
            # prints OK, so the self-test gate passes having tested nothing.
            # Positionals cannot be judged here (a hand-rolled scan may well
            # want them), but a flag that appears nowhere in the source is
            # certainly being dropped on the floor.
            handrolled += 1
            # A script that delegates to `selftestflag.wants_selftest` accepts
            # every spelling of the self-test flag without containing any of
            # them literally. Searching the source for the flag text is
            # otherwise exactly right, and was until 2026-09-10, when sixteen
            # scripts moved to the helper and this check reported all of them
            # as dropping a flag they had just started accepting MORE of.
            #
            # The indirection is the point of the helper, so the gate has to
            # know the one name it hides behind. If a second such helper ever
            # appears, it belongs in this tuple rather than in a looser match.
            delegates_selftest = "wants_selftest(" in src
            for tok in tokens:
                if not tok.startswith("-") or tok == "-":
                    continue
                name = tok.partition("=")[0]
                if name in src:
                    continue
                if delegates_selftest and name in (
                    "--self-test",
                    "--selftest",
                    "--self_test",
                ):
                    continue
                print(f"{caller}: gate {label} -> scripts/{script}")
                print(
                    f"    passes {name}, which appears nowhere in the script. "
                    "It parses sys.argv by hand, so the flag is silently "
                    "ignored rather than refused -- this gate reports OK "
                    "having done something other than what the call site asked"
                )
                print(f"    call site args: {tokens}")
                findings += 1
            continue
        iface = interface_of(src)
        if not iface.parsed:
            print(f"{caller}: scripts/{script} does not parse as Python")
            findings += 1
            continue
        checked += 1
        for problem in check_site(tokens, iface, src):
            print(f"{caller}: gate {label} -> scripts/{script}")
            print(f"    {problem}")
            print(f"    call site args: {tokens}")
            findings += 1

    if findings:
        print()
        print(
            f"{findings} gate call site(s) do not match their script's current "
            "interface.  Fix the call site, or the script, so the two agree: "
            "until then the gate either reaches no verdict (argparse exits 2) "
            "or reaches one about something other than what was asked (a "
            "hand-rolled scan ignoring the flag).  Either way what it checks "
            "is unchecked."
        )
        return 1

    if verbose:
        print(f"check-gate-call-sites: {len(sites)} sites, {checked} with arguments")
    print(
        f"check-gate-call-sites: OK ({len(sites)} gate call sites; "
        f"{checked} checked against an argparse parser, {handrolled} "
        "hand-rolled and checked for dropped flags only)"
    )
    return 0


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

SCRIPT_NO_POSITIONAL = '''
import argparse
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("--list", action="store_true")
    return 0
'''

SCRIPT_VALUED_FLAG = '''
import argparse
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit-markers", metavar="PATH", default=None)
    ap.add_argument("--quiet", action="store_true")
    return 0
'''

SCRIPT_PRE_ARGPARSE_FLAG = '''
import argparse, sys
def _selftest():
    return 0
def main():
    p = argparse.ArgumentParser()
    p.add_argument("--ignore-age", action="store_true")
    p.add_argument("-v", "--verbose", action="store_true")
    return 0
if __name__ == "__main__":
    if "--self-test" in sys.argv:
        sys.exit(_selftest())
    sys.exit(main())
'''

SCRIPT_WITH_POSITIONAL = '''
import argparse
def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("roots", nargs="*")
    return 0
'''


def self_test() -> int:
    """Each case is one that was live in this tree, or one that would mask it.

    The three real ones are kept because a synthetic suite that only exercises
    the shapes the author thought of is how the first version of this sweep
    came back clean on the very bug it was written for.
    """
    cases: list[tuple[str, str, list[str], bool, str]] = [
        # (label, script source, call-site tokens, expect_finding, why)
        (
            "positional passed to a parser with none (the real 2026-09-10 break)",
            SCRIPT_NO_POSITIONAL,
            ["apps", "userspace"],
            True,
            "argparse exits 2 on unrecognized arguments",
        ),
        (
            "known flag, no positionals",
            SCRIPT_NO_POSITIONAL,
            ["--self-test"],
            False,
            "declared via a second name on the same add_argument",
        ),
        (
            "valued flag consumes the token after it",
            SCRIPT_VALUED_FLAG,
            ["--emit-markers", "$GATED_MARKERS"],
            False,
            "the shell variable is the flag's value, not a positional",
        ),
        (
            "valued flag given inline",
            SCRIPT_VALUED_FLAG,
            ["--emit-markers=/tmp/x.json"],
            False,
            "--flag=value consumes nothing further",
        ),
        (
            "valued flag's value must not be mistaken for a positional",
            SCRIPT_VALUED_FLAG,
            ["--emit-markers", "/tmp/x.json", "extra"],
            True,
            "the third token really is an undeclared positional",
        ),
        (
            "flag dispatched off sys.argv before argparse (check-libc-shape)",
            SCRIPT_PRE_ARGPARSE_FLAG,
            ["--self-test"],
            False,
            "handled before a parser exists, so it is reachable",
        ),
        (
            "genuinely unknown flag",
            SCRIPT_PRE_ARGPARSE_FLAG,
            ["--no-such-flag"],
            True,
            "absent from the parser and from the source",
        ),
        (
            "store_true flag followed by a positional",
            SCRIPT_NO_POSITIONAL,
            ["--list", "apps"],
            True,
            "store_true eats nothing, so apps is a positional",
        ),
        (
            "parser that does declare a positional",
            SCRIPT_WITH_POSITIONAL,
            ["apps", "userspace"],
            False,
            "this is what the call site was written against in 3fe3552e8",
        ),
    ]

    failures = 0
    for label, src, tokens, expect, why in cases:
        iface = interface_of(src)
        got = bool(check_site(tokens, iface, src))
        if got != expect:
            verb = "missed" if expect else "falsely reported"
            print(f"selftest FAIL: {verb} -- {label} ({why})", file=sys.stderr)
            failures += 1

    # The regression that mattered most: reading add_argument with a regex
    # instead of ast scrapes dest=/action= and invents positionals.
    iface = interface_of(SCRIPT_NO_POSITIONAL)
    if iface.positionals:
        print(
            "selftest FAIL: a parser with no positional argument was read as "
            f"having {iface.positionals} -- the keyword values of add_argument "
            "are being counted as declared names, which makes every "
            "positional-passing call site look fine",
            file=sys.stderr,
        )
        failures += 1

    # The hand-rolled branch lives in report(), not check_site(), so it needs a
    # tree to run against.  Both directions matter: a dropped flag is the
    # silent half of this drift, and flagging a flag the script *does* handle
    # would block three lanes on nothing.
    handrolled_cases = [
        ("--self-test", 'if "--self-test" in sys.argv:\n    pass\n', False),
        ("--self-test", 'if "--selftest" in sys.argv:\n    pass\n', True),
    ]
    for flag, body, expect in handrolled_cases:
        with tempfile.TemporaryDirectory() as td:
            tree = pathlib.Path(td)
            (tree / "scripts").mkdir()
            # newline="" on both: this file is graded by
            # scripts/check-text-mode-writes.py, and a text-mode write with no
            # newline= turns every \n into \r\n on Windows. It caught these two
            # the first time this gate was boot-tested.
            (tree / "scripts" / "handrolled.py").write_text(
                "import sys\n" + body, encoding="utf-8", newline=""
            )
            (tree / "scripts" / "boot-test.sh").write_text(
                '    if ! run_checker hand "$py" '
                f'"$PROJECT_ROOT/scripts/handrolled.py" {flag}; then\n'
                "        return 1\n    fi\n",
                encoding="utf-8",
                newline="",
            )
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = report(tree)
            got = rc == 1
            if got != expect:
                verb = "missed" if expect else "falsely reported"
                print(
                    f"selftest FAIL: {verb} a hand-rolled gate passed {flag} "
                    f"against a script containing {body.splitlines()[0]!r}",
                    file=sys.stderr,
                )
                failures += 1

    # And the non-vacuity guard, checked on a tree with no callers at all.
    # Its complaint goes to stderr by design, so it is swallowed here -- a
    # self-test that prints "this cannot be right" while passing teaches the
    # reader to ignore the line that matters when it is real.
    with tempfile.TemporaryDirectory() as td:
        empty = pathlib.Path(td)
        (empty / "scripts").mkdir()
        with contextlib.redirect_stderr(io.StringIO()):
            rc = report(empty)
        if rc != 2:
            print(
                "selftest FAIL: a tree with no call sites returned "
                f"{rc}, not 2 -- a sweep that parses nothing must decline to "
                "give a verdict, not pass",
                file=sys.stderr,
            )
            failures += 1

    if failures:
        print(f"selftest: {failures} of {len(cases) + 4} cases FAILED", file=sys.stderr)
        return 1
    print(f"check-gate-call-sites --self-test: OK ({len(cases) + 4} cases)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="gate call sites vs their parsers")
    ap.add_argument("--self-test", "--selftest", action="store_true", dest="selftest")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return self_test()
    return report(REPO, verbose=args.verbose)


if __name__ == "__main__":
    sys.exit(main())
