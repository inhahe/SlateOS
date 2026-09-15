#!/usr/bin/env python3
"""Find functions that only their own tests ever call.

WHY THIS EXISTS, AND WHY THE COMPILER CANNOT DO IT.

`dead_code` is silenced by *any* use, and a `#[cfg(test)]` module is a use. So a
function with careful tests and no production caller is invisible: it compiles
clean, it is covered, the suite is green, and it does nothing. Three instances
in lane C on 2026-09-14 alone:

  * `DesktopShell::load_pinned` -- the taskbar wrote `taskbar.yaml` on every pin
    and nothing read it back. A pin reached the disk and never came back.
  * `apps/fileassoc`'s `import_config`, and `SyscallProvider::parse_u64` /
    `parse_u32` / `parse_f32` -- a complete, tested save format and three
    error-reporting parsers, called by nothing but their own tests, beside
    live code that swallowed the errors they were written to report.

Each was found by accident. The shape is always the same: the parts either side
of a door are tested and the door does not exist.

WHAT IT REPORTS. A function defined outside a test module, referenced at least
once from inside one, and referenced nowhere else in the tree. Referenced
*nowhere* is a different thing -- rustc's `dead_code` already says so -- and is
deliberately not reported here.

WHAT IT DOES NOT REPORT, and these are omissions rather than oversights:

  * trait methods and trait impls, which are called through the trait and so
    have no textual caller;
  * `main`, and anything under `#[cfg(test)]` itself;
  * names shared with another function anywhere in the tree, because one
    textual reference cannot be attributed between them. That is the same
    conservatism `scan-orphan-modules.py` learned the hard way -- see
    `TD-C-THE-ORPHAN-SCAN-CLEARS-A-MODULE-ON-A-NAME-AN-APP-HAPPENS-TO-SHARE`.
  * a counterpart in a *different crate*. A save/load pair is two halves of one
    program keeping one thing; halves in two programs are not a pair, they are
    two functions with a suffix in common. See `crate_of` below for what this
    cost before it was enforced.

A false positive here costs a reader a minute. A false negative is the defect
this is for, so the bias is deliberate.

Usage:
    python scripts/check-tested-but-uncalled.py            # report, exit 1 if any
    python scripts/check-tested-but-uncalled.py --list     # report, always exit 0
"""

import collections
import pathlib
import re
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import selftestflag  # noqa: E402  (needs the path above)

ROOT = pathlib.Path(__file__).resolve().parents[1]
# Lane C's ten directories, per `scripts/which-lane.py`. It listed six until
# 2026-09-14: `netipc`, `netring`, `aes` and `hmac` were simply missing, so the
# gate reported "no half of a save/load pair is missing its caller" while never
# having opened four of the trees that sentence covered.
#
# That is the same defect it exists to catch, in the tool itself: a clean
# verdict over a population the checker cannot enumerate. Nothing distinguishes
# "looked and found nothing" from "did not look" in the output, which is why
# the summary below now names the scope it examined.
LANE_C_ROOTS = (
    "apps",
    "gui",
    "net",
    "netipc",
    "netproto",
    "netring",
    "net80211",
    "aes",
    "hmac",
    "pkg",
)
ROOTS = LANE_C_ROOTS

# Directories that are build output or not Rust at all.
NOT_SOURCE = {"target", "build", "scripts", "requests", "toolchain", "limine"}

# A definition we can attribute. `pub` or not -- a private fn called only by its
# own tests is the same defect, and in a single-file crate it is the common one.
FN_DEF = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+"
    r"([a-z_][a-z0-9_]*)\s*[(<]"
)
# `impl Trait for Type` -- everything inside is reached through the trait.
TRAIT_IMPL = re.compile(r"^\s*impl(?:\s*<[^>]*>)?\s+[A-Za-z_][\w:<>, ]*\s+for\s+")
TEST_ATTR = re.compile(r"^\s*#\[(?:cfg\(test\)|test|tokio::test)\]")
IDENT = re.compile(r"\b([a-z_][a-z0-9_]*)\b")

