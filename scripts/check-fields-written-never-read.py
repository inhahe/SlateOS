#!/usr/bin/env python3
"""Struct fields a program computes in production and only its tests ever read.

WHY THIS EXISTS. `dead_code` warns about a field nothing reads -- but a
`#[cfg(test)]` module is a read, so a field the production code fills in and
never looks at again compiles clean, is covered, and is green. It is the same
blind spot `check-tested-but-uncalled.py` covers for functions, one level down,
and it hides a particular kind of user-facing defect: **work the program does
and then throws away.**

Two found on the day this was written:

  * `apps/fileassoc`'s `last_export`. The Export button rendered the
    associations into this `String`, put "Exported 8 association(s)" in the
    status bar, and stopped. Nothing displayed the field and nothing wrote a
    file. The message was true about a variable and false about the world.
  * `apps/explorer`'s `eta_secs`. `OperationProgress::update_rates` computed a
    time remaining on every tick from the moment the executor was written. The
    only thing that ever read it was its own test, so the user watched a
    progress bar with no way to tell ten seconds from ten minutes.

WHAT IT REPORTS. A field declared outside a test module, assigned at least once
in production, never read in production, and read at least once from a test.

WHAT IT DOES NOT REPORT, deliberately:

  * a field nothing reads at all -- `dead_code` already says so;
  * a field read anywhere in production, however far from where it is written;
  * anything listed in the baseline beside this file.

THE INVERSE QUESTION DOES NOT WORK, and it is worth saying so here because it
is the obvious next idea. "Fields *read* in production that only a test ever
writes" would find settings stuck at their default -- a control that exists,
is applied, and cannot be changed. Tried on 2026-09-14: **249 hits, almost all
artifact.**

The reason is an asymmetry in Rust rather than in this script. Reads are
overwhelmingly `.field` accesses, which is why the forward direction works.
Writes are overwhelmingly *struct literals* -- `yellow: LIGHT_YELLOW`,
`prevent_close: true` -- which a `.field =` matcher cannot see at all. So every
value built by construction looks unwritten. `Palette::yellow` came back as
"read 313 times, written only by tests".

The one promising hit, `gui/remote`'s `prevent_close`/`prevent_move`/
`prevent_resize`, turned out to be documented already:
`TD-C-TWELVE-OF-SEVENTEEN-WINDOW-RULE-ACTIONS-HAVE-NOWHERE-TO-GO`, which also
explains why it is deliberate. So the inverse sweep's best signal was a finding
someone had already written down, and its other 248 were noise.

HOW IT SEES A READ, AND THE MISTAKE THAT MATTERS. The first version matched
`self.field` only. Every finding vanished, including the two above -- because a
test reaches a field through the variable it built (`ui.last_export`), never
through `self`. A detector for "read only by tests" that cannot see a test read
reports zero on any tree, for ever, and that zero is indistinguishable from a
clean one. It matches any receiver now, and skips `.name(` as a method call.

That failure was caught by pointing it at a file where the bug was known to
exist rather than at the tree, which is also why `--self-test` exists.

Usage:
    python scripts/check-fields-written-never-read.py        # exit 1 on new
    python scripts/check-fields-written-never-read.py --list # all, exit 0
    python scripts/check-fields-written-never-read.py --self-test
"""

import collections
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import lanec_scan  # noqa: E402  (needs the path above)
import selftestflag  # noqa: E402

BASELINE = (
    pathlib.Path(__file__).resolve().parent / "fields-written-never-read-baseline.txt"
)

FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*[A-Za-z_&<(\[]")
STRUCT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Z]\w*)")
ACCESS = re.compile(r"\.([a-z_][a-z0-9_]*)")
# `let Self { title, completed_at, .. } = self;` reads every name it binds,
# and not one of them is written `.field`. Without this, a struct that
# serialises itself by destructuring looks like a struct nobody reads --
# `apps/reminders` writes `completed_at` into its JSON exactly that way and was
# reported as throwing it away.
DESTRUCTURE_OPEN = re.compile(r"^\s*let\s+(?:Self|[A-Z]\w*)\s*\{")
BOUND_NAME = re.compile(r"^\s*([a-z_][a-z0-9_]*)\s*,?\s*$")


# A backstop on how far above an assignment to look for the option literals
# that guard it. The real bound is structural -- see `_arm_above` -- and this
# only stops a runaway on a file that has no arm boundary at all.
ADVERTISED_WINDOW = 12

