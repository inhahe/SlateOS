#!/usr/bin/env python3
"""Find hand-written variant lists that have fallen behind their enum.

A list like

    const ALL_STATES: [ConnectionState; 6] = [ ... ];
    pub const ALL: [Self; 4] = [ ... ];

is a claim that it names *every* variant, and nothing in the language checks
it: adding a variant to the enum does not make the list longer, it just makes
the list quietly incomplete. Where the list drives a test loop, the new variant
becomes the one case nothing asks about -- which is `known-issues.md` lesson 42
("a test that re-derives the answer proves its own copy") in the form the
compiler cannot catch.

"Cannot" is measured, not assumed. The construction that *would* catch it is

    const _: () = assert!(Foo::ALL.len() == core::mem::variant_count::<Foo>());

and on this tree's host toolchain (stable 1.95.0, checked 2026-08-25) that is
`error[E0658]: use of unstable library feature 'variant_count'`. Putting every
GUI crate on nightly to get one assertion is a worse trade than this script, so
the check lives out here. If `variant_count` ever stabilises, delete this file
and write the assertion next to each list instead -- an error at the list beats
a report about the list.

The weaker in-language guard, which several lists already carry, is an
exhaustive `match` over the enum in the same test: adding a variant breaks that
match's compile, which lands the author in the test that owns the list. That is
a prompt, not a proof -- naming the variant in the match satisfies the compiler
whether or not it also reaches the array -- so it complements this check rather
than replacing it. `ShellControlAction::all_really_is_every_action` is the
worked example.

Scope: **every** list whose element type resolves to an enum is checked, and a
list that is deliberately short says so in `scripts/variant-lists-partial.txt`
with a reason.

That is the second design. The first scoped the check by *name* -- `ALL`,
`ALL_*`, `EVERY_*` -- on the reasoning that this was the convention the tree
already followed, and that checking the rest would cry wolf over deliberate
subsets like `ShellControlAction::ZONELESS` or `Category::EXPENSE_CATS`. The
observation was true and the conclusion was wrong, for a reason worth keeping:
**a population chosen by name inherits the vocabulary of whoever wrote the
code, and it fails toward silence.** A list that ought to be total and happens
to be called `FLEET`, `VIEW_MODES` or `DIFFICULTIES` gets no check at all, and
nothing anywhere says so. Twenty-one such lists were found when the tool was
first asked the question (`TD-C-TWENTY-ONE-LISTS-ARE-EXHAUSTIVE-BY-ACCIDENT`).

Inverting the default swaps a silent failure for a loud one. Forgetting to
opt in could not be noticed by anybody; forgetting to opt out is a failing gate
that asks for the reason in writing. The "crying wolf" cost is real and is paid
once per list, in a file where the reason stays readable next to the others.

What is compared is **which variants are named, not how many**. Counting was
the first design here too, and it has a hole: `[A, A, C]` over `enum { A, B, C }`
has three entries and three variants, and is missing `B`. Comparing names also
makes the check meaningful for an enum whose variants carry data, where the
array is a different length from the variant count by construction --
`apps/sudoku`'s `KEYPAD` lists `Target::Digit(1)` through `Digit(9)`, fourteen
entries over ten variants, and no count could ever say anything useful about
it.

The name-based rule is still enforced in one direction: a list called `ALL` or
`EVERY_*` may not appear in the exceptions file. A name claiming totality and a
record saying totality is not required are a contradiction, and the tool
refuses it rather than picking one. A subset named `ALL` is the same defect
wearing the other hat.

It is a heuristic, not a parser: it skips lists whose element type is not an
enum it can find, and skips an enum name that resolves ambiguously across
files. Exit status is 1 if any mismatch is found, so it can be run as a gate.

Resolution is *scoped*, and that is the part to preserve. A bare element type
like `[Level; 8]` names whatever `Level` is in scope in that file, so the
candidate enums are tried nearest-first: the same file, then the same crate,
and only then another crate -- and a cross-crate hit additionally requires the
file to `use` the name, because a bare identifier from another crate cannot be
in scope without one. Matching on the bare name alone is unsound and was
observed to be: `gui/keylayout` has a `struct Level` with an eight-element
`ALL_LEVELS`, and `kernel/src/klog.rs` has an unrelated five-variant `enum
Level`, so the first tree-wide run reported the pair as drifted. Every failure
to resolve is a *skip*, and every skip is counted in the summary line -- a run
that resolves nothing must not read like a clean one.
"""

