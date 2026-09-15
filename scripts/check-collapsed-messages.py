"""Refuse an assertion message whose line continuation rustfmt collapsed.

A message written across two source lines with a trailing backslash relies on
Rust stripping the newline *and* the following indentation. When `rustfmt`
later joins the two lines the indentation is left inside the literal, so the
message prints with a gap in the middle of a sentence -- at the exact moment
somebody is reading a failure quickly.

DECIDES BY POSITION, NOT BY SHAPE, which is the whole of why the first attempt
was reverted. A blanket regex for four-or-more spaces inside any string literal
ate `apps/screenshot`'s column-aligned shortcut list ("Alt+PrintScreen
Active window") and a JSON fixture's indentation. Both are strings whose runs
of spaces ARE the content.

PROSE FOR A HUMAN, NOT A FORMAT FOR A COLUMN. That is the real discriminator,
and it took two corrections to reach. The rule is not "which macro" and not
quite "which argument" either: it is whether the string is something a person
reads as a sentence, or a layout in which **the whitespace IS the payload**. A
checker that rewrites a layout is corrupting data rather than tidying it.

Two consequences, both measured against the whole tree on 2026-09-15 by lane B,
who found the macro set said one thing and the docstring another:

  * `write`, `writeln`, `format`, `print`, `println`, `eprint`, `eprintln` are
    NOT message-bearing and are gone from the set. Their content is output. Of
    73 tree-wide findings, 70 were theirs: `free.rs` and `ls.rs` expected-output
    fixtures, `arp`'s table header, the kernel's `PCPU: hit={}%  refills={}`.
    Three genuine ones hid among them, which is the ratio that says the
    predicate is wrong rather than the fixtures.
  * Only the MESSAGE argument of an assertion is prose. `assert_eq!(out,
    "aligned   output")` is inside an assertion macro and is an expected value,
    not a message. That was the remaining 27. The message is the third argument
    of `assert_eq!`/`assert_ne!`, the second of `assert!`, the first of
    `panic!`/`unreachable!`/`todo!`/`expect`/`expect_err`.

`unwrap_or_else` is gone too: its argument is a closure, never a message.
"""
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402

# Derived from `__file__`, never hardcoded. This read
#
#     ROOT = pathlib.Path(r"E:/visual studio projects/os-lane-c")
#
# until 2026-09-14, so every lane's boot scanned lane C's worktree instead of
# its own. Two consequences, and the second is worse: lane C's *uncommitted*
# edits could refuse another lane's build -- which is how this was found, when
# a message in `gui/desktop/src/session/tests.rs` that does not exist in lane
# A's tree at all failed lane A's boot at gate 60 -- and this gate's verdict
# never described the tree actually being built, so a lane could not have been
# cleared by it either.
#
# `check-eol.py` already carries the rule in a comment: ROOT derived from
# `__file__` means `os-lane-a/scripts/...` checks `os-lane-a` and nothing can
# redirect it. Same derivation here.
ROOT = pathlib.Path(__file__).resolve().parent.parent
NL = chr(10)
BS = chr(92)
SP = chr(32)

# Which argument of each call is prose a person reads. Everything absent from
# this table is not message-bearing at all -- the output macros because their
# content is a layout, `unwrap_or_else` because its argument is a closure.
MESSAGE_ARG = {
    "assert": 1,
    "debug_assert": 1,
    "assert_eq": 2,
    "assert_ne": 2,
    "debug_assert_eq": 2,
    "debug_assert_ne": 2,
    "panic": 0,
    "unreachable": 0,
    "todo": 0,
    "expect": 0,
    "expect_err": 0,
}

MACRO = re.compile(
    r"\b(assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne"
    r"|panic|unreachable|todo)!\s*\(|"
    r"\.(expect|expect_err)\s*\("
)
# A run of 4+ spaces that is maximal: starting the match one space in would
# defeat the "not after a newline escape" test below.
# The directories this scans. Everything outside them is invisible to it,
# which is a statement the summary line has to make rather than imply.
CORPUS = ("gui", "apps", "scripts")

RUN = re.compile(r"(?<!" + SP + r")" + SP + r"{4,}")


