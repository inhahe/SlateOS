#!/usr/bin/env python3
"""Find options a program's own --help advertises but its parser never reads.

The mirror image of `unknown-option-sweep.py`. That one asks "does an
option we do not have get accepted?"; this asks "does an option we
advertise exist?". Both are the same disagreement between what a program
says about its command line and what it does with it.

It needs no reference implementation, which is what makes it worth having:
the program supplies both halves of the comparison itself. Two thirds of
the unknown-option findings are blocked on a reference environment this
session cannot install into; none of these are.

The rule: an option named in a help string is RECOGNISED if the crate's
source also holds it as a bare string literal (`"--priority"`), as a
valued prefix (`"--priority="`), or inside a `starts_with`/`strip_prefix`.
If the only place the text `--priority` occurs is the help itself, nothing
parses it.
"""

import importlib.util
import io
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402  (needs the path line above)

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# An option as it appears inside a help line: `--long`, `--long=VAL`, `-x`.
# `(?<!-)` so the pattern cannot start in the middle of a run of dashes.
# `vmstat` prints a banner `---timestamp---` and `wget` prints
# `---request begin---`; without the guard those yield options called
# `--timestamp` and `--request`, which is this tool's sixth false
# positive and the second one caused by reading decoration as documentation.
LONG = re.compile(r"(?<!-)--[a-z][a-z0-9_-]*")
SHORT = re.compile(r"(?<![-\w])-([A-Za-z0-9])(?=[,\s])")

# A getopt optstring or a bundle of `set` letters: letters only, plus the
# `:` and `+` getopt uses for argument and ordering markers.
LETTERSET = re.compile(r"^[A-Za-z:+]+$")

# A Rust string literal, non-greedy, no escapes handled -- help text has none.
STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')

# Only strings that look like a help/usage line are mined for options, so a
# match arm `"--all" => ...` is not mistaken for an advertisement.
HELPISH = re.compile(r"^\s*(-|Usage:|usage:|Options:|Commands:)")

# `#[cfg(test)] mod NAME;` -- a test module kept in a file of its own. Every
# line of that file is test code, but `rustlex.live_code` works within one file
# and cannot know it: the attribute is in the PARENT. `lsblk/src/tests.rs`,
# which checks the program's usage text against upstream's, was this gate's
# first false positive of the kind -- a fixture's copy of the help, read as
# a help the file itself failed to parse.
CFG_TEST_MOD = re.compile(
    r"#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?"
    r"mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;"
)


def test_module_files(files):
    """The files among `files` that are out-of-line `#[cfg(test)]` modules.

    A module's children live beside `main.rs`, `lib.rs` and `mod.rs`, and in a
    directory named after any other file (`src/bin/foo.rs` -> `src/bin/foo/`).
    """
    out = set()
    for f in files:
        try:
            raw = io.open(f, encoding="utf-8", errors="replace").read()
        except OSError:
            continue
        # Comments out, so a doc comment showing the declaration is not one.
        text = rustlex.strip_noise(raw)
        base = os.path.dirname(f)
        stem = os.path.splitext(os.path.basename(f))[0]
        moddir = base if stem in ("main", "lib", "mod") else os.path.join(base, stem)
        for m in CFG_TEST_MOD.finditer(text):
            name = m.group(1)
            out.add(os.path.normpath(os.path.join(moddir, name + ".rs")))
            out.add(os.path.normpath(os.path.join(moddir, name, "mod.rs")))
    return out


def advertised(text):
    """Options named in help-looking string literals, with the line each came from."""
    found = {}
    for m in STRING.finditer(text):
        lit = m.group(1)
        if not HELPISH.match(lit):
            continue
        # A help line documents something, so it has a description after the
        # option. A literal that is nothing but the option is a fragment
        # being *built*, not text being printed: `xdg` holds `"--icon "` and
        # pushes it onto a command line when it expands a Desktop Entry `%i`
        # field code. Tenth false positive.
        if len(lit.split()) < 2:
            continue
        # A usage synopsis like `[-RVadlp]` is a cluster, not a list of
        # separately-documented options; skip those bracketed runs.
        body = re.sub(r"\[[^\]]*\]", " ", lit)
        # Only the leading option field, not the description. Help lines are
        # laid out `  -r N              Retry N times (-1 = forever)`, and
        # the `-1` in that sentence is prose, not an option -- this tool's
        # fourth false positive, on `flock` of all things. Two or more
        # spaces separate the field from the prose.
        body = re.split(r"\s{2,}", body.strip(), maxsplit=1)[0]
        # A field naming a lowercase bare word is a synopsis, not a list of
        # documented options: `iptables` writes `-m match --opts`, where
        # `match` is a metavariable and `--opts` means "that match's own
        # options" rather than an option called `--opts`. Documented options
        # spell their metavariables in caps or in angle brackets. Eighth
        # false positive.
        if any(re.fullmatch(r"[a-z][a-z0-9_-]*", tok) for tok in body.split()):
            continue
        for opt in LONG.findall(body):
            found.setdefault(opt, lit.strip())
        for c in SHORT.findall(body):
            found.setdefault("-" + c, lit.strip())
    return found


