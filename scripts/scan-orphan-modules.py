#!/usr/bin/env python3
"""Find library modules whose entire public surface is named by no other file.

`scan-unwired.py` asks the *function*-level question and asks it only of
`main.rs`, for a stated reason: a private fn in a library module can be called
by a sibling module, which a file-local scan cannot see, so reporting it would
be a false positive.  That reason does not apply one level up.  A `pub struct`
or a top-level `pub fn` in a library module **must be named** by any file that
uses it -- `use`d, or written out as a path.  So the question "does any other
file in the tree mention *any* of this module's public items?" is answerable
from source text alone, and a `no` is sound.

The shape it finds is lesson 45 at module scale.  `gui/desktop/src/a11y.rs` is
2292 lines -- a screen magnifier, four high-contrast schemes, sticky keys,
filter keys, mouse keys, a colourblind filter -- and no file outside it names
any of its thirteen public items or its module path.  The tests make it look
covered, which is what makes an island worse than plain dead code: `cargo
build` cannot warn about a `pub` item, and the test suite reports it green.

Measured 2026-08-25: **57 of the 200 library modules under lane C's roots are
islands, 113k lines**, and 39 of those 57 are in `gui/desktop` alone -- a
crate whose `lib.rs` declares 59 modules.  Read that against the hand-count in
`known-issues.md`'s
`TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES`, arrived at by a
different method: four modules drawn, thirty-nine with no caller at all.  The
two agree, and that agreement is the best evidence this scan is calibrated.
Note that the *first* run of it said 21, not 57 -- three separate alibi
classes (`member_names`, `plausible`, `code_only`) each hid a slice, and each
was found only by hand-checking a module the scan had cleared.  A scan of this
kind is not finished when it runs; it is finished when its clearances survive
being disbelieved.

**A hit is not automatically a bug.**  Four benign explanations:

  * the crate is a library whose consumer is outside this tree entirely;
  * the module is behind a `cfg` this scan does not evaluate;
  * it is a newly-written module whose caller is the next commit;
  * it is genuinely dead and should be deleted.

The fifth kind is the one worth a person's time: a module that duplicates
subject matter some *other* module owns and is live for, so the tree carries
two models of one setting and the user's edits go to whichever one is wired up.
Triage by asking who else in the tree covers the same nouns -- and the
`shares N name(s)` line does most of that asking for you.

**The question is asked of the module, not of its items.**  A module counts as
reached the moment *one* of its public items is used, so this says nothing
about the rest.  `gui/desktop/src/power.rs` is 2859 lines and is *not* an
island, because `lib.rs` draws its power menu -- while `PowerManager`,
`PowerConfig`, `ScreenSaver` and a `to_config_string`/`from_config_string`
pair inside it have no caller at all.  That is the function-level question and
`scan-unwired.py`'s, not this one's; an absent module here is not a clean bill
of health for the module.

**Usage.**

    python scripts/scan-orphan-modules.py            # the full report
    python scripts/scan-orphan-modules.py --check     # ratchet: fail on a NEW island
    python scripts/scan-orphan-modules.py --pin       # rewrite the baseline

Exit codes: `0` nothing new, `1` at least one unpinned island (listed), `2` the
check could not run -- a bad argument, a missing baseline, or a working
directory with no modules in it.  Two is never confused with zero, because a
gate that cannot fire must not look like a gate that passed.

`--check` is a **ratchet, not a clean-tree test**: the 57 already found are
pinned in `scripts/orphan-modules-baseline.txt` and the gate is silent about
them.  What it refuses is a *new* one.  That is the useful shape while the
existing debt is blocked on an operator decision (`open-questions.md` ->
C-Q6): the pile cannot be paid down today, and it also cannot grow.  Removing
a line is always welcome and the run says so; adding one is a decision that
has to be made in a commit message where somebody can see it.

**What it deliberately does not count as a mention.**  A bare re-export
(`pub use power::PowerManager;`) is plumbing, not a caller: it widens the
item's visibility without anyone having used it, and counting it would make
every re-exported island look reached.  Mentions inside `#[cfg(test)]` do not
count -- a test is not a user -- though a module reached *only* from other
files' tests is reported separately as a test helper, which is a complete and
benign explanation.  Comments and string literals do not count either; see
`code_only`.

**What it cannot see.**  A glob re-export (`pub use power::*;`) followed by an
unqualified use in a third file is invisible as an edge to the module, though
the *item* mention in that third file is still counted, so the module is
correctly reported as reached.  Macro-generated paths are invisible.  Names
shared with another module, with any enum variant, or with any associated
`fn`/`const`/`type` are dropped from the evidence entirely (see
`variant_names` and `member_names`), so a module all of whose items have
common names rests on its module path alone.  Struct field names are the one
spelling hazard still not folded in; `member_names` says why.  It never proves
a module is dead; it produces a short list worth reading.
"""