from __future__ import annotations

import pathlib
import re
import sys

import selftestflag

# Every directory in the tree that holds first-party Rust, matched as a prefix
# (`net` picks up netipc/netproto/netring). Lane C shipped this checking `gui`,
# `apps`, `net*` and `pkg` only, because that is where it had been falsified;
# widening it was lane A's job and the offer was in the request. The widened run
# is what turned up the `Level` collision described in the docstring, which is
# the argument for keeping it wide: a gate scoped to one lane is a gate whose
# unsoundness only the next lane finds.
#
# Build outputs are excluded by the `target` filter below rather than here, so
# a new crate needs no entry as long as its directory starts with one of these.
ROOTS = [
    "apps",
    "bench",
    "blockbuf",
    "byteread",
    "gui",
    "init",
    "kernel",
    "md5",
    "net",
    "pkg",
    "posix",
    "pwkdf",
    "randrange",
    "services",
    "sha1",
    "sha2",
    "textfind",
    "textfmt",
    "tzrules",
    "userspace",
    "yamldoc",
]

# `pub enum Foo {` / `enum Foo {` -- capturing the body up to the matching brace
# is done by hand below, because variants can contain braces (struct variants).
ENUM_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+([A-Za-z_]\w*)\s*\{", re.M)

# `const NAME: [Type; N] = [` -- the `= [` is what says it is a list literal
# rather than a type alias or a function signature.
LIST_RE = re.compile(
    r"^(\s*)(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z_][A-Z0-9_]*)\s*:\s*"
    r"\[\s*(?:Self|[A-Za-z_]\w*)\s*;\s*(\d+)\s*\]\s*=\s*\[",
    re.M,
)

# The element type, kept separately so `Self` can be resolved against the
# enclosing `impl`.
ELEM_RE = re.compile(r"\[\s*(Self|[A-Za-z_]\w*)\s*;\s*\d+\s*\]")

IMPL_RE = re.compile(r"^\s*impl(?:\s*<[^>]*>)?\s+([A-Za-z_]\w*)\s*(?:\{|where)", re.M)

# Types that are *not* enums. Collected because "I found no enum by this name"
# and "this name is a struct" are different facts: the second one says the
# element type is definitively not the enum found in some other crate, which is
# exactly the collision that made a tree-wide run report a false drift.
NONENUM_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?"
    r"(?:struct|union|type)\s+([A-Za-z_]\w*)\s*(?:<|\{|\(|;|=|:)",
    re.M,
)

# A name that claims to hold every variant. See the module docstring for why
# the check is scoped this way rather than applied to every list.
TOTAL_RE = re.compile(r"^(?:ALL|EVERY)(?:_[A-Z0-9_]+)?$")