def opens_a_message(lines, i):
    """Whether line `i` is the MESSAGE argument of a message-bearing call.

    Walks back while the paren depth says we are still inside a call, finds the
    call that encloses us, and then asks a second question the first version of
    this did not: **which argument are we?** `assert_eq!(out, "aligned
    output")` is inside an assertion macro and is an expected value, not
    a message, and rewriting its spaces would corrupt the thing being asserted.

    Bounded at 12 lines: an assertion whose message is a dozen lines below its
    macro is not a thing in this tree.
    """
    depth = 0
    for k in range(i, max(-1, i - 12), -1):
        line = lines[k]
        if k < i:
            depth += line.count(")") - line.count("(")
        if depth < 0 or (k < i and MACRO.search(line) and depth <= 0):
            m = MACRO.search(line)
            if not m:
                return False
            name = m.group(1) or m.group(2)
            want = MESSAGE_ARG.get(name)
            if want is None:
                return False
            return arg_index(lines, k, m.end() - 1, i) == want
    return False


def arg_index(lines, macro_line, open_paren, literal_line):
    """Which argument of the call opened at `open_paren` `literal_line` is.

    Counts top-level commas between the opening parenthesis and the start of
    the literal's line. Commas inside a nested call, a closure, a tuple or a
    string do not separate arguments, so nesting is tracked and the text is
    blanked first -- `assert!(f(a, b), "msg")` has one top-level comma, not
    two, and the message is still argument 1.
    """
    from rustlex import strip_noise

    span = [lines[macro_line][open_paren + 1 :]]
    span.extend(lines[k] for k in range(macro_line + 1, literal_line))
    blanked = strip_noise(NL.join(span))
    depth, commas = 0, 0
    for ch in blanked:
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            if depth == 0:
                break
            depth -= 1
        elif ch == "," and depth == 0:
            commas += 1
    return commas


def repair(path, apply):
    lines = path.read_text(encoding="utf-8", errors="replace").split(NL)
    out, changed = [], []
    for i, line in enumerate(lines):
        st = line.strip()
        body = None
        if st.startswith('"') and st.endswith('",'):
            body = st[1:-2]
        elif st.startswith('"') and st.endswith('"'):
            body = st[1:-1]
        if (
            body is None
            or BS + "n" in body
            or BS + "t" in body
            or body.startswith(SP)
            or body.endswith(SP)
            or not RUN.search(body)
            or not opens_a_message(lines, i)
        ):
            out.append(line)
            continue
        indent = line[: len(line) - len(line.lstrip())]
        fixed = RUN.sub(SP, body)
        out.append(indent + '"' + fixed + ('",' if st.endswith(",") else '"'))
        changed.append((i + 1, fixed[:64]))
    if apply and changed:
        path.write_text(NL.join(out), encoding="utf-8", newline=NL)
    return changed


# `(source, expected_repairs)`. Three of the five are the false positives that
# forced the first attempt at this to be reverted: their runs of spaces are the
# content, and none of them is an assertion message.
SELF_TESTS = [
    (
        # The case lane B measured 27 of, tree-wide. It is inside an assertion
        # macro, which the first version of this considered sufficient -- and
        # it is the EXPECTED VALUE. Rewriting its spaces would change the thing
        # being asserted, so the checker would be corrupting the test it was
        # tidying.
        "an expected value that happens to be aligned output is left alone",
        '''fn t() {
    assert_eq!(
        rendered,
        "NAME        SIZE        MODE"
    );
}
''',
        0,
    ),
    (
        # Same shape one argument further on: this one IS the message.
        "the third argument of assert_eq is a message and is repaired",
        '''fn t() {
    assert_eq!(
        got,
        want,
        "the row did not              survive the round trip"
    );
}
''',
        1,
    ),
    (
        # `println!` and its family are layout, not prose: 70 of lane B's 73
        # tree-wide findings were theirs, and the whitespace is the payload.
        "a printed table header is left alone",
        '''fn show() {
    println!(
        "PCPU: hit={}%        refills={}",
        hit, refills
    );
}
''',
        0,
    ),
    (
        # A comma inside a nested call does not separate arguments, so the
        # message is still argument 1 and is still repaired.
        "a nested call does not shift the argument count",
        '''fn t() {
    assert!(
        within(a, b),
        "the value drifted              out of range"
    );
}
''',
        1,
    ),
    (
        "an assertion message loses its collapsed indentation",
        '''fn t() {
    assert!(
        ok,
        "the stacking              is confined, not the focus"
    );
}
''',
        1,
    ),
    (
        "a column-aligned help line is left alone",
        '''fn help() -> Vec<&'static str> {
    vec![
        "PrintScreen          Full screen",
        "Alt+PrintScreen      Active window",
    ]
}
''',
        0,
    ),
    (
        "a JSON fixture's indentation is left alone",
        '''fn body() -> String {
    let mut s = String::new();
    s.push_str("      \\"type\\": \\"{}\\",");
    s
}
''',
        0,
    ),
    (
        "a table row with alignment specifiers is left alone",
        '''fn row() {
    println!("    .{:<12} {:>6} files  {:>12}", a, b, c);
}
''',
        0,
    ),
    (
        "an already-repaired message is not touched twice",
        '''fn t() {
    assert!(ok, "the stacking is confined, not the focus");
}
''',
        0,
    ),
]