# The line that OPENS a branch. Scanning upward from an assignment stops after
# the first of these, because that is the arm the assignment belongs to.
_ARM = re.compile(r"(^|\})\s*(else\s+if|if|match)\b")

_OPTION = re.compile(r"--[a-z][a-z0-9-]+")


def _advertising_spans(line):
    """The parts of a source line a USER could read as advertising an option.

    Two things count: a double-quoted string literal, because help text is
    built out of them (`text.push_str("  -l  --ignore-whitespace  ...")`), and
    a `//!` module doc, because these programs put their usage block there.

    A `//` or `///` comment does NOT count. It documents the code to the next
    reader rather than the option to the user, and treating the two alike is
    exactly how a field whose only nearby mention is `// --foo is inert` gets
    reported as advertised -- which would invert the finding, since an option
    documented as inert is the honest case.
    """
    stripped = line.lstrip()
    if stripped.startswith("//!"):
        return [stripped[3:]]
    if stripped.startswith("//"):
        return []
    out = []
    i = 0
    while True:
        a = line.find('"', i)
        if a < 0:
            return out
        b = line.find('"', a + 1)
        while b > 0 and line[b - 1] == "\\":
            b = line.find('"', b + 1)
        if b < 0:
            return out
        out.append(line[a + 1 : b])
        i = b + 1


_OPTIONISH = re.compile(r"-{1,2}[A-Za-z0-9][A-Za-z0-9-]*")


def _describes(span, opt):
    """Does this span EXPLAIN the option, rather than merely name it?

    The PARSER names it too: `if a == "-z" || a == "--zeta" {` holds the
    spelling in a string literal every bit as much as the help text does, so
    without this every parsed option would count as advertised and the flag
    would report nothing useful. What separates them is that help text says
    something ABOUT the option, so a span counts only if removing every option
    token from it leaves real words behind.

    Found by the self-test below before this shipped, which is the whole
    argument for lane C having asked for one.
    """
    rest = _OPTIONISH.sub(" ", span.replace(opt, " "))
    return sum(ch.isalpha() for ch in rest) >= 3


def _arm_above(lines, assign_idx, window=ADVERTISED_WINDOW):
    """The lines from the enclosing branch down to an assignment, inclusive.

    Scanning upward a FIXED number of lines does not work, and the failure is
    visible rather than theoretical: `patch.rs` parses `-Z` directly beneath
    `-f`, so four lines above `opts.set_utc = true;` reaches into the previous
    arm and reports `set_utc` as advertised by `--force`.

    The bound that does work is structural: stop after the first line that
    OPENS a branch, because that is the arm this assignment belongs to. It
    handles both the one-line shape and the several-line one --

        } else if a == "-Z" || a == "--set-utc" {
            opts.set_utc = true;

        } else if a == "-F" || a == "--fuzz" {
            i = i.saturating_add(1);
            match args.get(i).and_then(...) {
                Some(v) => opts.fuzz = Some(v),

    -- which no single line count does, the first needing 1 and the second 5.
    """
    lo = max(0, assign_idx - window)
    for i in range(assign_idx, lo - 1, -1):
        if _ARM.search(lines[i]) and i != assign_idx:
            return lines[i : assign_idx + 1]
    return lines[lo : assign_idx + 1]


def advertised_for(lines, field, window=ADVERTISED_WINDOW):
    """Option spellings assigned into `field` that the program also advertises.

    Returns `(option, assigned_line, advertised_line)` triples, 1-based.

    **This is a heuristic and the report says so.** The association is
    positional: it reads the enclosing parser arm above each ASSIGNMENT to the
    field, looking for the option literals that guard it -- the
    `} else if a == "-l" || a == "--ignore-whitespace" {` shape this tree
    parses arguments with. A looser rule collects whatever option happens to be
    parsed nearby; `finger`'s `match_real_name` came back associated with
    `--help` and `--version` under a six-line window around every MENTION
    rather than every assignment.

    A false association can only ever promote a row, never demote one, which is
    why this belongs behind a flag rather than in the default output: the
    findings it cannot classify keep their standing.
    """
    advertised = {}
    for n, line in enumerate(lines, 1):
        for span in _advertising_spans(line):
            for opt in _OPTION.findall(span):
                if _describes(span, opt):
                    advertised.setdefault(opt, n)

    assign = re.compile(r"\b" + re.escape(field) + r"\s*=[^=]")
    hits = []
    seen = set()
    for n, line in enumerate(lines, 1):
        if not assign.search(line):
            continue
        for src in _arm_above(lines, n - 1, window):
            for opt in _OPTION.findall(src):
                if opt in advertised and opt not in seen:
                    seen.add(opt)
                    hits.append((opt, n, advertised[opt]))
    return sorted(hits)