def strip_comments(text: str) -> str:
    """Remove `//`-to-newline and `/* */` comments.

    Done before any brace counting, because a comment is exactly the place an
    unbalanced brace or bracket is allowed to appear.
    """
    out = []
    i = 0
    n = len(text)
    while i < n:
        two = text[i : i + 2]
        if two == "//":
            j = text.find("\n", i)
            i = n if j < 0 else j
        elif two == "/*":
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def strip_attributes(text: str) -> str:
    """Remove `#[...]` and `#![...]`, matching brackets so nesting survives.

    `#[cfg_attr(test, derive(Debug))]` has no nested bracket, but `#[doc =
    "[link]"]` does, and a regex that stops at the first `]` would leave a
    stray `"]` behind to be parsed as a variant.
    """
    out = []
    i = 0
    n = len(text)
    while i < n:
        if text[i] == "#" and text[i + 1 : i + 2] in ("[", "!"):
            j = i + 1
            if text[j] == "!":
                j += 1
            if j < n and text[j] == "[":
                depth = 0
                while j < n:
                    if text[j] == "[":
                        depth += 1
                    elif text[j] == "]":
                        depth -= 1
                        if depth == 0:
                            j += 1
                            break
                    j += 1
                i = j
                continue
        out.append(text[i])
        i += 1
    return "".join(out)


def enum_variants(text: str, start: int) -> tuple[str, ...]:
    """Top-level variant names of the enum whose body opens at `start`.

    Names rather than a count, because a count cannot answer the question
    the gate is really asking. A list of `[A, A, C]` over `enum { A, B, C }`
    has three entries and three variants, and is missing `B`. Comparing
    lengths passes it; comparing names does not.
    """
    depth = 0
    i = start
    body_start = None
    while i < len(text):
        ch = text[i]
        if ch == "{":
            depth += 1
            if depth == 1:
                body_start = i + 1
        elif ch == "}":
            depth -= 1
            if depth == 0:
                body = text[body_start:i]
                break
        i += 1
    else:
        return ()

    # Order matters: comments and attributes go first, because both may contain
    # the brackets the nesting pass counts. Stripping `#[default]` *after* the
    # nesting pass leaves a bare `#` glued to the variant that follows it, and
    # that variant then fails to look like a variant -- which is how an earlier
    # version of this script reported `CursorShape` as having 12 of its 13.
    body = strip_attributes(strip_comments(body))

    # Collapse struct and tuple variants so each counts once. Angle brackets are
    # deliberately *not* counted: `<` is ambiguous in Rust source (`A = 1 << 3`
    # is a discriminant, not a generic), and generics only ever appear nested
    # inside the parens or braces that are counted.
    out = []
    depth = 0
    for ch in body:
        if ch in "{([":
            depth += 1
        elif ch in "})]":
            depth -= 1
        elif depth == 0:
            out.append(ch)
    stripped = "".join(out)
    # The identifier only. `Rank { Ace = 1, ... }` has a discriminant on every
    # variant, and while counting never cared, naming does: keeping the `= 1`
    # would make `Ace = 1` the variant's name and no array would ever mention
    # it, which reported `apps/freecell` and `apps/solitaire` as missing all
    # thirteen of the ranks they list in full.
    names = []
    for v in stripped.split(","):
        hit = re.match(r"^\s*([A-Z]\w*)\s*(?:=\s*[^,]+)?\s*$", v)
        if hit:
            names.append(hit.group(1))
    return tuple(names)


def enclosing_impl(text: str, pos: int) -> str | None:
    last = None
    for m in IMPL_RE.finditer(text, 0, pos):
        last = m.group(1)
    return last


def crate_of(path: pathlib.Path, root: pathlib.Path) -> str:
    """The nearest ancestor holding a `Cargo.toml`, as the scope unit.

    A module would be the exact unit, but a crate is the one that can be read
    off the filesystem, and it is the boundary that matters here: within a
    crate a bare type name needs no `use`, across one it always does.
    """
    d = path.parent
    while True:
        if (d / "Cargo.toml").is_file():
            return str(d)
        if d == root or d.parent == d:
            return str(path.parent)
        d = d.parent


def uses_name(text: str, name: str) -> bool:
    """Whether a `use` in this file brings `name` in by that name.

    Deliberately blind to globs: `use super::*` does bring names in, but it
    says nothing about *which*, so treating it as a hit would restore the bare
    name matching this function exists to replace. A glob therefore fails to
    resolve, and a failure to resolve is a skip.
    """
    pat = re.compile(r"^\s*(?:pub\s+)?use\s[^;]*\b" + re.escape(name) + r"\b", re.M)
    return pat.search(text) is not None


