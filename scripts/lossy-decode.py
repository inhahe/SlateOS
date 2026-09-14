#!/usr/bin/env python3
"""Find lossy byte->text conversions that reach a VALUE, not a message.

`String::from_utf8_lossy` and `OsStr::to_string_lossy` replace every byte they
cannot decode with U+FFFD. CLAUDE.md names the first of them outright -- "No
`from_utf8_lossy` -- that's silent data corruption" -- and on this OS the rule
bites hard, because a file name may hold every byte except `/` and NUL
(design.txt).

WHAT IT COST, in the one case that has been fixed so far:

    diff.rs   `diff_dirs` passed `p1.to_string_lossy()` to `diff_files`, so a
              directory walk decoded the names it had just listed and then
              could not open them. `diff -r` answered `No such file or
              directory` for a file it had itself enumerated. GNU diffs it.

WHY A GREP IS THE WRONG INSTRUMENT, measured rather than asserted. A plain
count of the two names across the 72 binaries on the image gives **131**. The
number that means anything is far smaller, and the difference is four
categories that are not defects at all:

  * 85 of them are inside `#[cfg(test)]`, where a lossy decode formats an
    assertion message;
  * a `#[cfg(not(unix))]` fallback is the WINDOWS HOST path -- the target
    compiles the `as_bytes()` branch beside it, so it cannot corrupt anything
    that runs on SlateOS. `quoting::os_bytes` documents this shape and several
    bins carry a private copy of it;
  * a decode that feeds `format!`/`panic!`/`Err(...)` is producing a MESSAGE.
    Rendering an undecodable name in a diagnostic is a display question, not a
    corruption one -- `quoting::quotef` is the better answer, but it is a
    different defect and a much smaller one;
  * a comment that merely names the function is not a call.

Ranking what is left by intuition does not work either, and that is worth
recording because it was tried: `sed` (25 raw) and `realpath` (9 raw) were
named as the two to open first, on the reasoning that both are "fundamentally
about transforming text and paths". `realpath` has ZERO outside its tests and
`sed` has one, in an error message. The bin that actually had the defect was
`diff`.

WHAT THE DEFAULT RUN DOES NOT COVER, learned by planting a probe in `cat.rs`
and watching the checker not notice. The default scope is
`scripts/rootfs-bin-manifest.txt`, which is what the IMAGE ships -- and that
list deliberately omits the thirteen names fastpy's compiled utilities own
(`cat`, `ls`, `grep`, `sort`, `wc` and the rest, per design-decisions.md 108).
Their Rust implementations exist in this tree and are NOT scanned by default,
because they are not on the image. `--all` covers them, along with every other
crate under `userspace/`, and reports a much larger and wholly unaudited
number.

    python scripts/lossy-decode.py                # the image's binaries
    python scripts/lossy-decode.py --all          # every crate under userspace
    python scripts/lossy-decode.py --selftest
"""

import argparse
import io
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

CALLS = ("from_utf8_lossy", "to_string_lossy", "to_str_lossy")

# A decode whose result is going into one of these is producing text for a
# human to read, not a value for the program to act on.
DIAGNOSTIC = re.compile(
    r"format!|panic!|diag!|eprintln!|println!|write!|writeln!|"
    r"unreachable!|assert|expect\(|Err\(|map_err|fatal|Fail::|"
    r"\.detail|usage|help|warn"
)

NL = chr(10)

# Sites that ARE a lossy decode reaching a value, and are still correct. Each
# is anchored on a substring of the line rather than a line number, so that
# editing the line re-opens the question -- which is the point: the exemption
# is for the code as audited, not for the file forever.
#
# This table records WHY. There is deliberately no baseline file recording
# merely THAT, which is the split `scripts/raced-globals-baseline.txt` states
# in its own header: "A genuine false positive belongs in the IGNORE table in
# the script, which records *why*, not here, which records only *that*."
#
# Audited 2026-09-14, when the image's raw count of 131 came down to these six.
IGNORE = (
    ("stat", "from_utf8_lossy(TERSE_FILE)",
     "TERSE_FILE is a const format string in this file; ASCII by construction"),
    ("stat", "from_utf8_lossy(TERSE_FS)",
     "TERSE_FS likewise"),
    ("expr", "BigInt::from_str(&String::from_utf8_lossy(v))",
     "guarded by looks_like_integer(v), which admits only ASCII digits and a "
     "leading `-`, so the decode is provably lossless"),
    ("od", "let typed = String::from_utf8_lossy(typed_bytes)",
     "decoded only to MATCH a long-option name, all of which are ASCII; the "
     "raw bytes are passed alongside and are what the diagnostic uses"),
    ("split", "let shown = String::from_utf8_lossy(name)",
     "`shown` reaches only println!/format!; the child's FILE= environment "
     "variable is set from os_from_bytes(name) on the next line"),
    ("strings", "let text = String::from_utf8_lossy(&bytes)",
     "`text` reaches only STRINGS.usage(format!(...))"),
)