def recognised(text, opt):
    """True if anything other than the help text could act on `opt`."""
    if '"%s"' % opt in text:
        return True
    if '"%s="' % opt in text:
        return True
    # ...and the accepted form may be one specific `name=value` literal
    # rather than a prefix: `pstree` matches `"--compact=no"` and nothing
    # else, so a search that requires the closing quote after `=` misses it.
    # Ninth false positive.
    if '"%s=' % opt in text:
        return True
    # `starts_with("--suffix=")` / `strip_prefix("--suffix=")`
    if re.search(r'(?:starts_with|strip_prefix)\(\s*"%s' % re.escape(opt), text):
        return True
    # A short option folded into a byte or char match: `b'R' =>` / `'R' =>`
    if len(opt) == 2:
        c = opt[1]
        # Any char or byte literal of that letter counts, not only one
        # immediately followed by `=>`. `coreutils/free` writes
        # `Opt::Short(b'b', _) | Opt::Long("bytes", _) =>`, where the byte
        # is followed by a comma -- this tool's third false positive. A
        # bare `'b'` somewhere unrelated would now hide a real finding,
        # which is the safer way to be wrong about an accusation.
        if re.search(r"b?'%s'" % re.escape(c), text):
            return True
        if '"%s"' % c in text:
            return True
        # A run of option letters held as one string rather than one literal
        # each: `oils` has `const SET_OPTION_LETTERS: &str =
        # "euxfaCnTEBmbhkptvHP"`, and a getopt optstring like `"abc:"` is the
        # same shape. All five `set` letters it was accused of are in there.
        # Seventh false positive.
        for lit in STRING.findall(text):
            if len(lit) >= 4 and LETTERSET.match(lit) and c in lit:
                return True
        return False
    # Some parsers strip the dashes before matching, and then the literal is
    # the bare name: `coreutils/cal` holds `("help", Takes::Nothing)` and
    # `"help" => Code::Help`, so a search for `"--help"` finds nothing and
    # the option is nonetheless implemented. That was this tool's second
    # false positive. Bare-name matching can in principle collide with an
    # unrelated string, which costs a false *negative* -- the safer
    # direction for a tool whose output is a list of accusations.
    bare = opt.lstrip("-")
    if '"%s"' % bare in text:
        return True
    # ...and the de-dashed form may carry its `=`, because the parser has
    # already split the value off: `objdump` holds
    # `rest.strip_prefix("start-address=")`, so neither `"--start-address"`
    # nor `"start-address"` occurs and the option works perfectly. This
    # tool's fifth false positive.
    if '"%s="' % bare in text:
        return True
    if re.search(r'(?:starts_with|strip_prefix)\(\s*"%s' % re.escape(bare), text):
        return True
    return False


SELFTEST_SRC = """
//! A doc comment naming `--from-a-comment` must not count as advertising.
fn help() {
    println!("Usage: demo [options]");
    println!("  -r, --real       an option the parser reads");
    println!("  -g, --ghost      an option nothing reads");
}
fn parse(a: &str) {
    match a {
        "--real" => {}
        _ => {}
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        let _ = "--ghost";
    }
}
"""