import os
import pathlib
import re
import sys

# Lane C's tree, plus every `net*` crate.  Mentions are searched for across the
# *whole* repository, not just these roots: a lane-C type used by lane B's
# userspace is used, and reporting it as an island would be wrong.
ROOTS = ["gui", "apps", "pkg"]

# Top-level public items only -- column zero, no leading whitespace.
#
# Methods (`    pub fn ...` inside an `impl`) are excluded on purpose.  Their
# names are the common ones -- `new`, `render`, `label`, `apply` -- and one
# unrelated `.render(` anywhere in the tree would mark a genuine island as
# reached.  Excluding them can only make the scan report *more* islands, never
# fewer, and every extra one is checked by hand.
PUB_ITEM = re.compile(
    r"^pub(?:\([^)]*\))?\s+(?:unsafe\s+)?(?:async\s+)?"
    r"(?:struct|enum|trait|union|type|fn|const|static)\s+"
    r"([A-Za-z_][A-Za-z0-9_]*)"
)

# A line that only widens visibility.  `pub use x::Y;` and `pub(crate) use`.
REEXPORT = re.compile(r"^\s*pub(?:\([^)]*\))?\s+use\b")

# Every identifier on a line, in source order.  Deliberately not anchored to
# `::` or `<`: a type is named plenty of ways -- `use m::T;`, `T::new()`,
# `Vec<T>`, `-> T`, `let x: T` -- and the point is only to know whether the
# name appears at all, not how it was used.
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")


def code_only(line):
    """`line` with any trailing `//` comment and every string literal removed.

    Prose is not a caller, and in a tree that documents itself as heavily as
    this one that distinction decides findings.  `user_accounts.rs` defines
    `Avatar` and nothing outside it uses the type -- but `login_screen.rs`
    says "Avatar icon character (placeholder for real avatar images)" in a doc
    comment, and `apps/contacts` says "// Avatar circle".  Counting those, the
    module reported as reached.

    A string literal is prose too, and dropping only comments was not enough.
    Every mention of `desktop` in lane A's `kernel/src/fs/contextmenu.rs` is a
    comment save one -- `serial_println!("[contextmenu] test 3 passed:
    desktop menu build")` -- and that one word inside that one string was the
    whole basis on which the file counted as a plausible user of the desktop
    crate, which in turn was the whole basis on which
    `gui/desktop/src/context_ext.rs` reported as reached.  Rust has no way to
    name an item from inside a string, so nothing real is lost.

    The `//` scan is string-aware, so a `"https://..."` does not truncate the
    line.  Char literals and raw or multi-line strings can still fool the
    state machine; the failure is then to keep text that is not code, which
    can only *hide* an island, and every island printed is checked by hand.
    """
    # The overwhelming majority of lines contain neither, and the character
    # loop below is the single hottest thing this script does -- it is the
    # difference between a 78-second gate and a 20-second one, which is the
    # difference between a gate that runs before every build and one that
    # gets commented out.
    if '"' not in line and "//" not in line:
        return line

    out = []
    in_str = False
    escaped = False
    for i, ch in enumerate(line):
        if escaped:
            escaped = False
        elif ch == "\\":
            escaped = True
        elif ch == '"':
            in_str = not in_str
        elif ch == "/" and not in_str and line[i + 1 : i + 2] == "/":
            break
        if not in_str and ch != '"':
            out.append(ch)
    return "".join(out)