def report_advertised(found, root):
    """Rank findings by whether the program's own `--help` promises them.

    A field that is parsed, stored, never read AND advertised is worse than one
    that is merely unread: the user has been told it works. `patch`'s
    `-l/--ignore-whitespace` was written down in four places -- three said
    inert and the only one a user reads said it worked.
    """
    promised, silent = [], []
    for name, (path, line, who) in sorted(found.items(), key=lambda kv: kv[1]):
        try:
            text = (root / path).read_text(encoding="utf-8", errors="replace")
        except OSError as exc:
            print(f"{path}: cannot read: {exc}", file=sys.stderr)
            silent.append((path, line, name, who, []))
            continue
        hits = advertised_for(text.splitlines(), name)
        (promised if hits else silent).append((path, line, name, who, hits))

    print(
        "-- `advertised` means the spelling also appears in a string literal "
        "or a `//!` usage line in the SAME file, AND inside the parser arm that "
        "assigns the field. Both halves are positional; the line numbers below "
        "are there so you can check rather than believe."
    )
    print()
    print(f"=== ADVERTISED, and unread ({len(promised)}) ===")
    for path, line, name, who, hits in sorted(
        promised, key=lambda r: (r[3] != "nothing", r[0])
    ):
        opts = ", ".join(o for o, _a, _h in hits)
        where = "; ".join(f"{o} assigned :{a}, advertised :{h}" for o, a, h in hits)
        read_by = "read only by tests" if who == "tests" else "read by NOTHING"
        print(f"{path}:{line}: `{name}` -> {opts} [{read_by}]")
        print(f"    {where}")
    print()
    print(f"=== not advertised ({len(silent)}) ===")
    for path, line, name, who, _hits in silent:
        read_by = "read only by tests" if who == "tests" else "read by NOTHING"
        print(f"{path}:{line}: `{name}` [{read_by}]")
    print()
    print(
        f"-- {len(promised) + len(silent)} field(s); {len(promised)} advertised "
        f"in the program's own help text."
    )


def detect(roots=lanec_scan.LANE_C_ROOTS, root=None):
    """`{field name: (relative path, line)}` for every asymmetric field."""
    decls = {}
    writes = collections.Counter()
    reads = collections.Counter()
    base = pathlib.Path(root) if root else lanec_scan.ROOT

    for path, lines, inside in lanec_scan.scanned(roots, root):
        in_struct = False
        in_destructure = False
        depth = 0
        for n, line in enumerate(lines):
            # A destructuring pattern reads every field it names.
            if in_destructure:
                m = BOUND_NAME.match(line)
                if m:
                    reads[(m.group(1), inside[n])] += 1
                if "}" in line:
                    in_destructure = False
                continue
            if DESTRUCTURE_OPEN.match(line) and "=" not in line:
                in_destructure = True
                continue
            if STRUCT.search(line):
                in_struct = "{" in line
                depth = line.count("{") - line.count("}")
                continue
            if in_struct:
                depth += line.count("{") - line.count("}")
                m = FIELD.match(line)
                if m and not inside[n]:
                    decls.setdefault(
                        m.group(1), (path.relative_to(base).as_posix(), n + 1)
                    )
                if depth <= 0:
                    in_struct = False
                continue
            for m in ACCESS.finditer(line):
                after = line[m.end() :].lstrip()
                if after.startswith("("):
                    continue  # a method call, not a field
                # `=>` is a match arm, not an assignment, and `==` is a
                # comparison. Both READ the field. Without this, a rule
                # expressed as a match guard -- `None if !self.hearts_broken
                # =>` in the Hearts game -- counted as a write, so the field
                # had "no production reads" and was reported as thrown-away
                # work when it was the opposite: load-bearing game logic.
                written = after.startswith("=") and not after.startswith(("==", "=>"))
                (writes if written else reads)[(m.group(1), inside[n])] += 1

    # Two categories, reported together because the action is the same for
    # both: wire the field to the behaviour it names, or delete it.
    #
    # `nobody` is the stricter finding and was invisible here until 2026-09-15.
    # The condition used to require `reads[(name, True)] >= 1` -- at least one
    # test read -- so a field read by NO ONE fell outside a detector named for
    # exactly that defect. It was found from the other direction: `auto_connect`
    # in `apps/ircclient`, set on a pre-shipped IRC network and read nowhere.
    #
    # Worth knowing why the compiler does not cover it either. `SavedNetwork`
    # derives `Debug`, and a derive READS every field, so `dead_code` cannot
    # see the case by construction -- the same shape as "a `#[cfg(test)]`
    # module is a use", one layer down. `cargo clippy --all-targets` on that
    # crate reports zero warnings.
    out = {}
    for name, where in decls.items():
        if writes[(name, False)] < 1 or reads[(name, False)] != 0:
            continue
        if reads[(name, True)] >= 1:
            out[name] = (*where, "tests")
        else:
            out[name] = (*where, "nobody")
    return out