# The variant named in an expression like `Self::Any` or `Target::Digit(1)`.
# Nested paths are matched too, so `Target::Level(Difficulty::Easy)` yields
# both `Level` and `Easy`. The extra name is harmless: the check asks whether
# every variant of one enum appears, so a name belonging to some other enum
# simply never matches. The residual risk is a false *pass* where two enums
# share a variant name and one is nested inside the other -- which is a
# narrower failure than the one this replaces, and fails in the direction that
# does not invent work.
MENTION_RE = re.compile(r"(?:Self|[A-Z]\w*)\s*::\s*([A-Z]\w*)")

# Lists that are not required to hold every variant, and why. Keyed by path and
# `Enum::NAME` rather than by line, so moving a list does not rot the record.
PARTIAL_FILE = pathlib.Path(__file__).resolve().parent / "variant-lists-partial.txt"


def array_body(text: str, after_open: int) -> str | None:
    """The literal between `= [` and its matching `]`.

    `after_open` is the index just past the opening bracket, which is where
    `LIST_RE` ends. Bracket depth rather than a regex, because the elements may
    themselves be indexed or contain arrays.
    """
    depth = 1
    i = after_open
    while i < len(text):
        ch = text[i]
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
            if depth == 0:
                return text[after_open:i]
        i += 1
    return None


def mentioned(body: str) -> set[str]:
    """Every variant name appearing in an array literal.

    Comments are stripped first, so a variant that was commented out counts as
    absent -- which is the whole point, since commenting one out is one of the
    ways a list silently stops being exhaustive.
    """
    return {m.group(1) for m in MENTION_RE.finditer(strip_comments(body))}


def load_partial(path: pathlib.Path) -> dict[tuple[str, str], str]:
    """The recorded not-required-to-be-total lists: `path<TAB>Enum::NAME<TAB>why`.

    Keyed by the *element type* as well as the constant's name, because a
    name alone is not unique within a file: `apps/settings` has both a
    `DropdownId::FIXED` and a `SliderId::FIXED`, and a record meant for the
    first quietly excused the second -- which was exhaustive and wanted
    checking. A key that can match the wrong thing is a silent hole in the
    gate, which is the one kind of bug this whole file exists to prevent.
    """
    out: dict[tuple[str, str], str] = {}
    if not path.exists():
        return out
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = [p.strip() for p in line.split("\t") if p.strip()]
        if len(parts) < 3:
            continue
        out[(parts[0], parts[1])] = parts[2]
    return out

def resolve_enum(
    elem: str,
    path: str,
    crate: str,
    counts: dict[str, dict[str, tuple[str, ...]]],
    nonenum: dict[str, set[str]],
    crates: dict[str, str],
    text: str,
) -> tuple[tuple[str, ...] | None, str]:
    """Variant names of `elem` as seen from `path`, or why they could not be had.

    Nearest-first, because that is how Rust resolves a bare name: the same
    file, then the same crate, then another crate that this file imports from.
    Anything that does not land on exactly one count returns `None` and a
    reason, and the caller counts it as a skip -- guessing here is how an
    unrelated same-named enum in a distant crate becomes a confident report.
    """
    per_file = counts.get(elem, {})

    if path in per_file:
        return per_file[path], ""

    same_crate = {p: n for p, n in per_file.items() if crates[p] == crate}
    if same_crate:
        vals = set(same_crate.values())
        if len(vals) == 1:
            return vals.pop(), ""
        return None, f"`{elem}` is several different enums within this crate"

    # No enum by this name in the crate. If a *non*-enum by this name is here,
    # that settles it: the element type is that, and any enum elsewhere is a
    # different type that happens to share a name.
    if any(crates[p] == crate for p in nonenum.get(elem, ())):
        return None, f"`{elem}` is a struct/type alias here, not an enum"

    if not per_file:
        return None, f"no `enum {elem}` found"

    if not uses_name(text, elem):
        return None, f"`{elem}` is only an enum in another crate, not imported here"

    vals = set(per_file.values())
    if len(vals) == 1:
        return vals.pop(), ""
    return None, f"`{elem}` names different enums in different crates"


