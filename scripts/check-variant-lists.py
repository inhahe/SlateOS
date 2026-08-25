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

Scope: only lists whose *name* claims totality (`ALL`, `ALL_*`, `EVERY_*`) are
checked. That is not an invented convention -- it is the one the tree already
follows. Every deliberate subset found when this was written says so in its
name and in a doc comment giving the reason: `ShellControlAction::ZONELESS`
(the zone actions are generated, and `SnapSlot::all` is their list),
`Category::EXPENSE_CATS` (income and investment are not expenses),
`PREVIEW_ACTIONS` (the three toolbar buttons, not the two other ways to make a
`PreviewButton`), `SliderId::FIXED` (the per-app volumes are state, not a
constant). Checking those against the variant count would report four
non-problems, and a gate that cries wolf four times is a gate nobody reads. So
the rule is: name it `ALL` and it is checked; name it anything else and the doc
comment beside it is what says why it is short.

Disagreement is reported *in either direction*, since a list longer than its
enum means the pair has drifted just as surely.

It is a heuristic, not a parser: it skips lists whose element type is not an
enum it can find, and skips an enum name that resolves ambiguously across
files. Exit status is 1 if any mismatch is found, so it can be run as a gate.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOTS = ["gui", "apps", "net", "pkg"]

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


def enum_variants(text: str, start: int) -> int:
    """Count top-level variants of the enum whose body opens at `start`."""
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
        return -1

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
    names = [
        v.strip()
        for v in stripped.split(",")
        if re.match(r"^\s*[A-Z]\w*\s*(=\s*[^,]+)?\s*$", v)
    ]
    return len(names)


def enclosing_impl(text: str, pos: int) -> str | None:
    last = None
    for m in IMPL_RE.finditer(text, 0, pos):
        last = m.group(1)
    return last


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


def self_test() -> int:
    """Check the variant counter against sources counted by hand.

    Also pins the one shape it knowingly does *not* handle -- a generic enum,
    whose `<T>` stops `ENUM_RE` matching at all. That is a safe failure: an
    uncounted enum is simply absent from the table, so lists naming it are
    skipped rather than misjudged. It is asserted here so the day someone
    teaches the regex about generics, they find out this was deliberate.
    """
    bad = 0
    for label, src, want in SELF_TESTS:
        m = ENUM_RE.search(src)
        got = enum_variants(src, m.start()) if m else -1
        ok = got == want
        bad += not ok
        print(f"{'ok  ' if ok else 'FAIL'}  {label}: {got} (want {want})")

    generic = "enum A<T> { One(T), Two }"
    ok = ENUM_RE.search(generic) is None
    bad += not ok
    print(f"{'ok  ' if ok else 'FAIL'}  a generic enum is skipped, not miscounted")

    print(f"\n{len(SELF_TESTS) + 1} cases, {bad} failed")
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    if "--self-test" in argv:
        return self_test()
    verbose = "--list" in argv
    root = pathlib.Path(__file__).resolve().parent.parent
    problems: list[str] = []
    checked: list[str] = []
    subsets = 0
    unresolved = 0

    # One pass to collect every enum in the tree, so a list can name an enum
    # that lives in another file.
    counts: dict[str, dict[str, int]] = {}
    files: list[pathlib.Path] = []
    for r in ROOTS:
        for d in sorted(root.glob(f"{r}*")):
            if d.is_dir():
                files.extend(sorted(d.rglob("*.rs")))
    files = [f for f in files if "target" not in f.parts]
    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for m in ENUM_RE.finditer(text):
            n = enum_variants(text, m.start())
            if n > 0:
                counts.setdefault(m.group(1), {})[str(path)] = n

    for path in files:
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        for m in LIST_RE.finditer(text):
            name, declared = m.group(2), int(m.group(3))
            elem = ELEM_RE.search(m.group(0)).group(1)
            if elem == "Self":
                elem = enclosing_impl(text, m.start())
                if elem is None:
                    continue
            if elem not in counts:
                continue
            if not TOTAL_RE.match(name):
                subsets += 1
                continue
            # Prefer the count from this same file, since a name can repeat.
            per_file = counts[elem]
            if len(set(per_file.values())) > 1 and str(path) not in per_file:
                unresolved += 1
                continue  # ambiguous across files; do not guess
            actual = per_file.get(str(path)) or next(iter(per_file.values()))
            rel = path.relative_to(root).as_posix()
            line = text.count("\n", 0, m.start()) + 1
            checked.append(f"{rel}:{line}: {name}: [{elem}; {declared}]")
            if actual != declared:
                problems.append(
                    f"{rel}:{line}: {name}: [{elem}; {declared}] "
                    f"but `enum {elem}` has {actual} variants"
                )

    if verbose:
        for c in checked:
            print(c)
        print()
    for p in problems:
        print(p)
    print(
        f"{len(checked)} exhaustive lists checked, {len(problems)} out of step; "
        f"{subsets} named as subsets and not checked, "
        f"{unresolved} skipped as ambiguous"
    )
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