def ignored(name, text):
    """Whether this site is in the audited-exempt table."""
    for bin_name, anchor, _why in IGNORE:
        if bin_name == name and anchor in text:
            return True
    return False


def strip_comments(text):
    """Blank out comments, keeping line numbering intact.

    A comment that names `from_utf8_lossy` -- this file's own docstring does
    it, and so do three of the binaries -- is not a call. Replacing rather
    than deleting keeps every reported line number the one a reader will find
    in their editor.
    """
    out = []
    in_block = False
    for line in text.split(NL):
        if in_block:
            end = line.find("*/")
            if end < 0:
                out.append("")
                continue
            line = " " * (end + 2) + line[end + 2:]
            in_block = False
        # A `/* ... */` that CLOSES on the same line leaves the rest of the
        # line live -- the self-test pins this, because the first draft threw
        # the remainder away and so hid any call after an inline comment.
        while True:
            start = line.find("/*")
            if start < 0:
                break
            end = line.find("*/", start + 2)
            if end < 0:
                in_block = True
                line = line[:start]
                break
            line = line[:start] + " " * (end + 2 - start) + line[end + 2:]
        # `//` outside a string literal. Good enough here: the false positive
        # is a `//` inside a string, which would only ever HIDE a call, and a
        # hidden call is caught by the self-test rather than shipped.
        at = line.find("//")
        if at >= 0:
            line = line[:at]
        out.append(line)
    return NL.join(out)


def production_part(text):
    """`text` up to its test module, and the line it was cut at."""
    at = text.find("#[cfg(test)]")
    if at < 0:
        return text, None
    return text[:at], text[:at].count(NL) + 1


def enclosing_fn(lines, at):
    """The index of the `fn` line enclosing `at`, or None."""
    for i in range(at, -1, -1):
        if re.match(r"\s*(pub\s+)?(const\s+|async\s+|unsafe\s+|extern\s+\S+\s+)*fn\s", lines[i]):
            return i
    return None


def is_host_only(lines, fn_at):
    """Whether the function at `fn_at` is compiled only off-target.

    The `#[cfg(not(unix))]` half of a pair like `quoting::os_bytes` never runs
    on SlateOS -- the target is `unix` -- so a lossy decode inside it cannot
    corrupt anything the OS does. It is a developing-on-Windows concession and
    is documented as one.
    """
    for i in range(fn_at - 1, max(-1, fn_at - 6), -1):
        line = lines[i].strip()
        if not line.startswith("#["):
            if line and not line.startswith("///"):
                break
            continue
        if "cfg(not(unix))" in line or "cfg(windows)" in line:
            return True
    return False


def enclosing_cfgs(lines, at):
    """Every `#[cfg(...)]` on a block or item that ENCLOSES line `at`.

    An attribute on the function is only one of the two shapes this has to
    catch. The other is a bare block inside a function:

    ```text
    #[cfg(unix)]
    { PathBuf::from(OsStr::from_bytes(bytes)) }
    #[cfg(not(unix))]
    { PathBuf::from(String::from_utf8_lossy(bytes).into_owned()) }
    ```

    `test.rs` writes `path_of` exactly that way, and the first draft of this
    checker reported the second block as a VALUE defect -- a false positive on
    the one file whose shape most resembled the real bug it was built to find.
    Walking out by brace depth catches both, and does not catch a cfg block
    that has already CLOSED above the call.
    """
    found = []
    depth = 0
    for i in range(at, -1, -1):
        depth += lines[i].count("}") - lines[i].count("{")
        if depth < 0:
            for j in (i, i - 1, i - 2):
                if j >= 0 and "#[cfg(" in lines[j]:
                    found.append(lines[j])
                    break
            depth = 0
    return found