# Sources whose variant count is known by inspection, kept because the counter
# is a heuristic and a heuristic nobody falsifies is just a confident guess.
# Every case here is one that broke it or could: the `#[default]` case is the
# real bug this found (it reported `CursorShape` as 12 of its 13, because the
# nesting pass ate `[default]` and left a bare `#` glued to `Arrow`).
SELF_TESTS: list[tuple[str, str, int]] = [
    (
        "attribute before a variant",
        """
        enum A {
            #[default]
            One,
            Two,
        }
        """,
        2,
    ),
    (
        "attribute with a nested bracket",
        """
        enum A {
            #[doc = "see [`Other`]"]
            One,
            Two,
        }
        """,
        2,
    ),
    (
        "struct and tuple variants count once each",
        """
        enum A {
            Plain,
            Tuple(Vec<u8>, u32),
            Struct { a: u8, b: u8 },
        }
        """,
        3,
    ),
    (
        "a discriminant containing a shift",
        """
        enum A {
            One = 1 << 3,
            Two = 1 << 4,
        }
        """,
        2,
    ),
    (
        "a doc comment containing braces, commas and brackets",
        """
        enum A {
            /// Shaped like `Foo { a, b }` -- see [`Bar`], not a variant.
            One,
            /// Another, with an unbalanced ) in prose.
            Two,
        }
        """,
        2,
    ),
    (
        "a block comment containing something variant-shaped",
        """
        enum A {
            One,
            /* Removed,
               Gone, */
            Two,
        }
        """,
        2,
    ),
    (
        "a trailing comma is not a variant",
        """
        enum A {
            One,
            Two
        }
        """,
        2,
    ),
]


# Cases about the variant *names*, which the count-based table above cannot
# see: a name that differs from the identifier is invisible to `len()`.
NAME_TESTS: list[tuple[str, str, tuple[str, ...]]] = [
    (
        "a discriminant is not part of the name",
        """
        enum A {
            Ace = 1,
            Two = 2,
        }
        """,
        ("Ace", "Two"),
    ),
    (
        "struct and tuple variants give their bare names",
        """
        enum A {
            One(u8),
            Two { x: u8 },
            Three,
        }
        """,
        ("One", "Two", "Three"),
    ),
]


def self_test() -> int:
    """Check the variant counter against sources counted by hand.

    Also pins the one shape it knowingly does *not* handle -- a generic enum,
    whose `<T>` stops `ENUM_RE` matching at all. That is a safe failure: an
    uncounted enum is simply absent from the table, so lists naming it are
    skipped rather than misjudged. It is asserted here so the day someone
    teaches the regex about generics, they find out this was deliberate.
    """
    bad = 0
    for label, src, want_names in NAME_TESTS:
        m = ENUM_RE.search(src)
        got_names = enum_variants(src, m.start()) if m else ()
        ok = got_names == want_names
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {got_names} (want {want_names})")
    for label, src, want in SELF_TESTS:
        m = ENUM_RE.search(src)
        got = len(enum_variants(src, m.start())) if m else -1
        ok = got == want
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {got} (want {want})")

    generic = "enum A<T> { One(T), Two }"
    ok = ENUM_RE.search(generic) is None
    bad += not ok
    print(f"{'ok  ' if ok else 'FAIL'}  a generic enum is skipped, not miscounted")

    bad += resolve_self_test()
    print(f"\n{len(NAME_TESTS) + len(SELF_TESTS) + 1 + len(RESOLVE_TESTS)} cases, {bad} failed")
    return 1 if bad else 0


