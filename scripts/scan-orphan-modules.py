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

**What it deliberately does not count as a mention.**  A bare re-export
(`pub use power::PowerManager;`) is plumbing, not a caller: it widens the
item's visibility without anyone having used it, and counting it would make
every re-exported island look reached.  Mentions inside `#[cfg(test)]` do not
count -- a test is not a user -- though a module reached *only* from other
files' tests is reported separately as a test helper, which is a complete and
benign explanation.  Comments do not count either; see `strip_comment`.

**What it cannot see.**  A glob re-export (`pub use power::*;`) followed by an
unqualified use in a third file is invisible as an edge to the module, though
the *item* mention in that third file is still counted, so the module is
correctly reported as reached.  Macro-generated paths are invisible.  Names
shared with another module or with any enum variant are dropped from the
evidence entirely (see `variant_names`), so a module all of whose items have
common names rests on its module path alone.  It never proves a module is
dead; it produces a short list worth reading.
"""

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


def strip_comment(line):
    """`line` with any trailing `//` comment removed.

    Prose is not a caller, and in a tree that documents itself as heavily as
    this one that distinction decides findings.  `user_accounts.rs` defines
    `Avatar` and nothing outside it uses the type -- but `login_screen.rs`
    says "Avatar icon character (placeholder for real avatar images)" in a doc
    comment, and `apps/contacts` says "// Avatar circle".  Counting those, the
    module reported as reached.

    The `//` is ignored inside a string literal, so a `"https://..."` does not
    truncate the line.  Char literals and raw strings with embedded quotes can
    still fool it; the failure is to keep a comment, which can only *hide* an
    island, and every island printed is checked by hand anyway.
    """
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
            return line[:i]
    return line

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


def rust_files(base):
    for f in sorted(base.rglob("*.rs")):
        if "target" in f.parts:
            continue
        yield f


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


def public_items(lines):
    """`{name: line_no}` for every top-level public item defined outside tests."""
    spans = test_spans(lines)
    items = {}
    for i, line in enumerate(lines):
        m = PUB_ITEM.match(line)
        if m and not in_spans(i, spans):
            items.setdefault(m.group(1), i + 1)
    return items


def main():
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
        print("no candidate modules found -- wrong working directory?")
        return 1

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

    # Enum variants anywhere in the repository, for the reason in
    # `variant_names`: a variant is not an owner, but it spoils a spelling.
    variants = {}
    for f in rust_files(base):
        try:
            lines = f.read_text(encoding="utf-8", errors="replace").split("\n")
        except OSError:
            continue
        for name in variant_names(lines):
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
    for f in rust_files(base):
        try:
            lines = f.read_text(encoding="utf-8", errors="replace").split("\n")
        except OSError:
            continue
        spans = test_spans(lines)
        for i, raw in enumerate(lines):
            if REEXPORT.match(raw):
                continue
            line = strip_comment(raw)
            where = "test" if in_spans(i, spans) else "prod"
            for name in IDENT.findall(line):
                if name in unambiguous:
                    hits[where].setdefault(name, set()).add(f)
            for seg in path_use.findall(line):
                if seg in stems:
                    key = ("mod", seg, crate_of(f))
                    hits[where].setdefault(key, set()).add(f)
            if qualified:
                for crate, seg in qualified.findall(line):
                    if seg in stems:
                        hits[where].setdefault(("qual", crate, seg), set()).add(f)

    def reached_by(f, items, where):
        keys = [n for n in items if n in unambiguous]
        keys.append(("mod", f.stem, crate_of(f)))
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
