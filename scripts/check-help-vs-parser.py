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


def advertised(text):
    """Options named in help-looking string literals, with the line each came from."""
    found = {}
    for m in STRING.finditer(text):
        lit = m.group(1)
        if not HELPISH.match(lit):
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

    total, liars = 0, []
    for f in files:
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
    print("  options advertised but never read: %d" % n)
    for f, missing in liars[:40]:
        print("  %s" % f.replace(os.sep, "/"))
        for o, line, where in missing[:6]:
            print("      %-20s %-22s from: %s" % (o, where, line[:48]))
    return 1 if liars else 0


if __name__ == "__main__":
    sys.exit(main())