# Resolution cases. The counter is only half the gate; the other half decides
# *which* enum a bare element type means, and getting that wrong is the failure
# mode that reports a clean tree as drifted. Case 3 is the live one: it is
# `gui/keylayout`'s `struct Level` against `kernel/src/klog.rs`'s `enum Level`,
# reduced to a fixture so the collision cannot come back unnoticed.
#
# Each case is (label, elem, file, crate, counts, nonenum, file text, expected).
# `expected` is a count, or None for "must not be judged".
RESOLVE_TESTS: list[tuple] = [
    (
        "the enum in the same file wins over one elsewhere",
        "E", "a/src/x.rs", "a",
        {"E": {"a/src/x.rs": 3, "b/src/y.rs": 9}}, {}, "",
        3,
    ),
    (
        "an enum elsewhere in the same crate is used",
        "E", "a/src/x.rs", "a",
        {"E": {"a/src/y.rs": 4}}, {}, "",
        4,
    ),
    (
        "a struct of that name in this crate is not an enum anywhere else",
        "Level", "gui/keylayout/src/tests.rs", "gui/keylayout",
        {"Level": {"kernel/src/klog.rs": 5}},
        {"Level": {"gui/keylayout/src/lib.rs"}},
        "use super::*;",
        None,
    ),
    (
        "another crate's enum needs a use naming it",
        "E", "a/src/x.rs", "a",
        {"E": {"b/src/y.rs": 6}}, {}, "use super::*;",
        None,
    ),
    (
        "and is used when the file does name it",
        "E", "a/src/x.rs", "a",
        {"E": {"b/src/y.rs": 6}}, {}, "use b::thing::E;",
        6,
    ),
    (
        "two crates disagreeing is not resolved even with a use",
        "E", "a/src/x.rs", "a",
        {"E": {"b/src/y.rs": 6, "c/src/z.rs": 7}}, {}, "use b::E;",
        None,
    ),
    (
        "an element type with no enum anywhere is not resolved",
        "E", "a/src/x.rs", "a",
        {}, {}, "",
        None,
    ),
]