def baseline():
    """`{"path:name"}` already known, one per line; `#` comments ignored."""
    if not BASELINE.is_file():
        return set()
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").split("\n"):
        line = line.split("#", 1)[0].strip()
        if line:
            out.add(line)
    return out


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    roots = None
    rest = []
    for arg in argv:
        if arg.startswith("--roots="):
            roots = [r for r in arg.split("=", 1)[1].split(",") if r]
        else:
            rest.append(arg)
    unknown = selftestflag.unknown_options(rest, known=("--list", "--advertised"))
    if unknown:
        print(f"unrecognised option(s): {' '.join(unknown)}", file=sys.stderr)
        return 2

    listing = "--list" in rest
    advertised = "--advertised" in rest

    # `--roots` is report-only, and the refusal is the point of the flag rather
    # than a limitation of it.
    #
    # The baseline is a list of `path:field`. Nothing in it records WHICH roots
    # produced it, so a pass against different roots would be a true sentence
    # about a population the run never looked at -- the failure this project
    # keeps finding in its own gates. A flag that lets another lane scan their
    # tree is useful; one that lets them collect a green tick for it is worse
    # than not having the flag.
    if roots is not None:
        missing = [r for r in roots if not (lanec_scan.ROOT / r).is_dir()]
        if missing:
            print(f"no such directory: {', '.join(missing)}", file=sys.stderr)
            return 2
        if listing is False:
            print(
                f"scanning {', '.join(sorted(roots))} (report only; the "
                f"baseline in {BASELINE.as_posix()} describes lane C and is "
                "not consulted)",
                file=sys.stderr,
            )
        found = detect(roots=tuple(roots))
        if advertised:
            report_advertised(found, lanec_scan.ROOT)
            return 0
        for name, (path, line, who) in sorted(found.items(), key=lambda kv: kv[1]):
            read_by = "read only by tests" if who == "tests" else "read by nothing at all"
            print(f"{path}:{line}: `{name}` is written in production and {read_by}")
        print(f"-- {len(found)} field(s) in {', '.join(sorted(roots))}.")
        return 0

    found = detect()

    # `--advertised` is a REPORT mode in both paths. Wiring it only into the
    # `--roots` branch would have left `--advertised` alone silently ignored --
    # a flag parsed, stored and read by nothing, which is the exact defect this
    # flag exists to rank.
    if advertised:
        report_advertised(found, lanec_scan.ROOT)
        return 0

    known = baseline()

    shown = []
    for name, (path, line, who) in sorted(found.items(), key=lambda kv: kv[1]):
        key = f"{path}:{name}"
        if key in known and not listing:
            continue
        shown.append((path, line, name, key in known, who))

    for path, line, name, was_known, who in shown:
        mark = "  (baseline)" if was_known else ""
        read_by = "read only by tests" if who == "tests" else "read by nothing at all"
        print(f"{path}:{line}: `{name}` is written in production and {read_by}{mark}")

    # A baseline line that no longer matches anything is not harmless. It
    # silently suppresses that exact field if it ever comes back, and it
    # describes a tree that no longer exists -- the same stale-blocker shape
    # that had three documents in this repo telling readers a decided question
    # was still open. Reported, and fatal, so the file cannot rot quietly.
    live = {f"{path}:{name}" for name, (path, _l, _w) in found.items()}
    stale = sorted(known - live)
    for key in stale:
        print(
            f"{key}: in the baseline but no longer found. Delete the line -- "
            f"while it is there, this field cannot be reported again."
        )

    unreported = [row for row in shown if not row[3]]
    if listing:
        print(f"-- {len(found)} field(s); {len(known)} in the baseline.")
        return 0
    if stale:
        return 1
    if not unreported:
        print(
            f"ok: no new write-only fields ({len(known)} in the baseline, "
            f"{len(found)} found)"
        )
        return 0
    by_tests = sum(1 for row in unreported if row[4] == "tests")
    by_nobody = len(unreported) - by_tests
    print(
        f"{len(unreported)} field(s) written in production and never read by "
        f"it: {by_tests} read only by tests, {by_nobody} read by nothing at all."
    )
    print(
        "  `dead_code` cannot see either. A test counts as a read -- and so "
        "does a derive, which is why a field on a struct deriving `Debug` is "
        "invisible to it however dead the field is."
    )
    return 1