SKIP = {"main", "new", "default", "fmt", "drop", "clone", "from", "eq", "cmp", "hash"}


def strip_comments(line, in_block):
    """The code on a line, with comments removed. Returns (code, still_in_block).

    WHY THIS IS NOT FUSSINESS. Without it this script counted *every mention*
    of a name as a call, including mentions in comments -- and the comments
    that name a function are overwhelmingly the ones explaining a bug it was
    involved in. `DesktopShell::load_pinned` is the case that proved it. On
    2026-09-14 it was found with no caller and wired up, and three sentences
    were written about the find: two in `session/tests.rs`, one in
    `session.rs`. Those three, plus its own definition, came to four
    "production references".

    So when the call was removed again as a positive control, this gate stayed
    silent: the record of the last time the door went missing was enough to
    hide the next. A check switched off by documenting its own successes is
    worse than no check -- it reads green for the exact reason it should read
    red, and it gets quieter the more diligent its owner is.

    String literals are deliberately *not* stripped. Comments were observed
    masking a real defect three times over; strings have not been observed
    doing it once, and changing two things at a time would leave neither
    proven.
    """
    out = []
    i = 0
    n = len(line)
    in_str = False
    while i < n:
        ch = line[i]
        nxt = line[i + 1] if i + 1 < n else ""
        if in_block:
            if ch == "*" and nxt == "/":
                in_block = False
                i += 2
                continue
            i += 1
        elif in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            out.append(ch)
            i += 1
        elif ch == '"':
            in_str = True
            out.append(ch)
            i += 1
        elif ch == "/" and nxt == "/":
            break
        elif ch == "/" and nxt == "*":
            in_block = True
            i += 2
        else:
            out.append(ch)
            i += 1
    return "".join(out), in_block


EXT_TEST_MOD = re.compile(r"^\s*mod\s+([a-z_][a-z0-9_]*)\s*;")


def external_test_files():
    """Files that are wholly test code because their *parent* declared them so.

    `#[cfg(test)] mod tests;` puts the whole of `tests.rs` under that
    attribute, but the attribute is in the parent file. `test_spans` looks for
    it inside the file it is reading, finds nothing, and concludes the file is
    production code -- so every test in `gui/desktop/src/session/tests.rs`
    counted as a production caller. That is exactly backwards: those are the
    callers this script exists to see past.
    """
    found = set()
    for path in rust_files():
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
        block = False
        for n, raw in enumerate(lines):
            code, block = strip_comments(raw, block)
            if not TEST_ATTR.match(code):
                continue
            nxt = lines[n + 1] if n + 1 < len(lines) else ""
            m = EXT_TEST_MOD.match(nxt)
            if not m:
                continue
            stem = path.parent / m.group(1)
            found.add(stem.with_suffix(".rs"))
            found.add(stem / "mod.rs")
    return found


def every_root():
    """Every top-level directory with Rust in it, for `--all-lanes`.

    The default scope is lane C's own trees because that is the lane that owns
    this script and a gate should refuse the push of whoever can fix the
    finding. A missing door in `services/` is just as real, though, and lane B
    cannot see it from here -- so the scope is a flag rather than a constant.
    """
    roots = []
    for child in sorted(ROOT.iterdir()):
        if not child.is_dir() or child.name.startswith("."):
            continue
        if child.name in NOT_SOURCE:
            continue
        if next(child.rglob("*.rs"), None) is not None:
            roots.append(child.name)
    return tuple(roots)


def rust_files():
    for root in ROOTS:
        base = ROOT / root
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            if "target" in path.parts or "build" in path.parts:
                continue
            yield path


def test_spans(lines):
    """Line indices that sit inside a `#[cfg(test)]` region, by brace depth.

    Depth counting rather than a marker scan, because a test module is where a
    fixture's own helpers live and those are as much "test code" as the
    `#[test]` functions beside them.
    """
    inside = [False] * len(lines)
    i = 0
    while i < len(lines):
        if not TEST_ATTR.match(lines[i]):
            i += 1
            continue
        # Walk to the opening brace of the item the attribute is on.
        j = i
        while j < len(lines) and "{" not in lines[j]:
            j += 1
        if j >= len(lines):
            break
        depth = 0
        for k in range(j, len(lines)):
            depth += lines[k].count("{") - lines[k].count("}")
            inside[k] = True
            if depth <= 0 and k >= j:
                i = k + 1
                break
        else:
            break
    return inside