def selftest():
    """Prove it finds a ghost, spares a real option, and ignores comments."""
    live, _t = rustlex.live_code(SELFTEST_SRC)
    text = rustlex.strip_noise(live, keep_literals=True)
    ads = advertised(text)
    failures = 0

    if "--ghost" in ads and "--real" in ads:
        print("  ok    both documented options are seen as advertised")
    else:
        print("  FAIL  advertised set was %r" % sorted(ads))
        failures += 1

    if "--from-a-comment" not in ads:
        print("  ok    an option named only in a comment is not an advertisement")
    else:
        print("  FAIL  a comment was read as help text")
        failures += 1

    if recognised(text, "--real"):
        print("  ok    an option the parser matches is recognised")
    else:
        print("  FAIL  a real match arm was missed")
        failures += 1

    if not recognised(text, "--ghost"):
        print("  ok    an option only the tests mention is not recognised")
    else:
        print("  FAIL  test-only text counted as an implementation")
        failures += 1

    if recognised(SELFTEST_SRC, "--ghost"):
        print("  ok    ...but it is still found in the raw source, so the")
        print("        report can say 'only in tests' rather than 'nowhere'")
    else:
        print("  FAIL  raw-source check did not see the test mention")
        failures += 1

    import tempfile

    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, "src")
        os.makedirs(os.path.join(src, "bin"))
        parts = {
            "main.rs": "mod util;\n#[cfg(test)]\nmod tests;\n// #[cfg(test)] mod doc;\n",
            "bin/tool.rs": "#[cfg(test)]\n#[allow(clippy::all)]\nmod checks;\n",
        }
        for rel, body in parts.items():
            with io.open(os.path.join(src, rel), "w", encoding="utf-8", newline="\n") as fh:
                fh.write(body)
        got = test_module_files([os.path.join(src, "main.rs"), os.path.join(src, "bin", "tool.rs")])
        want_in = [os.path.join(src, "tests.rs"), os.path.join(src, "bin", "tool", "checks.rs")]
        want_out = [os.path.join(src, "util.rs"), os.path.join(src, "doc.rs")]
        if all(os.path.normpath(p) in got for p in want_in) and not any(
            os.path.normpath(p) in got for p in want_out
        ):
            print("  ok    an out-of-line #[cfg(test)] module is test code, and")
            print("        neither an ordinary module nor a commented one is")
        else:
            print("  FAIL  test_module_files gave %r" % sorted(got))
            failures += 1

    print("selftest: %d failure(s)" % failures)
    return 1 if failures else 0


def main():
    if any(a in ("--selftest", "--self-test", "--self_test") for a in sys.argv[1:]):
        return selftest()
    roots = [a for a in sys.argv[1:] if not a.startswith("-")] or ["userspace"]
    files = []
    for root in roots:
        for base, _dirs, names in os.walk(root):
            for n in names:
                if n.endswith(".rs"):
                    files.append(os.path.join(base, n))
    files.sort()
    test_files = test_module_files(files)

    total, liars = 0, []
    for f in files:
        if os.path.normpath(f) in test_files:
            continue
        try:
            raw = io.open(f, encoding="utf-8", errors="replace").read()
        except OSError:
            continue
        # Comments out, string literals kept: a doc comment showing an example
        # command line (`b"--verbose,-d,--port=8080"` in services/init) is not
        # a help text advertising options, and reading it as one was this
        # tool's first false positive. Test code out too -- a fixture's help
        # string documents the fixture, not the program.
        live, _tests = rustlex.live_code(raw)
        text = rustlex.strip_noise(live, keep_literals=True)
        ads = advertised(text)
        if not ads:
            continue
        total += 1
        missing = []
        for o, line in sorted(ads.items()):
            if recognised(text, o):
                continue
            # Distinguish "nothing anywhere parses this" from "only the
            # tests or a comment mention it". The second is still a defect
            # -- the binary does not have the option -- but it is a
            # different story about how it got that way.
            where = "only in tests/comments" if recognised(raw, o) else "nowhere"
            missing.append((o, line, where))
        if missing:
            liars.append((f, missing))

    print("help-vs-parser sweep")
    print("  files with help text: %d" % total)
    print("  files that advertise something unparsed: %d" % len(liars))
    n = sum(len(m) for _f, m in liars)
    # "never PARSED", not "never read". `recognised()` asks whether the option
    # reaches the parser at all; it says nothing about whether the field the
    # parser writes is ever looked at again. Those are different defects and
    # this gate only sees the first.
    #
    # The label used to read "never read", and a zero on that line invited
    # exactly the wrong conclusion: on 2026-09-15 this sweep reported 0 while
    # `lscpu` alone had FIVE options that were parsed, stored and read by
    # nothing -- `-e`, `-p`, `--hex`, `--online`, `--offline`. Six more had
    # been fixed across `patch`, `curl`, `tee`, `pstree` and `gdb` in the same
    # session. The check was right and its summary line was not.
    print("  options advertised but never parsed: %d" % n)
    print(
        "  (parsed-but-never-read is a different defect this gate cannot see:"
        " check-fields-written-never-read.py --advertised)"
    )
    for f, missing in liars[:40]:
        print("  %s" % f.replace(os.sep, "/"))
        for o, line, where in missing[:6]:
            print("      %-20s %-22s from: %s" % (o, where, line[:48]))
    return 1 if liars else 0


if __name__ == "__main__":
    sys.exit(main())