# Files that aggregate rather than define.  A crate root naming its own modules
# says nothing about whether anyone uses them.
AGGREGATORS = {"lib.rs", "mod.rs"}


def test_spans(lines):
    """Line ranges (0-based, inclusive) covered by a `#[cfg(test)]` item."""
    spans = []
    for i, line in enumerate(lines):
        if "cfg(test)" not in line:
            continue
        depth = 0
        started = False
        j = i
        while j < len(lines):
            depth += lines[j].count("{") - lines[j].count("}")
            if "{" in lines[j]:
                started = True
            if started and depth <= 0:
                break
            j += 1
        spans.append((i, j))
    return spans


def in_spans(idx, spans):
    return any(a <= idx <= b for a, b in spans)


def block_end(lines, idx):
    """Last line (0-based, inclusive) of the block opened at or after `idx`."""
    depth = 0
    started = False
    j = idx
    while j < len(lines):
        depth += lines[j].count("{") - lines[j].count("}")
        if "{" in lines[j]:
            started = True
        if started and depth <= 0:
            return j
        j += 1
    return len(lines) - 1


#  Directories never worth descending into.  `target` is the one that matters:
#  a single Rust build tree holds tens of thousands of files, and there is one
#  per crate and one per worktree.
PRUNE = {"target", ".git", "node_modules", "__pycache__", ".venv"}


def rust_files(base):
    """Every `.rs` file under `base`, build output excluded.

    Walks and prunes rather than `rglob("*.rs")`-then-filter.  The filtering
    version was correct and was also, by a wide margin, the slowest thing in
    this script: `rglob` descends into every `target/` in the tree before the
    `"target" in f.parts` test gets a chance to reject anything, so the run
    paid a full recursive `scandir` of the build output -- 23 of 48 seconds,
    to enumerate files it then discarded.  Pruning the directory is the same
    predicate applied one level earlier, where it costs nothing.
    """
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames[:] = sorted(d for d in dirnames if d not in PRUNE)
        d = pathlib.Path(dirpath)
        for name in sorted(filenames):
            if name.endswith(".rs"):
                yield d / name


ENUM_HEAD = re.compile(r"\benum\s+([A-Za-z_][A-Za-z0-9_]*)")
VARIANT = re.compile(r"^\s+([A-Z][A-Za-z0-9_]*)\s*(?:[,({=]|$)")


def variant_names(lines):
    """Every enum variant name declared in `lines`.

    Variants share a namespace with nothing, but they share *spelling* with
    plenty, and a spelling is all this scan has.  `a11y.rs` defines
    `pub struct StickyKeys`, `FilterKeys`, `MouseKeys` and `Magnifier`; the
    module that duplicates it, `accessibility_settings.rs`, has an
    `enum A11yFeature` with variants of all four names.  Every `StickyKeys`
    outside `a11y.rs` is `A11yFeature::StickyKeys` -- and on the strength of
    those, a 2291-line island reported as reached, alibi'd by its own
    duplicate.  Folding variants into the ambiguity pool costs a handful of
    real edges and buys back the finding the scan exists for.
    """
    names = set()
    for i, line in enumerate(lines):
        if not ENUM_HEAD.search(line) or "{" not in "".join(lines[i : i + 2]):
            continue
        for j in range(i + 1, block_end(lines, i) + 1):
            m = VARIANT.match(lines[j])
            if m:
                names.add(m.group(1))
    return names


MEMBER = re.compile(
    r"^\s+(?:pub(?:\([^)]*\))?\s+)?(?:default\s+)?(?:const\s+)?(?:async\s+)?"
    r'(?:unsafe\s+)?(?:extern\s+"[^"]*"\s+)?(?:fn|const|type)\s+'
    r"([A-Za-z_][A-Za-z0-9_]*)"
)