def advertised_self_test():
    """The distinction lane B was asked for: advertised, versus merely mentioned.

    Both fixtures below name `--zeta` within the window. Only the first
    ADVERTISES it, and calling the second advertised would invert the finding:
    an option a comment records as inert is the honest case, not the bad one.
    """
    cases = [
        # (label, source, expect_advertised)
        ("help text in a string literal", [
            'fn help() {',
            '    text.push_str("  -z  --zeta   Do the zeta thing.");',
            '}',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], True),
        ("a `//!` usage block", [
            '//!   -z, --zeta    Do the zeta thing.',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], True),
        ("named ONLY in a comment", [
            '// --zeta is accepted and inert; see known-issues.',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], False),
        ("named only in a `///` doc comment", [
            '/// `--zeta` is the flag this field carries.',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], False),
        # The PARSER's own literal must not count as advertising. Without
        # this every parsed option is "advertised" and the flag says nothing.
        ("parsed but never described", [
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], False),
        # THE PREVIOUS ARM'S option must not leak in. `patch.rs` parses
        # `-Z` directly below `-f`, and a fixed four-line window reported
        # `set_utc` as advertised by `--force`. Here `zeta` is parsed by its
        # SHORT form only, so the sole long option in reach belongs to the arm
        # above and must not be credited to it.
        ("the arm above carries a different option", [
            'fn help() {',
            '    text.push_str("  -y  --yankee  Do the yankee thing.");',
            '}',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-y" || a == "--yankee" {',
            '        opts.yankee = true;',
            '    } else if a == "-z" {',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], False),
        # ...while a long arm still reaches its OWN option, which no single
        # line count manages together with the case above.
        ("several lines below the option, same arm", [
            'fn help() {',
            '    text.push_str("  -z  --zeta   Do the zeta thing.");',
            '}',
            'fn parse(a: &str, opts: &mut Opts) {',
            '    if a == "-z" || a == "--zeta" {',
            '        let _ = 1;',
            '        let _ = 2;',
            '        let _ = 3;',
            '        let _ = 4;',
            '        let _ = 5;',
            '        opts.zeta = true;',
            '    }',
            '}',
        ], True),
        # The backstop still has to be able to MISS, or it is not a bound: no
        # enclosing arm at all, and the help text further off than it reaches.
        ("no arm, help text out of range", [
            'fn help() {',
            '    text.push_str("  -z  --zeta   Do the zeta thing.");',
            '}',
        ] + ['    let _ = 0;'] * 14 + [
            'fn go(opts: &mut Opts) {',
            '    opts.zeta = true;',
            '}',
        ], False),
    ]

    problems = []
    for label, lines, want in cases:
        got = bool(advertised_for(lines, "zeta"))
        if got != want:
            problems.append(
                f"  {label}: expected advertised={want}, got {got}"
            )
    if problems:
        print("advertised self-test FAILED:")
        print("\n".join(problems))
        return 1
    print(f"ok: --advertised self-test passed ({len(cases)} case(s))")
    return 0