def scan():
    defs = collections.defaultdict(list)      # name -> [(path, line)]
    prod_refs = collections.Counter()
    test_refs = collections.Counter()
    # (crate, name) -> count. The global counters answer "does anything call
    # this?", which is a whole-tree question and must stay whole-tree: a
    # toolkit function called only from `apps/` is wired, not dead. This one
    # answers "does *this program* call it?", which is the only question a
    # pair-of-halves claim can be built on.
    crate_prod_refs = collections.Counter()
    whole_file_tests = external_test_files()

    for path in rust_files():
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.split("\n")
        # Comments first: `test_spans` should not be fooled by a doc comment
        # that quotes `#[cfg(test)]` either.
        stripped = []
        block = False
        for raw in lines:
            code, block = strip_comments(raw, block)
            stripped.append(code)
        lines = stripped
        inside = test_spans(lines)
        if path in whole_file_tests:
            inside = [True] * len(lines)
        in_trait_impl_until = -1
        depth = 0

        for n, line in enumerate(lines):
            if TRAIT_IMPL.match(line):
                in_trait_impl_until = depth + line.count("{") - line.count("}")
            depth += line.count("{") - line.count("}")
            if in_trait_impl_until >= 0 and depth <= in_trait_impl_until:
                in_trait_impl_until = -1

            m = FN_DEF.match(line)
            if m and not inside[n] and in_trait_impl_until < 0:
                name = m.group(1)
                if name not in SKIP:
                    defs[name].append((path.relative_to(ROOT).as_posix(), n + 1))

            bucket = test_refs if inside[n] else prod_refs
            crate = crate_of(path.relative_to(ROOT).as_posix())
            for ident in IDENT.findall(line):
                bucket[ident] += 1
                if not inside[n]:
                    crate_prod_refs[(crate, ident)] += 1

    return defs, prod_refs, test_refs, crate_prod_refs


# The two halves of keeping something. A file written and never read back is a
# different and worse thing than an unused builder method, and this is what
# separates them.
READERS = ("load", "read", "import", "restore", "reload")
WRITERS = ("save", "write", "export", "store", "persist")


def crate_of(rel_path):
    """Which program a file belongs to: `apps/passwordgen/src/main.rs` -> `apps/passwordgen`.

    WHY THE PAIRING NEEDS THIS, in one case that this script got wrong for a
    day. `apps/passwordgen` has `export_history` (it returns the password list
    as a String). `gui/desktop/src/run_dialog.rs` has `load_history` (it takes
    the Run box's previous commands). Different programs, different data, no
    relationship whatever -- but `defs` was keyed by bare function name across
    the whole tree, so the two matched and the gate reported a missing door
    between two buildings.

    It was the only thing the gate reported, so its false-positive rate was
    100% while reading as a clean, specific finding. Worse, the "fix" for it
    was obvious and wrong: wire passwordgen's export to a file dialog, watch
    the gate go green, and conclude the instrument worked.
    """
    parts = rel_path.split("/src/")
    if len(parts) > 1:
        return parts[0]
    return rel_path.rsplit("/", 1)[0]


def counterpart(name):
    """The other half of `name`, if it looks like half of a pair.

    `load_pinned` <-> `save_pinned`. Returns every spelling worth trying, since
    a tree is free to pair `import_config` with `export_config` or with
    `write_config`.
    """
    for group, others in ((READERS, WRITERS), (WRITERS, READERS)):
        for verb in group:
            if name == verb or name.startswith(verb + "_"):
                tail = name[len(verb):]
                return [other + tail for other in others]
    return []


