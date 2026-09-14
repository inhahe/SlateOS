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
of spaces ARE the content. Only a literal that is an argument to `assert!`,
`assert_eq!`, `assert_ne!`, `panic!`, `unreachable!`, `expect` or
`expect_err` is touched -- none of the false positives is one.
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

MACRO = re.compile(
    r"\b(assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne"
    r"|panic|unreachable|todo|write|writeln|format|print|println|eprint|eprintln)!\s*\(|"
    r"\.(expect|expect_err|unwrap_or_else)\s*\("
)
# A run of 4+ spaces that is maximal: starting the match one space in would
# defeat the "not after a newline escape" test below.
RUN = re.compile(r"(?<!" + SP + r")" + SP + r"{4,}")


def opens_a_message(lines, i):
    """Whether line `i` sits inside an assertion-like macro call.

    Walks back while the paren depth says we are still inside a call, and
    reports whether the call that encloses us is one of the message-bearing
    ones. Bounded at 12 lines: an assertion whose message is a dozen lines
    below its macro is not a thing in this tree.
    """
    depth = 0
    for k in range(i, max(-1, i - 12), -1):
        line = lines[k]
        if k < i:
            depth += line.count(")") - line.count("(")
        if depth < 0 or (k < i and MACRO.search(line) and depth <= 0):
            return bool(MACRO.search(line))
    return False


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
    for top in ("gui", "apps", "scripts"):
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
    print(f"ok -- no collapsed assertion messages ({files} source file(s)).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