def self_test():
    """Three fixtures: the shape this looks for, and two that must not fire."""
    import tempfile

    files = {
        # 1. The real shape, and the one the first version could not see: the
        #    test reads through the variable it built, never through `self`.
        "apps/alpha/src/main.rs": """
            pub struct App {
                pub last_export: String,
                pub shown: String,
            }
            impl App {
                fn go(&mut self) {
                    self.last_export = "x".to_string();
                    self.shown = "y".to_string();
                }
                fn draw(&self) -> &str { &self.shown }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() {
                    let mut app = App::new();
                    app.go();
                    assert_eq!(app.last_export, "x");
                }
            }
            """,
        # 2. A field written AND read in production is not a finding, even
        #    though a test reads it too.
        "apps/beta/src/main.rs": """
            pub struct Beta {
                pub counted: u32,
            }
            impl Beta {
                fn bump(&mut self) { self.counted = 1; }
                fn report(&self) -> u32 { self.counted }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let b = Beta::new(); assert_eq!(b.counted, 0); }
            }
            """,
        # 3. A field only ever touched by tests is `dead_code`'s business, not
        #    this script's: no production work is being thrown away.
        "apps/gamma/src/main.rs": """
            pub struct Gamma {
                pub scratch: u32,
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() {
                    let mut g = Gamma::new();
                    g.scratch = 1;
                    assert_eq!(g.scratch, 1);
                }
            }
            """,
        # 4. A comment that names the field is not a read of it. The comments
        #    that name a field are overwhelmingly the ones explaining what it
        #    is for, so without stripping, documenting a field hides it.
        "apps/delta/src/main.rs": """
            pub struct Delta {
                pub caption: String,
            }
            impl Delta {
                // Fills in self.caption, which the panel shows.
                fn go(&mut self) {
                    self.caption = "x".to_string();
                }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let d = Delta::new(); assert_eq!(d.caption, "x"); }
            }
            """,
        # 5. A method call is not a field read, even when a method and a field
        #    share a name -- which is legal, and common for an accessor.
        "apps/epsilon/src/main.rs": """
            pub struct Eps {
                pub title: String,
            }
            impl Eps {
                fn go(&mut self) {
                    self.title = "x".to_string();
                }
                fn title(&self) -> &str { "constant" }
                fn draw(&self) -> &str { self.title() }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let e = Eps::new(); assert_eq!(e.title, "x"); }
            }
            """,
        # 7. A destructuring pattern reads every field it binds, and binds
        #    them by bare name -- no `.field` anywhere. `apps/reminders`
        #    serialises itself with `let Self { .., completed_at, .. } = self;`
        #    and was reported as throwing the value away.
        "apps/eta/src/main.rs": """
            pub struct Eta {
                pub kept: String,
            }
            impl Eta {
                fn go(&mut self) { self.kept = "x".to_string(); }
                fn to_json(&self) -> String {
                    let Self {
                        kept,
                    } = self;
                    format!("{kept}")
                }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let e = Eta::new(); assert_eq!(e.kept, "x"); }
            }
            """,
        # 6. A match guard reads the field. `=>` is not `=`, and reading it
        #    as one turns every rule expressed as a guard into a "write",
        #    which leaves the field with no reads and reports live logic as
        #    dead. Found in `apps/hearts`, where `hearts_broken` gates whether
        #    a heart may be led -- the rule the game is named after.
        "apps/zeta/src/main.rs": """
            pub struct Zeta {
                pub broken: bool,
            }
            impl Zeta {
                fn go(&mut self) { self.broken = true; }
                fn may_lead(&self) -> bool {
                    match self.pick() {
                        None if !self.broken => false,
                        _ => true,
                    }
                }
                fn pick(&self) -> Option<u8> { None }
            }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { let z = Zeta::new(); assert!(!z.broken); }
            }
            """,
    }

    # Every one of these was verified by reintroducing the bug it stands for
    # and watching the self-test go red. The first version had only case 1,
    # passed, and was blind to both 4 and 5.
    expected = {"last_export", "caption", "title"}
    forbidden = {"shown", "counted", "scratch", "broken", "kept"}

    with tempfile.TemporaryDirectory(prefix="fieldscan_selftest_") as tmp:
        base = pathlib.Path(tmp)
        for rel, body in files.items():
            f = base / rel
            f.parent.mkdir(parents=True, exist_ok=True)
            text = "\n".join(line[12:] for line in body.strip("\n").split("\n"))
            # `newline` is load-bearing here, not tidiness: without it Python
            # translates to CRLF on Windows, and these fixtures are parsed back
            # by column, so a stray carriage return rides along in every value
            # the self-test then compares. The gate that catches this reds the
            # boot in 15 seconds; lane A found both of mine.
            f.write_text(text + "\n", encoding="utf-8", newline="\n")
        found = set(detect(roots=("apps",), root=base))

    problems = []
    for want in sorted(expected):
        if want not in found:
            problems.append(
                f"  MISSED  {want}: written in production, read only by a test"
            )
    for never in sorted(forbidden):
        if never in found:
            problems.append(
                f"  FALSE   {never}: it is read in production, or never written there"
            )
    if problems:
        print("check-fields-written-never-read self-test FAILED:")
        print("\n".join(problems))
        print(f"  reported: {sorted(found)}")
        return 1
    print(
        f"ok: self-test passed ({len(expected)} write-only field found, "
        f"{len(forbidden)} sound fields rejected)"
    )
    return advertised_self_test()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