def analyse():
    """Everything the report is computed from.

    Split out of `main` so the self-test can assert on verdicts rather than on
    printed text. A gate whose self-test greps its own stdout passes when the
    wording changes and fails when it does not, which is backwards.
    """
    defs, prod_refs, test_refs, crate_prod_refs = scan()

    uncalled = []
    for name, sites in sorted(defs.items()):
        if len(sites) != 1:
            continue
        path, line = sites[0]
        if prod_refs[name] > 1 or test_refs[name] == 0:
            continue
        uncalled.append((path, line, name, test_refs[name]))

    # THE NARROW QUESTION, and the reason this script is worth running.
    #
    # The broad one -- "what do only tests call?" -- answers 915 times on this
    # tree, and a gate that reports 915 things is a gate nobody reads. Most are
    # library API with no consumer yet, which is the island problem and is
    # already tracked module-by-module in orphan-modules-baseline.txt.
    #
    # An asymmetric *pair* is a different animal. If `save_x` is called in
    # production and `load_x` only by tests, the program writes a file it never
    # reads: every part works, the suite is green, and the user's setting
    # vanishes. That is not unused API, it is a missing door, and it happened
    # three times in lane C on 2026-09-14.
    pairs = []
    for path, line, name, uses in uncalled:
        crate = crate_of(path)
        for other in counterpart(name):
            # Both tests are needed and neither implies the other.  # noqa: E501 The first
            # says this program *has* the other half; the second says this
            # program *uses* it outside its tests. A half defined here and
            # called only from another crate's production code would pass the
            # second on the global counter and mean nothing.
            here = [d for d in defs.get(other, []) if crate_of(d[0]) == crate]
            if here and crate_prod_refs[(crate, other)] > 1:
                pairs.append((path, line, name, other, uses))
                break

    return uncalled, pairs


def main(argv):
    if selftestflag.wants_selftest(argv):
        return self_test()
    unknown = selftestflag.unknown_options(
        argv, known=("--list", "--all", "--all-lanes")
    )
    if unknown:
        # Lane A lost minutes to this exact silence: `--self-test` was accepted
        # by an `in argv` test that nothing else checked, so the flag ran a
        # live scan and exited 1 on a real finding, which reads precisely like
        # a failing self-test. An option this script does not know is now an
        # error rather than a scan.
        print(f"unrecognised option(s): {' '.join(unknown)}", file=sys.stderr)
        return 2

    listing = "--list" in argv
    every = "--all" in argv

    global ROOTS
    if "--all-lanes" in argv:
        ROOTS = every_root()
    scope = "the whole tree" if ROOTS is not LANE_C_ROOTS else "lane C's trees"

    uncalled, pairs = analyse()

    for path, line, name, other, uses in pairs:
        # States what was measured and stops. A gate that diagnoses past its
        # evidence teaches its readers to discount it.
        #
        # This line has now been wrong twice about the same pair, and the
        # second time is the instructive one. The first version asserted
        # "Something is written and never read back", which was true of
        # `load_pinned`/`save_pinned` and false of
        # `export_history`/`load_history`; the repair was to weaken the
        # sentence until it was true of both.
        #
        # That repair treated a false *finding* as a wording problem. The
        # evidence was right there -- the two functions were in different
        # programs -- and softening the claim made the output survive the
        # contradiction instead of the contradiction killing the claim. The
        # question that would have caught it is not "is this sentence true?"
        # but "why did these two get paired at all?", and it went unasked for
        # a day because the sentence, once softened, read fine.
        print(
            f"{path}:{line}: `{name}` is called {uses}x, all from tests, "
            f"while its counterpart `{other}` is called in production."
        )

    if every:
        print()
        for path, line, name, uses in uncalled:
            print(f"{path}:{line}: `{name}` is called {uses}x, all from tests")
        print(f"-- {len(uncalled)} called only by their own tests (--all).")

    if not pairs:
        print(
            f"ok: no half of a save/load pair is missing its caller, "
            f"in {len(ROOTS)} director(ies) of {scope}"
        )
        if not every:
            print(
                f"   ({len(uncalled)} functions are called only by their own "
                "tests; --all lists them. Most are API with no consumer yet, "
                "which the orphan-module ledger tracks.)"
            )
        return 0
    print(
        f"{len(pairs)} asymmetric pair(s). `dead_code` cannot see these: a test "
        "counts as a use, so the half nobody calls compiles clean and passes."
    )
    return 0 if listing else 1


