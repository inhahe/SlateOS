#!/usr/bin/env python3
"""Fail when a `RAN-IF` marker is not printed by the call it annotates.

Why this exists
---------------

`check-gated-selftests.py` asks "has this gated suite ever announced itself?"
and answers from the `RAN-IF:` comment on each dispatch in `kernel/src/main.rs`.
That comment is prose. Nothing checked that the serial line it names is emitted
by the function it is attached to.

On 2026-09-12 one of the six named a neighbour's banner: `fs::fat::self_test()`
declared `[fat] Running mkfs/format self-test...`, which `format_self_test`
prints. That suite is dispatched unconditionally, so the marker was present on
every boot and the gate reported the gated site as running in 135 of 136 boots.
The suite behind the gate -- 1,184 lines of FAT read/write coverage -- had never
run. The 2026-08-31 audit that cleared all six read the same declared markers
against a serial log, so it agreed, for the same reason.

Both of those consult the boot log. This one deliberately does not: it resolves
the annotated call to its `fn` and asserts the literal appears in that body. A
marker and a dispatch that cannot disagree are one witness with extra steps.
Reading the source is what lets them disagree.

Exit status: 0 every annotation resolves and matches, 1 findings, 2 the tree or
the convention could not be read at all.
"""

import io
import os
import re
import sys
import tempfile

RAN_IF = re.compile(r'^\s*//\s*RAN-IF:\s*"(.+)"\s*$')
CALL = re.compile(r"([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s*\(")
FN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?"
    r'(?:extern\s+"[^"]*"\s+)?fn\s+([A-Za-z0-9_]+)'
)
NL = chr(10)
QUOTE = chr(34)
BACKSLASH = chr(92)


def strip_comment(line):
    """Drop a trailing `//` comment, ignoring one inside a string literal.

    Needed in both directions: the annotation itself contains the marker, so a
    naive search finds it in `main.rs` and calls every site correct.
    """
    out = []
    in_str = False
    i = 0
    while i < len(line):
        c = line[i]
        if in_str:
            if c == BACKSLASH:
                out.append(c)
                i += 1
                if i < len(line):
                    out.append(line[i])
                    i += 1
                continue
            if c == QUOTE:
                in_str = False
        elif c == QUOTE:
            in_str = True
        elif c == "/" and i + 1 < len(line) and line[i + 1] == "/":
            break
        out.append(c)
        i += 1
    return "".join(out)


def rs_files(src):
    out = []
    for base, dirs, names in os.walk(src):
        dirs[:] = [d for d in dirs if d not in ("target", ".git")]
        for n in names:
            if n.endswith(".rs"):
                out.append(os.path.join(base, n))
    return sorted(out)


def fn_bodies(src):
    """{fn name: [(relpath, line, body with comments stripped)]}"""
    index = {}
    for path in rs_files(src):
        try:
            lines = io.open(path, encoding="utf-8").read().split(NL)
        except (OSError, UnicodeDecodeError):
            continue
        for i, ln in enumerate(lines):
            m = FN.match(ln)
            if not m:
                continue
            depth = 0
            started = False
            body = []
            for j in range(i, len(lines)):
                code = strip_comment(lines[j])
                body.append(code)
                depth += code.count("{") - code.count("}")
                if "{" in code:
                    started = True
                if started and depth <= 0:
                    break
            index.setdefault(m.group(1), []).append(
                (os.path.relpath(path, src), i + 1, NL.join(body))
            )
    return index


def annotations(main_rs):
    """[(line, marker, call or None)] for every RAN-IF in main.rs."""
    lines = io.open(main_rs, encoding="utf-8").read().split(NL)
    out = []
    for i, ln in enumerate(lines):
        m = RAN_IF.match(ln)
        if not m:
            continue
        call = None
        for j in range(i + 1, min(i + 6, len(lines))):
            # Every call on the line, not just the first: a one-line dispatch
            # reads `selftest::dispatch(..., acpi::self_test())`, whose first
            # match is the dispatcher. Taking only the first reported "no call
            # found" for two of the six real sites -- and a gate that quietly
            # skips what it cannot parse is the failure this one exists to end.
            code = strip_comment(lines[j])
            for c in CALL.finditer(code):
                if not c.group(1).startswith("selftest::"):
                    call = c.group(1)
                    break
            if call:
                break
        out.append((i + 1, m.group(1), call))
    return out


def analyse(anns, index):
    """[(line, marker, call, why)] -- one per annotation that does not hold."""
    findings = []
    for line, marker, call in anns:
        if call is None:
            findings.append((line, marker, None,
                             "no call on the lines after the annotation"))
            continue
        name = call.rsplit("::", 1)[1]
        cands = index.get(name, [])
        if not cands:
            findings.append((line, marker, call, "no `fn " + name + "` in the tree"))
            continue
        if any(marker in body for _, _, body in cands):
            continue
        elsewhere = sorted({
            other + " (" + d[0] + ":" + str(d[1]) + ")"
            for other, defs in index.items() for d in defs
            if other != name and marker in d[2]
        })
        findings.append((line, marker, call,
                         "not printed by " + name + "; printed by " +
                         (", ".join(elsewhere) if elsewhere else "nothing in the tree")))
    return findings