def self_test():
    """Grade the rule against the cases that forced the first version back.

    The reverted attempt matched four-or-more spaces inside any string literal,
    which is a rule about *shape*. Three of the five fixtures below are strings
    whose spaces are the content -- a shortcut list, a JSON body, a table row --
    and a shape rule rewrites all three. Deciding by *position* -- is this
    literal an argument to an assertion macro -- excludes them by construction
    rather than by exception list.
    """
    import tempfile

    failed = 0
    for name, source, want in SELF_TESTS:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "fixture.rs"
            path.write_text(source, encoding="utf-8", newline=NL)
            got = len(repair(path, False))
        ok = got == want
        print(("ok   " if ok else "FAIL ") + name)
        if not ok:
            print(f"       expected {want} repair(s), got {got}")
            failed += 1
    print(f"{NL}{len(SELF_TESTS)} self-test case(s), {failed} failed")
    return 1 if failed else 0


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    apply = "--apply" in argv
    unknown = selftestflag.unknown_options(argv, known=("--apply",))
    if unknown:
        for opt in unknown:
            print("check-collapsed-messages: unrecognized option " + repr(opt))
        print("usage: check-collapsed-messages.py [--apply] [--self-test]")
        return 2

    total, files = 0, 0
    # STILL `gui`/`apps`/`scripts`, and widening it is a real piece of work
    # rather than a one-line change. Measured on 2026-09-15 by pointing this
    # at the whole tree: 73 findings, of which 3 were genuine collapsed
    # assertion messages and 70 were column-aligned text -- `free.rs` and
    # `ls.rs` expected-output fixtures, `arp`'s table header, the kernel's
    # `PCPU: hit={}%  refills={}` statistics lines.
    #
    # Narrowing the macro set to assertions alone takes it to 30, and the
    # remaining 27 are the same class one level in: an `assert_eq!` whose
    # EXPECTED VALUE is formatted output, sitting on its own line. So the
    # discriminator is not the macro, it is WHICH ARGUMENT -- only the
    # message is prose, and the message is the third argument of
    # `assert_eq!`, the second of `assert!`, the first of `panic!`.
    #
    # That is the fix, and it is lane C's to make: it changes what this tool
    # considers a message, which is its whole design. Filed back to them with
    # these numbers.
    # NAMED, and named in the output too. Lane A read "ok (382 source file(s))"
    # after every merge as reassurance about their tree, and it never scanned
    # their tree: no kernel/, no deflate/, no bench/, no netproto/. The 382
    # files were not too few, **they were the wrong 382** -- the same shape as
    # the hardcoded ROOT one screen up, and the count was the one number that
    # would have exposed it. A total that does not say what it counted invites
    # exactly that reading, so the summary below says which directories.
    #
    # Widening this is lane B's call, not a change to smuggle in alongside a
    # predicate fix: it decides what a shared gate refuses for all three lanes.
    # The measurement that unblocks the decision is in
    # requests/c-b-the-argument-position-question-answered-and-six-in-your-globs.md
    for top in CORPUS:
        for path in sorted((ROOT / top).rglob("*.rs")):
            files += 1
            for ln, text in repair(path, apply):
                print(f"{path.relative_to(ROOT).as_posix()}:{ln}  {text}")
                total += 1
    if not files:
        print("no source files scanned; refusing to call that a pass", file=sys.stderr)
        return 2
    if total and apply:
        print(f"{NL}{total} assertion message(s) repaired")
        return 0
    if total:
        print(
            f"{NL}{total} assertion message(s) carry a collapsed line continuation. "
            "Run:  python scripts/check-collapsed-messages.py --apply",
            file=sys.stderr,
        )
        return 1
    print(
        f"ok -- no collapsed assertion messages "
        f"({files} source file(s) under {', '.join(CORPUS)})."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