def self_test():
    """Check the scanner against four crates written for the purpose.

    Every case here is a bug this script actually had on 2026-09-14, the day
    after it was written, when its only finding on a clean tree was false and
    it could not see the defect it was written from. Fixtures rather than the
    live tree, because the live tree is expected to be clean: a check whose
    only evidence is "it found nothing" cannot tell working from broken.
    """
    import tempfile

    global ROOT, ROOTS

    files = {
        # 1. The true positive. `load_alpha` is tested and never called.
        "apps/alpha/src/main.rs": """
            pub fn save_alpha() {}
            pub fn load_alpha() {}
            fn wired() { save_alpha(); }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { load_alpha(); }
            }
            """,
        # 2. Two programs sharing a suffix are not a pair. This was the only
        #    thing the gate reported on a clean tree, and it was this.
        "apps/beta/src/main.rs": """
            pub fn export_beta() {}
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { export_beta(); }
            }
            """,
        "gui/gamma/src/lib.rs": """
            pub fn load_beta() {}
            fn wired() { load_beta(); }
            """,
        # 3. A comment naming the function is not a caller. Three sentences
        #    about the last time `load_pinned` went missing were enough to
        #    hide the next time.
        "apps/delta/src/main.rs": """
            // load_delta had no caller at all until today, see the notes.
            pub fn load_delta() {}
            /// Saves. See `load_delta` for the other half.
            pub fn save_delta() {}
            fn wired() { save_delta(); }
            #[cfg(test)]
            mod tests {
                #[test]
                fn t() { load_delta(); }
            }
            """,
        # 4. `#[cfg(test)] mod tests;` puts the attribute in the parent, so the
        #    child file is wholly test code even though it says so nowhere.
        "apps/epsilon/src/main.rs": """
            pub fn save_eps() {}
            pub fn load_eps() {}
            fn wired() { save_eps(); }
            #[cfg(test)]
            mod tests;
            """,
        #    The call must sit in a plain helper, NOT under `#[test]`: an
        #    attribute makes a span all by itself, so a `#[test]` here would
        #    count as test code with or without the fix and prove nothing.
        #    The first version of this fixture did exactly that and passed
        #    against the bug.
        "apps/epsilon/src/tests.rs": """
            fn helper() { load_eps(); }
            #[test]
            fn t() { helper(); }
            """,
    }

    expected = {"load_alpha", "load_delta", "load_eps"}
    forbidden = {"export_beta"}

    with tempfile.TemporaryDirectory(prefix="uncalled_selftest_") as tmp:
        base = pathlib.Path(tmp)
        for rel, body in files.items():
            f = base / rel
            f.parent.mkdir(parents=True, exist_ok=True)
            text = "\n".join(line[12:] for line in body.strip("\n").split("\n"))
            f.write_text(text + "\n", encoding="utf-8")

        saved = (ROOT, ROOTS)
        ROOT, ROOTS = base, ("apps", "gui")
        try:
            _uncalled, pairs = analyse()
        finally:
            ROOT, ROOTS = saved

    reported = {name for _p, _l, name, _o, _u in pairs}
    problems = []
    for want in sorted(expected):
        if want not in reported:
            problems.append(f"  MISSED  {want}: a real asymmetric pair went unreported")
    for never in sorted(forbidden):
        if never in reported:
            problems.append(
                f"  FALSE   {never}: paired with a function in another crate"
            )

    if problems:
        print("check-tested-but-uncalled self-test FAILED:")
        print("\n".join(problems))
        print(f"  reported: {sorted(reported)}")
        return 1
    print(
        f"ok: self-test passed ({len(expected)} asymmetric pairs found, "
        f"{len(forbidden)} cross-crate false pair rejected)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))