def classify(lines, at):
    """`HOST`, `DIAG` or `VALUE` for the call on line index `at`."""
    fn_at = enclosing_fn(lines, at)
    if fn_at is not None and is_host_only(lines, fn_at):
        return "HOST"
    for cfg in enclosing_cfgs(lines, at):
        if "cfg(not(unix))" in cfg or "cfg(windows)" in cfg:
            return "HOST"
    # The statement around the call, not a fixed window: a `format!` opening
    # four lines above its argument is still that argument's destination.
    lo = at
    while lo > 0 and not re.search(r"[;{}]\s*$", lines[lo - 1]):
        lo -= 1
        if at - lo > 12:
            break
    hi = at
    while hi < len(lines) - 1 and not re.search(r"[;{}]\s*$", lines[hi]):
        hi += 1
        if hi - at > 12:
            break
    if DIAGNOSTIC.search(NL.join(lines[lo:hi + 1])):
        return "DIAG"

    # A BINDING is followed to its uses. `let shown = from_utf8_lossy(name);`
    # printed three lines later is a message, and the statement window above
    # cannot see that. Five of the nine sites this checker first reported were
    # exactly that shape -- including `split`'s, where the binding is printed
    # while the real bytes go to the child's environment beside it.
    m = re.match(r"\s*let\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?::[^=]*)?=", lines[at])
    if m:
        name = m.group(1)
        fn_at = enclosing_fn(lines, at) or 0
        end = fn_at
        depth = 0
        seen_open = False
        for i in range(fn_at, len(lines)):
            depth += lines[i].count("{") - lines[i].count("}")
            if lines[i].count("{"):
                seen_open = True
            if seen_open and depth <= 0:
                end = i
                break
        else:
            end = len(lines) - 1
        uses = [l for l in lines[at + 1:end + 1]
                if re.search(r"\b" + re.escape(name) + r"\b", l)]
        if uses and all(DIAGNOSTIC.search(u) for u in uses):
            return "DIAG"
    return "VALUE"


def scan(path, name=None):
    """Every lossy call in `path`, as `(line_number, kind, text)`.

    A site in [`IGNORE`] is reported as `OK` rather than dropped, so that
    `--show all` still shows it and an exemption cannot quietly cover a line
    that has since changed -- the anchor stops matching and the site comes
    back as a VALUE.
    """
    raw = io.open(path, encoding="utf-8", errors="replace").read()
    prod, _ = production_part(raw)
    prod = strip_comments(prod)
    lines = prod.split(NL)
    found = []
    for i, line in enumerate(lines):
        if any(c in line for c in CALLS):
            kind = classify(lines, i)
            if kind == "VALUE" and name and ignored(name, line):
                kind = "OK"
            found.append((i + 1, kind, line.strip()[:74]))
    return found


