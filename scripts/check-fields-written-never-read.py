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
import rustscan  # noqa: E402  (needs the path above)
import selftestflag  # noqa: E402

BASELINE = (
    pathlib.Path(__file__).resolve().parent / "fields-written-never-read-baseline.txt"
)

FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*[A-Za-z_&<(\[]")
STRUCT = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+([A-Z]\w*)")
ACCESS = re.compile(r"\.([a-z_][a-z0-9_]*)")


def detect(roots=rustscan.LANE_C_ROOTS, root=None):
    """`{field name: (relative path, line)}` for every asymmetric field."""
    decls = {}
    writes = collections.Counter()
    reads = collections.Counter()
    base = pathlib.Path(root) if root else rustscan.ROOT

    for path, lines, inside in rustscan.scanned(roots, root):
        in_struct = False
        depth = 0
        for n, line in enumerate(lines):
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

    return {
        name: where
        for name, where in decls.items()
        if writes[(name, False)] >= 1
        and reads[(name, False)] == 0
        and reads[(name, True)] >= 1
    }


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
    unknown = selftestflag.unknown_options(argv, known=("--list",))
    if unknown:
        print(f"unrecognised option(s): {' '.join(unknown)}", file=sys.stderr)
        return 2

    listing = "--list" in argv
    found = detect()
    known = baseline()

    shown = []
    for name, (path, line) in sorted(found.items(), key=lambda kv: kv[1]):
        key = f"{path}:{name}"
        if key in known and not listing:
            continue
        shown.append((path, line, name, key in known))

    for path, line, name, was_known in shown:
        mark = "  (baseline)" if was_known else ""
        print(
            f"{path}:{line}: `{name}` is written in production and read only "
            f"by tests{mark}"
        )

    # A baseline line that no longer matches anything is not harmless. It
    # silently suppresses that exact field if it ever comes back, and it
    # describes a tree that no longer exists -- the same stale-blocker shape
    # that had three documents in this repo telling readers a decided question
    # was still open. Reported, and fatal, so the file cannot rot quietly.
    live = {f"{path}:{name}" for name, (path, _l) in found.items()}
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
    print(
        f"{len(unreported)} field(s) written in production and read only by "
        "tests. `dead_code` cannot see these: a test counts as a read."
    )
    return 1


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
    forbidden = {"shown", "counted", "scratch", "broken"}

    with tempfile.TemporaryDirectory(prefix="fieldscan_selftest_") as tmp:
        base = pathlib.Path(tmp)
        for rel, body in files.items():
            f = base / rel
            f.parent.mkdir(parents=True, exist_ok=True)
            text = "\n".join(line[12:] for line in body.strip("\n").split("\n"))
            f.write_text(text + "\n", encoding="utf-8")
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
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