def member_names(lines):
    """Every associated `fn`/`const`/`type` name declared in `lines`.

    An associated item lives in its own per-type namespace, so `Taskbar` and
    `ProcExplorer` may each have a `render_context_menu` without either being
    the other -- but a *call* to one, `self.render_context_menu(..)`, is
    spelled exactly like a call to a free `pub fn render_context_menu`, and a
    spelling is all this scan has.  That is not hypothetical: it is the third
    module this scan reported as reached and was not.
    `gui/desktop/src/context_ext.rs` has 2042 lines and a single mentioned
    name -- its free `pub fn render_context_menu` -- and the three mentions
    are inherent methods on `taskbar::Taskbar`, `procexplorer` and
    `sysmonitor`, which do not know the module exists.  Nothing else in the
    file, `ContextMenuExtensionManager` and `ExtensionSettingsUI` included, is
    named anywhere outside it.

    Note the shape shared with `variant_names`: both fold a *non-owner* that
    nonetheless spoils a spelling into the ambiguity pool.  Struct **fields**
    are the same hazard one step further (`cfg.timeout_policy` reads like a
    call to a free `timeout_policy`) and are deliberately not folded in yet --
    fields are far more numerous than methods, and each name dropped costs a
    real edge.  If a hand-check of a future run turns up a field alibi, this
    is where it goes.
    """
    names = set()
    for line in lines:
        m = MEMBER.match(line)
        if m:
            names.add(m.group(1))
    return names


def public_items(lines):
    """`{name: line_no}` for every top-level public item defined outside tests."""
    spans = test_spans(lines)
    items = {}
    for i, line in enumerate(lines):
        m = PUB_ITEM.match(line)
        if m and not in_spans(i, spans):
            items.setdefault(m.group(1), i + 1)
    return items


BASELINE = pathlib.Path("scripts/orphan-modules-baseline.txt")

BASELINE_HEADER = """\
# Islands pinned by scripts/scan-orphan-modules.py --check.
#
# Each line is a library module under lane C's roots that defines top-level
# public items which no other file in the repository names.  57 of them, 113k
# lines, were found the day this file was created; the list is a debt ledger,
# not an allow-list, and the only edit it should ever receive is a deletion.
#
# `--check` fails on a module that is an island and is NOT listed here.  That
# is the whole point: the count may fall, never rise.  A new module lands
# wired up or it does not land.  When you connect one, delete its line
# (`--pin` rewrites the file, but read the diff -- a --pin that ADDS a line is
# the failure this gate exists to prevent, committed by hand).
#
# Being on this list is not absolution.  See known-issues.md ->
# TD-C-THE-SHELL-DRAWS-FOUR-OF-ITS-FIFTY-SEVEN-MODULES, and note that the
# largest entries are blocked on open-questions.md -> C-Q6, which decides
# whether the shell's settings pages survive at all.
"""