def resolve_self_test() -> int:
    bad = 0
    for label, elem, path, crate, counts, nonenum, text, want in RESOLVE_TESTS:
        crates = {p: p.rsplit("/src/", 1)[0] for d in counts.values() for p in d}
        for s in nonenum.values():
            for p in s:
                crates[p] = p.rsplit("/src/", 1)[0]
        crates[path] = crate
        got, why = resolve_enum(elem, path, crate, counts, nonenum, crates, text)
        ok = got == want
        bad += not ok
        shown = got if got is not None else f"skipped ({why})"
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {shown}")
    return bad


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    verbose = "--list" in argv
    root = pathlib.Path(__file__).resolve().parent.parent
    problems: list[str] = []
    checked: list[str] = []
    skipped: list[str] = []
    excused: list[str] = []
    # Every (path, name) the scan actually found, so a record in
    # variant-lists-partial.txt naming something that is gone can be told
    # from one that is merely unresolved.
    seen: set[tuple[str, str]] = set()
    partial = load_partial(PARTIAL_FILE)

    # One pass to collect every enum in the tree, so a list can name an enum
    # that lives in another file, plus every non-enum type -- the second table
    # is what lets "this name is a struct" be said rather than merely "no enum
    # of this name is in scope".
    counts: dict[str, dict[str, tuple[str, ...]]] = {}
    nonenum: dict[str, set[str]] = {}
    files: list[pathlib.Path] = []
    for r in ROOTS:
        for d in sorted(root.glob(f"{r}*")):
            if d.is_dir():
                files.extend(sorted(d.rglob("*.rs")))
    files = [f for f in files if "target" not in f.parts]
    sources: dict[str, str] = {}
    crates: dict[str, str] = {}
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        key = str(path)
        sources[key] = text
        crates[key] = crate_of(path, root)
        for m in ENUM_RE.finditer(text):
            n = enum_variants(text, m.start())
            if n:
                counts.setdefault(m.group(1), {})[key] = n
        for m in NONENUM_RE.finditer(text):
            nonenum.setdefault(m.group(1), set()).add(key)

    for path in files:
        key = str(path)
        text = sources.get(key)
        if text is None:
            continue
        for m in LIST_RE.finditer(text):
            name, declared = m.group(2), int(m.group(3))
            elem = ELEM_RE.search(m.group(0)).group(1)
            if elem == "Self":
                elem = enclosing_impl(text, m.start())
                if elem is None:
                    continue
            rel = path.relative_to(root).as_posix()
            line = text.count("\n", 0, m.start()) + 1
            variants, why = resolve_enum(
                elem, key, crates[key], counts, nonenum, crates, text
            )
            if variants is None:
                # A list whose *name* claims totality and whose element type
                # cannot be resolved is worth reporting -- something claims to
                # be exhaustive and nothing checked it. A list with an ordinary
                # name and an unresolvable element type is usually not a list
                # of variants at all (`[u8; 32]`), so it is not news.
                if TOTAL_RE.match(name):
                    skipped.append(f"{rel}:{line}: {name}: not checked -- {why}")
                continue

            ident = f"{elem}::{name}"
            seen.add((rel, ident))
            body = array_body(text, m.end())
            named = mentioned(body) if body is not None else set()
            missing = [v for v in variants if v not in named]

            reason = partial.get((rel, ident))
            if reason is not None:
                if TOTAL_RE.match(name):
                    problems.append(
                        f"{rel}:{line}: {name}: recorded as not required to be "
                        f"exhaustive, but its own name claims it holds every "
                        f"variant. One of the two is wrong."
                    )
                else:
                    excused.append(f"{rel}:{line}: {name}: [{elem}; {declared}] -- {reason}")
                continue

            if missing:
                # Named, not merely counted -- the point of the check is to say
                # *which* variant fell out. Long tails are trimmed because a
                # list short by eighty names is a list nobody meant to be
                # total, and printing all eighty buries the ones that matter.
                shown = ", ".join(missing[:6])
                if len(missing) > 6:
                    shown += f", and {len(missing) - 6} more"
                problems.append(
                    f"{rel}:{line}: {name}: [{elem}; {declared}] does not name "
                    f"{shown} of `enum {elem}`'s {len(variants)}. Add "
                    f"{'it' if len(missing) == 1 else 'them'}, or say why the list "
                    f"is not required to be exhaustive in {PARTIAL_FILE.name}."
                )
                continue
            checked.append(f"{rel}:{line}: {name}: [{elem}; {declared}]")

    # A record naming a list that is no longer there is not a harmless leftover.
    # It reads as a decision somebody made about the code as it is now, and the
    # list it excused may have been renamed into one that ought to be checked.
    for (rel, ident), reason in sorted(partial.items()):
        if (rel, ident) not in seen:
            problems.append(
                f"{PARTIAL_FILE.name}: {rel}: {ident}: no such variant list any "
                f"more -- the record has rotted and says: {reason}"
            )

    unresolved = len(skipped)
    if verbose:
        for c in checked:
            print(c)
        print()
        # The lists excused from the check, with the reason each was excused.
        # Printed rather than merely counted because this is the population a
        # reader most needs to audit: every one of them is a judgement somebody
        # made, and a wrong one is invisible from the summary line alone.
        for entry in excused:
            print(f"(not required to be total) {entry}")
        print()
        for s in skipped:
            print(s)
        print()
    for p in problems:
        print(p)
    print(
        f"{len(checked)} lists hold every variant, {len(problems)} do not; "
        f"{len(excused)} excused by {PARTIAL_FILE.name}, "
        f"{unresolved} named as total but unresolved; --list names all three groups"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