def run(root):
    main_rs = os.path.join(root, "kernel", "src", "main.rs")
    src = os.path.join(root, "kernel", "src")
    if not os.path.isfile(main_rs):
        sys.stderr.write("check-ran-if: cannot read " + main_rs + NL)
        return 2
    anns = annotations(main_rs)
    if not anns:
        sys.stderr.write(
            "check-ran-if: no RAN-IF annotations in main.rs. Either the convention "
            "moved or the comment format changed; either way this gate is blind and "
            "check-gated-selftests.py is trusting markers nobody is checking." + NL)
        return 2
    findings = analyse(anns, fn_bodies(src))
    print("check-ran-if: " + str(len(anns)) + " annotation(s) checked")
    for line, marker, call, why in findings:
        print("  kernel/src/main.rs:" + str(line) + "  " + (call or "<unparsed>"))
        print("      declares: " + marker)
        print("      " + why)
    if findings:
        print("check-ran-if: " + str(len(findings)) + " finding(s)")
        return 1
    print("check-ran-if: OK (every marker is printed by the call it annotates)")
    return 0


# --------------------------------------------------------------------------
# Self-test
# --------------------------------------------------------------------------

def _tree(tmp, main_body, extra=None):
    """Write a miniature kernel/src and return its root."""
    src = os.path.join(tmp, "kernel", "src")
    os.makedirs(src, exist_ok=True)
    io.open(os.path.join(src, "main.rs"), "w", encoding="utf-8",
            newline="").write(main_body)
    for name, text in (extra or {}).items():
        path = os.path.join(src, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        io.open(path, "w", encoding="utf-8", newline="").write(text)
    return tmp


def _fn(name, prints):
    body = ["pub fn " + name + "() -> Result<(), ()> {"]
    for p in prints:
        body.append('    serial_println!("' + p + '");')
    body.append("    Ok(())")
    body.append("}")
    return NL.join(body) + NL


def _findings(root):
    return analyse(annotations(os.path.join(root, "kernel", "src", "main.rs")),
                   fn_bodies(os.path.join(root, "kernel", "src")))


def self_test():
    failures = []

    def check(label, got, want):
        if got != want:
            failures.append(label + ": got " + repr(got) + ", want " + repr(want))

    M = "[x] Running self-test..."
    OTHER = "[x] Running other self-test..."

    with tempfile.TemporaryDirectory() as tmp:
        # 1. The marker is printed by the annotated call.
        root = _tree(os.path.join(tmp, "a"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    fs::x::self_test();",
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [M])})
        check("a matching annotation is clean", _findings(root), [])

        # 2. The real bug: printed by a *neighbour*, and the message says which.
        root = _tree(os.path.join(tmp, "b"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    fs::x::self_test();",
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [OTHER]) + _fn("format_self_test", [M])})
        f = _findings(root)
        check("a neighbour's banner is a finding", len(f), 1)
        if f:
            check("and the neighbour is named", "format_self_test" in f[0][3], True)

        # 3. Printed nowhere at all.
        root = _tree(os.path.join(tmp, "c"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    fs::x::self_test();",
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [OTHER])})
        f = _findings(root)
        check("a marker printed nowhere is a finding", len(f), 1)
        if f:
            check("and says so", "nothing in the tree" in f[0][3], True)

        # 4. The annotated function does not exist.
        root = _tree(os.path.join(tmp, "d"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    fs::x::gone();",
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [M])})
        f = _findings(root)
        check("a vanished callee is a finding", len(f), 1)

        # 5. A one-line dispatch: the callee is the *second* call on the line.
        #    Regression guard -- taking only the first match reported "no call
        #    found" for two real sites, which would have been a silent skip.
        root = _tree(os.path.join(tmp, "e"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    selftest::dispatch(" + QUOTE + "X" + QUOTE +
            ", selftest::Severity::Diagnostic, fs::x::self_test());",
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [M])})
        check("a one-line dispatch resolves its callee", _findings(root), [])

        # 6. The marker appearing only in the annotation must not satisfy it.
        #    `main.rs` contains the literal by construction; if comments counted,
        #    every site would verify itself.
        root = _tree(os.path.join(tmp, "f"), NL.join([
            "fn kmain() {",
            '    // RAN-IF: "' + M + '"',
            "    fs::x::self_test();",
            "}",
            "fn decoy() {",
            '    // serial_println!("' + M + '");',
            "}",
        ]) + NL, {"fs/x.rs": _fn("self_test", [OTHER])})
        f = _findings(root)
        check("a commented-out print does not count as evidence", len(f), 1)

        # 7. No annotations at all is no-verdict, not a pass. stderr is
        #    captured: this path prints a deliberately alarming message, and a
        #    passing gate must not look like a failing one in the boot log.
        root = _tree(os.path.join(tmp, "g"), "fn kmain() {}" + NL)
        held, sys.stderr = sys.stderr, io.StringIO()
        try:
            rc = run(root)
        finally:
            sys.stderr = held
        check("an empty convention exits 2", rc, 2)

    for f in failures:
        print("  FAIL " + f)
    if failures:
        print("check-ran-if --self-test: " + str(len(failures)) + " failure(s)")
        return 1
    print("check-ran-if --self-test: all 9 checks passed")
    return 0


def main(argv):
    if "--self-test" in argv:
        return self_test()
    root = os.environ.get("RANIF_ROOT") or os.path.dirname(
        os.path.dirname(os.path.abspath(__file__)))
    return run(root)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