def read_baseline():
    """The pinned island set, or None if the file is absent."""
    if not BASELINE.is_file():
        return None
    out = set()
    for line in BASELINE.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def main():
    argv = sys.argv[1:]
    mode = "report"
    for a in argv:
        if a in ("--check", "--pin"):
            mode = a[2:]
        else:
            print(f"unknown argument: {a}", file=sys.stderr)
            print(__doc__, file=sys.stderr)
            return 2

    base = pathlib.Path(".")
    roots = list(ROOTS)
    roots += [p.name for p in base.iterdir() if p.is_dir() and p.name.startswith("net")]

    # Candidate modules: library modules under lane C's roots that define at
    # least one top-level public item.
    candidates = {}
    for root in sorted(set(roots)):
        rp = base / root
        if not rp.is_dir():
            continue
        for f in rust_files(rp):
            if f.name == "main.rs" or f.name in AGGREGATORS:
                continue
            try:
                lines = f.read_text(encoding="utf-8", errors="replace").split("\n")
            except OSError:
                continue
            items = public_items(lines)
            if items:
                candidates[f] = (items, len(lines))

    if not candidates:
        # Exit 2, not 1: a check that cannot run must never be
        # indistinguishable from a check that passed.
        print("no candidate modules found -- wrong working directory?", file=sys.stderr)
        return 2

    # One pass over the whole repository, counting which candidate item names
    # are mentioned by some file other than the one that defines them.
    #
    # Tokenise each line into identifiers and intersect with the name set,
    # rather than matching one alternation of every name against every line.
    # The alternation was tried first and is unusably slow for the reason
    # Python's `re` is a backtracking engine: it tries the branches in order at
    # every position, so the cost is (names x line length) per line, and at
    # ~4000 names over ~4500 files it had not finished in four minutes.
    # Tokenising is (line length) per line, and the whole tree takes seconds.
    every_name = set()
    for items, _ in candidates.values():
        every_name.update(items)

    # A name defined by two candidate modules cannot be attributed to either
    # from a mention alone, and this is not a corner case -- it is precisely
    # the duplication the scan exists to find.  `gui/desktop` defines
    # `ColorFilter` twice, in `a11y.rs` and in `accessibility_settings.rs`;
    # `MagnifierConfig` likewise.  Counting an ambiguous mention for both
    # owners let each module vouch for the other, and the first run of this
    # script reported neither -- two 2000-line models of one setting, mutually
    # alibi'd.  An ambiguous name therefore proves nothing about *either*
    # owner and is dropped; a module all of whose names are ambiguous falls
    # back on its module path, below.
    owners = {}
    for f, (items, _) in candidates.items():
        for name in items:
            owners.setdefault(name, set()).add(f)

    # One read of the repository, cached, because there are three separate
    # questions to ask of every file (variants, associated items, mentions) and
    # reading each file once per question was most of a 78-second runtime.
    # Roughly 4500 files of source; the peak cost is bounded and paid once.
    tree = []
    for f in rust_files(base):
        try:
            tree.append((f, f.read_text(encoding="utf-8", errors="replace").split("\n")))
        except OSError:
            continue

    # Enum variants and associated items anywhere in the repository, for the
    # reason in `variant_names` and `member_names`: neither is an owner, but
    # each spoils a spelling, and a spelling is the whole of the evidence.
    variants = {}
    for f, lines in tree:
        for name in variant_names(lines) | member_names(lines):
            variants.setdefault(name, set()).add(f)

    unambiguous = {
        n
        for n, fs in owners.items()
        if len(fs) == 1 and not (variants.get(n, set()) - fs)
    }

    # The module path is the other edge: `crate::power::Foo`,
    # `use crate::power;`.  A file stem is not a unique token either
    # (`session` names both a module and a hundred variables), so this
    # requires the `::` -- a path segment, not a word.
    #
    # And it is counted **only within the same crate**, because a bare stem is
    # not crate-qualified and modules of one name exist in several.  Lane A
    # has a `kernel/src/fs/a11y.rs`; `kshell.rs` calls `a11y::register_tool`,
    # and on the strength of that the desktop's unrelated 2291-line
    # `gui/desktop/src/a11y.rs` reported as reached.  Restricting stem edges
    # to one crate costs nothing real: a *cross*-crate user must write the
    # item's name too (`desktop::a11y::AccessibilityConfig`), which the item
    # edge already counts.
    stems = {f.stem for f in candidates}
    path_use = re.compile(r"\b([a-z_][a-z0-9_]*)\s*::")

    crate_cache = {}

    def crate_of(path):
        """The directory of the nearest enclosing `Cargo.toml`, or None."""
        key = path.parent
        if key not in crate_cache:
            d = key
            while True:
                if (d / "Cargo.toml").is_file():
                    crate_cache[key] = d
                    break
                if d.parent == d:
                    crate_cache[key] = None
                    break
                d = d.parent
        return crate_cache[key]

    # The *other* half of the module-path edge: a crate-qualified path, which
    # is how a different crate names the same module -- `guitk::table::Table`.
    # Restricting stem edges to one crate (above) made these invisible, and
    # they are not rare: `gui/toolkit/src/table.rs` is imported by
    # `apps/defrag`, `apps/diskanalyzer`, `apps/filesearch` and `flashcards`
    # as `use guitk::table::{Column, Fit, Table};`, and every one of its item
    # names is too common to survive the ambiguity filter.  Reported as an
    # island, it was simply wrong.
    #
    # Matched against the crate's *package name* rather than any leading
    # segment, so `super::a11y::stats()` in the kernel does not vouch for the
    # desktop's `a11y` -- which is the collision that motivated crate-scoping
    # in the first place.
    crate_names = {}
    for d in {crate_of(f) for f in candidates if crate_of(f)}:
        try:
            manifest = (d / "Cargo.toml").read_text(encoding="utf-8")
        except OSError:
            continue
        m = re.search(r'^\s*name\s*=\s*"([^"]+)"', manifest, re.M)
        if m:
            crate_names[d] = m.group(1).replace("-", "_")

    qualified = re.compile(
        r"\b(" + "|".join(sorted(map(re.escape, set(crate_names.values())))) + r")"
        r"\s*::\s*([a-z_][a-z0-9_]*)\b"
    ) if crate_names else None

    # `{name -> {file}}` and `{stem -> {file}}`, split by whether the mention
    # was inside a `#[cfg(test)]` item.  The split matters: a module named
    # only by other files' tests is a *test helper*, which is a benign and
    # complete explanation, and lumping it in with unreferenced code is how a
    # report earns the reputation of crying wolf.
    hits = {"prod": {}, "test": {}}

    # Which crates each file so much as names.  Used to scope the *item* edge
    # to plausible users; see `reached_by`.
    all_crates = set(crate_names.values())
    crate_mentions = {}

    for f, lines in tree:
        spans = test_spans(lines)
        for i, raw in enumerate(lines):
            if REEXPORT.match(raw):
                continue
            # Inlined fast path: 3.5M calls, and most lines have neither.
            line = raw if ('"' not in raw and "//" not in raw) else code_only(raw)
            where = "test" if in_spans(i, spans) else "prod"
            for name in IDENT.findall(line):
                if name in unambiguous:
                    hits[where].setdefault(name, set()).add(f)
                if name in all_crates:
                    crate_mentions.setdefault(f, set()).add(name)
            for seg in path_use.findall(line):
                if seg in stems:
                    key = ("mod", seg, crate_of(f))
                    hits[where].setdefault(key, set()).add(f)
            if qualified:
                for crate, seg in qualified.findall(line):
                    if seg in stems:
                        hits[where].setdefault(("qual", crate, seg), set()).add(f)

    def plausible(f, mentioners):
        """`mentioners`, minus files that cannot be naming *this* module's item.

        The item edge is matched by spelling across the whole repository, and
        an unambiguous spelling is only unambiguous *among candidates* -- lane
        A and lane B are not scanned for owners, so a type of the same name
        over there vouches for a lane-C module it has never heard of.  That is
        how `gui/desktop/src/context_ext.rs` (2042 lines) reported as reached:
        its one surviving name, `ContextTarget`, is also
        `kernel/src/fs/contextmenu.rs`'s own enum, in a crate that does not
        depend on the desktop shell at all.

        The rule that removes it costs nothing real.  A mention from inside
        the same crate always counts.  A mention from *outside* counts only if
        that file also names the owning crate somewhere -- which a genuine
        cross-crate user must, since it can reach the item no other way than
        `use guitk::table::Table;` or `guitk::table::Table`.  The four apps
        that import `guitk::table` all say `guitk`; `contextmenu.rs` never
        says `desktop`.
        """
        home = crate_of(f)
        own = crate_names.get(home)
        return {
            g
            for g in mentioners
            if g != f and (crate_of(g) == home or (own and own in crate_mentions.get(g, ())))
        }

    def reached_by(f, items, where):
        for n in items:
            if n in unambiguous and plausible(f, hits[where].get(n, set())):
                return True
        keys = [("mod", f.stem, crate_of(f))]
        own = crate_names.get(crate_of(f))
        if own:
            keys.append(("qual", own, f.stem))
        return any(hits[where].get(k, set()) - {f} for k in keys)

    islands = []
    for f, (items, length) in candidates.items():
        if reached_by(f, items, "prod"):
            continue
        test_only = reached_by(f, items, "test")
        ambiguous = sorted(n for n in items if n not in unambiguous)
        islands.append((length, f, items, test_only, ambiguous))

    islands.sort(key=lambda r: (r[3], r[0]), reverse=True)

    # The ledger counts hard islands only.  A test-only helper is a complete
    # and benign explanation, and pinning those would make the gate fire on a
    # module that is behaving exactly as intended.
    hard_paths = {f.as_posix() for _, f, _, test_only, _ in islands if not test_only}

    if mode == "pin":
        body = "\n".join(sorted(hard_paths))
        BASELINE.parent.mkdir(parents=True, exist_ok=True)
        # `newline=""` because the default is text mode, which on Windows turns
        # every `\n` into `\r\n`. This one matters more than most sites in the
        # sweep: the baseline is a *tracked* `.txt`, so a `--pin` run from a
        # Windows worktree would commit the exact corruption
        # `scripts/check-eol.py` refuses builds over -- and that gate reads this
        # file. See `known-issues.md` -> `TD-B-SIX-TRACKED-FILES-HELD-CRLF-...`.
        BASELINE.write_text(BASELINE_HEADER + body + "\n", encoding="utf-8", newline="")
        print(f"pinned {len(hard_paths)} island(s) to {BASELINE.as_posix()}")
        return 0

    if mode == "check":
        pinned = read_baseline()
        if pinned is None:
            print(
                f"{BASELINE.as_posix()} is missing -- run --pin to create it",
                file=sys.stderr,
            )
            return 2
        added = sorted(hard_paths - pinned)
        healed = sorted(pinned - hard_paths)
        for p in healed:
            print(f"reached now, drop from the baseline: {p}")
        if added:
            print(
                f"\n{len(added)} module(s) define public items that nothing"
                " outside them names:"
            )
            for p in added:
                print(f"  {p}")
            print(
                "\nA module with no caller is not a feature; `cargo build` cannot"
                "\nwarn about a `pub` item and the test suite reports it green."
                "\nWire it up, delete it, or -- if you are knowingly deferring --"
                "\nadd it to the baseline in the same commit, with the reason in"
                "\nthe commit message.  Full report: run with no arguments."
            )
            return 1
        print(
            f"no new islands ({len(hard_paths)} pinned"
            + (f", {len(healed)} now reached" if healed else "")
            + ")"
        )
        return 0

    for length, f, items, test_only, ambiguous in islands:
        names = sorted(items, key=lambda n: items[n])
        shown = ", ".join(names[:6]) + (", ..." if len(names) > 6 else "")
        tag = "  [test helper: other files' tests use it]" if test_only else ""
        print(f"\n{f.as_posix()}  --  {length} lines, {len(items)} public item(s){tag}")
        print(f"  {shown}")
        if ambiguous:
            others = sorted(
                {
                    g.as_posix()
                    for n in ambiguous
                    for g in owners.get(n, set()) | variants.get(n, set())
                    if g != f
                }
            )
            print(
                f"  shares {len(ambiguous)} name(s) with another module"
                f" -- {', '.join(ambiguous)}"
            )
            print(f"    also spelled in: {', '.join(others[:4])}")

    hard = [r for r in islands if not r[3]]
    total_lines = sum(r[0] for r in hard)
    print(
        f"\n{len(hard)} island module(s), {total_lines} lines,"
        f" out of {len(candidates)} library module(s) scanned"
        f" ({len(islands) - len(hard)} further test-only helper(s) listed above)."
    )
    print(
        "An island defines top-level public items and no other file in the"
        " repository names\nany of them or its module path, outside tests and"
        " bare re-exports.  Triage by asking\nwhich *other* module covers the"
        " same subject matter and is wired up -- two models of\none setting is"
        " the finding that matters; a module waiting for its caller is not.\n"
        "A `shares N name(s)` line is that finding already half-proven: the"
        " same noun is\nmodelled twice, and this module is the copy nobody"
        " calls."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