def image_binaries():
    """The binaries the image actually ships, from the rootfs manifest."""
    names = []
    p = os.path.join(ROOT, "scripts", "rootfs-bin-manifest.txt")
    for line in io.open(p, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#") and "=" not in line:
            names.append(line)
    paths = []
    base = os.path.join(ROOT, "userspace", "coreutils", "src", "bin")
    for n in names:
        for cand in (os.path.join(base, n + ".rs"),
                     os.path.join(base, n, "main.rs")):
            if os.path.isfile(cand):
                paths.append((n, cand))
                break
    return paths


def all_sources():
    out = []
    for dirpath, dirnames, filenames in os.walk(os.path.join(ROOT, "userspace")):
        dirnames[:] = [d for d in dirnames if d != "target"]
        for f in filenames:
            if f.endswith(".rs"):
                full = os.path.join(dirpath, f)
                out.append((os.path.relpath(full, ROOT), full))
    return out


def selftest():
    bad = 0
    checks = 0

    def ck(ok, msg):
        nonlocal bad, checks
        checks += 1
        if not ok:
            print("selftest FAIL: " + msg, file=sys.stderr)
            bad += 1

    def kinds(src):
        src = strip_comments(production_part(src)[0])
        lines = src.split(NL)
        return [classify(lines, i) for i, l in enumerate(lines)
                if any(c in l for c in CALLS)]

    # A plain value conversion is the thing this exists to find.
    ck(kinds("fn f() {" + NL + "    let p = x.to_string_lossy();" + NL + "}") == ["VALUE"],
       "a bare conversion is a VALUE")

    # ...and the real defect's shape, which was an argument to a call.
    ck(kinds("fn f() {" + NL + "    g(&p.to_string_lossy(), 1);" + NL + "}") == ["VALUE"],
       "a conversion passed as an argument is a VALUE")

    # A message is not corruption.
    ck(kinds("fn f() {" + NL + '    panic!("{}", String::from_utf8_lossy(v));' + NL + "}") == ["DIAG"],
       "a panic argument is a DIAG")

    # The destination may be several lines above the argument, which is why
    # this looks at the statement rather than a fixed window.
    multi = ("fn f() {" + NL
             + "    return Err(format!(" + NL
             + '        "bad thing: {}",' + NL
             + "        String::from_utf8_lossy(&value)" + NL
             + "    ));" + NL + "}")
    ck(kinds(multi) == ["DIAG"], "a format! four lines up still claims its argument")

    # The off-target half of a pair cannot corrupt the target.
    host = ("#[cfg(not(unix))]" + NL
            + "fn os_from_bytes(b: &[u8]) -> OsString {" + NL
            + "    OsString::from(String::from_utf8_lossy(b).into_owned())" + NL + "}")
    ck(kinds(host) == ["HOST"], "a cfg(not(unix)) fallback is HOST")

    # The other shape of the same thing: a bare cfg BLOCK inside a function,
    # which is how `test.rs` writes `path_of`. The first draft of this checker
    # called that a defect.
    blockform = ("fn path_of(bytes: &[u8]) -> PathBuf {" + NL
                 + "    #[cfg(unix)]" + NL
                 + "    {" + NL
                 + "        PathBuf::from(OsStr::from_bytes(bytes))" + NL
                 + "    }" + NL
                 + "    #[cfg(not(unix))]" + NL
                 + "    {" + NL
                 + "        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())" + NL
                 + "    }" + NL + "}")
    ck(kinds(blockform) == ["HOST"], "a cfg(not(unix)) BLOCK is HOST")

    # ...and a cfg block that has CLOSED above the call does not cover it.
    closed = ("fn f(b: &[u8]) {" + NL
              + "    #[cfg(not(unix))]" + NL
              + "    {" + NL
              + "        let _ = 1;" + NL
              + "    }" + NL
              + "    let p = String::from_utf8_lossy(b).into_owned();" + NL
              + "    use_it(p);" + NL + "}")
    ck(kinds(closed) == ["VALUE"], "a cfg block that already closed does not cover the call")

    # ...but the unix half of the same pair is not exempt by association.
    both = ("#[cfg(unix)]" + NL
            + "fn a(b: &[u8]) -> OsString {" + NL
            + "    OsString::from(String::from_utf8_lossy(b).into_owned())" + NL + "}")
    ck(kinds(both) == ["VALUE"], "a cfg(unix) function is NOT exempt")

    # A binding used only in messages is a message, however far away.
    bound = ("fn f(name: &[u8]) {" + NL
             + "    let shown = String::from_utf8_lossy(name).into_owned();" + NL
             + "    if verbose {" + NL
             + '        println!("executing with FILE={shown}");' + NL
             + "    }" + NL
             + "    go();" + NL + "}")
    ck(kinds(bound) == ["DIAG"], "a binding used only in a println is a DIAG")

    # ...and a binding that reaches real work is still a VALUE. Without this
    # the rule above would excuse every decode that is ALSO printed.
    both_uses = ("fn f(name: &[u8]) {" + NL
                 + "    let shown = String::from_utf8_lossy(name).into_owned();" + NL
                 + '    println!("{shown}");' + NL
                 + "    open_the_file(&shown);" + NL + "}")
    ck(kinds(both_uses) == ["VALUE"], "a binding that also reaches real work is a VALUE")

    # A comment naming the call is not a call. This file's own docstring would
    # otherwise report itself.
    ck(kinds("fn f() {" + NL + "    // from_utf8_lossy would be wrong here" + NL + "}") == [],
       "a comment mentioning the name is not a call")
    ck(kinds("fn f() {" + NL + "    let x = 1; // to_string_lossy" + NL + "}") == [],
       "a trailing comment is not a call")

    # Test code is out of scope: a lossy decode in an assertion message is not
    # a defect, and 85 of the 131 raw hits on the image are exactly that.
    intest = ("fn f() {}" + NL + "#[cfg(test)]" + NL + "mod tests {" + NL
              + "    fn g() { let s = String::from_utf8_lossy(v); }" + NL + "}")
    ck(kinds(intest) == [], "a call inside the test module is not counted")

    # A block comment spanning lines must not swallow real code after it.
    blk = ("fn f() {" + NL + "    /* from_utf8_lossy */ let p = x.to_string_lossy();" + NL + "}")
    ck(kinds(blk) == ["VALUE"], "code after a block comment is still scanned")

    # The manifest must resolve to real files, or this grades nothing.
    bins = image_binaries()
    ck(len(bins) > 50, "the image manifest should resolve to most of its binaries, got "
       + str(len(bins)))

    print("selftest: " + str(checks - bad) + "/" + str(checks) + " cases pass")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", "--self-test", dest="selftest", action="store_true")
    ap.add_argument("--all", action="store_true",
                    help="every .rs under userspace/, not just the image's binaries")
    ap.add_argument("--show", choices=("value", "diag", "host", "ok", "all"),
                    default="value")
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    targets = all_sources() if args.all else image_binaries()
    totals = {"VALUE": 0, "DIAG": 0, "HOST": 0, "OK": 0}
    per_bin = []
    for name, path in targets:
        hits = scan(path, name)
        if not hits:
            continue
        counts = {"VALUE": 0, "DIAG": 0, "HOST": 0, "OK": 0}
        for _, kind, _text in hits:
            counts[kind] += 1
            totals[kind] += 1
        per_bin.append((name, counts, hits))

    want = {"value": ("VALUE",), "diag": ("DIAG",), "host": ("HOST",),
            "ok": ("OK",),
            "all": ("VALUE", "DIAG", "HOST", "OK")}[args.show]
    per_bin.sort(key=lambda r: -sum(r[1][k] for k in want))
    for name, counts, hits in per_bin:
        if not sum(counts[k] for k in want):
            continue
        print(name + ":")
        for ln, kind, text in hits:
            if kind in want:
                print("    %-6s line %-5d %s" % (kind, ln, text))

    # A SCAN THAT READ ALMOST NOTHING MUST NOT REPORT SUCCESS. Zero findings
    # is the expected state here, so "clean" and "the manifest resolved to
    # nothing" produce the same number -- and only this tells them apart. The
    # sibling checkers carry the same guard for the same reason.
    if len(targets) < 40:
        print("lossy-decode: REFUSING -- only %d source file(s) found, which is "
              "too few to conclude anything. Is the manifest readable?"
              % len(targets), file=sys.stderr)
        return 2

    print("")
    print("VALUE %d   DIAG %d   HOST %d   OK %d   (in %d file(s), %d scanned)"
          % (totals["VALUE"], totals["DIAG"], totals["HOST"], totals["OK"],
             len(per_bin), len(targets)))
    print("")
    print("VALUE is the only column that means 'silent corruption'. DIAG is a")
    print("name rendered into a message -- `quoting::quotef` is the better")
    print("answer there, but it is a display defect, not a data one. HOST is")
    print("the `cfg(not(unix))` half of a pair whose unix half the target")
    print("compiles instead, so it never runs on SlateOS. OK is a VALUE site")
    print("audited and exempted in the script's IGNORE table, which records")
    print("why; edit the line and the anchor stops matching, so the exemption")
    print("expires with the code it was granted for.")
    if totals["VALUE"]:
        print("", file=sys.stderr)
        print("A VALUE site is a byte sequence being turned into text with "
              "U+FFFD in place of whatever could not be decoded, and then "
              "USED. On this OS a file name may hold every byte but `/` and "
              "NUL, so that is a name nobody can open.", file=sys.stderr)
        print("If it is genuinely safe -- an ASCII constant, a guarded "
              "decode -- add it to IGNORE in the script WITH THE REASON, "
              "not to a list that records only that it exists.", file=sys.stderr)
    return 1 if totals["VALUE"] else 0


if __name__ == "__main__":
    sys.exit(main())
