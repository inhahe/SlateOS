#!/usr/bin/env python3
"""Refuse an intra-doc link whose target does not exist anywhere in its crate.

WHY THIS EXISTS
---------------
On 2026-09-01 a sweep of `coreutils` found 35 broken `cargo doc` intra-doc
links. They fell into four kinds, and only one of them rots on its own:

    prose eaten as a link       `set[ug]id` -> `[ug]` is not an item
    the path does not name it   `get_header` is really `Df::get_header`
    THE ITEM DOES NOT EXIST     `destination_is_older` is now
                                `destination_is_up_to_date`
    test-only target            `#[cfg(test)]`, so no doc build resolves it

The third kind is what a *rename* leaves behind. Every one of the eight found
named a real function under a name that was no longer real, because renaming a
function does not rename the prose that points at it. That class will come back
the next time anything is renamed, so it is the class worth a gate.

The other three do not need one: rustdoc reports them the moment anyone runs
`cargo doc`, and they do not appear spontaneously.

WHY NOT JUST RUN `cargo doc`
----------------------------
Because no single `cargo doc` invocation sees the whole truth, which is the
other thing that sweep established:

    --target x86_64-pc-windows-gnu      misses everything inside `#[cfg(unix)]`
                                        AND reports five cfg(unix) items as
                                        missing that are perfectly fine
    default (no --document-private-items)
                                        does not resolve links on private
                                        items at all -- a private item's doc
                                        comment is never read

So the honest rustdoc check is three builds on two targets, one of which
(`x86_64-unknown-linux-gnu`) is not otherwise built here and whose `target/`
subtree is pure cost. This checker is a text scan: it needs no toolchain, no
target installed, and no build, and it is blind to `cfg` by construction --
which is exactly right for this class, since a renamed function is missing
under every `cfg`.

WHAT IT DELIBERATELY DOES NOT DO
--------------------------------
It does not resolve paths. `[\\`get_header\\`]` when the item is
`Df::get_header` is a real defect and this will not flag it, because deciding
that needs name resolution and a wrong answer here is worse than no answer: a
gate that cries wolf gets bypassed, and a bypassed gate is not a gate. The rule
is deliberately one-sided -- flag only when the final path segment appears
NOWHERE in the crate as any kind of definition. If a name exists at all, this
stays quiet and leaves the judgement to rustdoc.

Consequently every finding is actionable: the name is simply gone.

WHAT IT READS: `--head`
-----------------------
By default it reads the working tree. With `--head <rev>` it reads that
revision instead, through `gittree.open_tree`, and never touches the disk copy
at all. The push hook passes the commit being published, which is what makes
the gate's verdict a statement about the push rather than about whatever
happens to be lying in the worktree at the time: a dead link introduced by a
commit cannot be hidden by tidying the worktree afterwards, and an uncommitted
one cannot block a push of unrelated, clean commits.

`--paths-from` composes with it. The scope list is a list of *names*, resolved
against the revision's own crate roots -- a directory that does not exist at
that revision matches no crate and simply contributes nothing to the scope.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import os
import re
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gittree  # noqa: E402
from gittree import Tree  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Roots scanned. Lane B's trees; a crate outside these is another lane's to
# gate, and a gate scoped wider than its owner can fix is a gate that blocks
# people who cannot act on it.
ROOTS = ("userspace", "services", "init", "posix")

# --------------------------------------------------------------------------
# Definitions. Anything that can legitimately be the target of an intra-doc
# link and can be spotted without parsing Rust.
#
# Generous on purpose: a name matched here silences the checker, and silence is
# the safe direction. Missing a definition form would produce a false positive,
# which is the failure mode that gets a gate switched off.
# --------------------------------------------------------------------------
# Type definitions specifically, and only by keyword. This is the set a link's
# *head* is tested against, and it is deliberately much narrower than the one
# below.
#
# The distinction is the difference between a working gate and a noisy one.
# `Style::quote_with` is a real method of `quoting::Style`, which `coreutils`
# re-exports as `coreutils::quote::Style`; `Ch::U` is a real variant of
# `ere::ch::Ch`, which `oils` re-exports. Neither type is *defined* in the crate
# that links to it, so neither one's members can be found by reading that
# crate -- but both names still land in the general set below, because some
# unrelated enum has a variant spelled `Ch(Ch)` and some struct has a field
# spelled `style:`. Requiring the head to be keyword-defined here is what tells
# "a type we can read" from "a name that merely occurs".
# One pass, not sixteen. The `ty` group is the narrow type set; every other
# group feeds the general "this name exists" set. Scanning `oils/src/interp.rs`
# -- 100k lines -- once per pattern took the whole scan to 61 seconds, which is
# far too slow for something that runs on every push; merged, it is about four.
#
# The last two branches catch enum variants and struct fields: a bare `Name,` /
# `name: Type,` at the head of an indented line. Both are linkable and neither
# is declared by a keyword, so they have to be caught by shape -- and that
# looseness is exactly why they are kept out of `ty`.
DEF_RE = re.compile(
    r"\b(?:struct|enum|trait|union|type)\s+(?P<ty>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\b(?:fn|mod)\s+(?P<it>[A-Za-z_][A-Za-z0-9_]*)"
    # `const fn hex_len` must yield `hex_len`, not `fn`. A merged alternation
    # matches at the FIRST keyword and then resumes past what it consumed, so a
    # branch that stops at `const` swallows the `fn` and hides the real name --
    # which silently un-defined every `const fn` in the tree and invented ten
    # findings. Separate patterns did not have this problem because each rescanned
    # the whole text.
    r"|\bconst\s+(?:fn\s+)?(?P<cn>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\bstatic\s+(?:mut\s+)?(?P<st>[A-Za-z_][A-Za-z0-9_]*)"
    r"|\bmacro_rules!\s*(?P<mc>[A-Za-z_][A-Za-z0-9_]*)"
    r"|^[ \t]+(?P<va>[A-Z][A-Za-z0-9_]*)\s*[,({]"
    r"|^[ \t]+(?:pub\s+)?(?P<fl>[a-z_][A-Za-z0-9_]*)\s*:",
    re.M,
)

# Everything a `use` brings into scope. Rustdoc resolves against the importing
# module's scope, so `use quoting::escape_os;` makes `[`escape_os`]` a perfectly
# good link in a crate that does not define it -- `tar` does exactly this, and
# treating it as dead was this checker's first false positive.
#
# Every identifier in the statement is taken, module names included. That is
# over-generous by design: an extra name can only silence the checker, and
# silence is the safe direction (see the module docstring).
USE_STMT = re.compile(r"^\s*(?:pub\s+)?use\s+([^;]+);", re.M)
IDENT = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")

# A declarative macro *defining an item named by its first argument* -- the
# idiom that generates `oils`'s `nested_parts` / `nested_parts_mut` from one
# `macro_rules! nested_parts_fn`. Both functions are real, both are linked to,
# and no scan of definition keywords can see either: their names exist only as
# arguments here.
#
# Only invocations of a macro the crate itself declares are read this way, which
# is what keeps the rule from swallowing `assert_eq!(status, 0)` and adding the
# local `status` to the crate's namespace. The cost of that restriction is an
# item generated by an imported macro (`paste!` and friends), which nothing here
# does; if one ever appears it shows up as a finding rather than as silence.
MACRO_CALL = re.compile(
    r"^\s*(?P<mname>[a-z_][A-Za-z0-9_]*)!\s*[(\[{]\s*"
    r"(?P<marg>[A-Za-z_][A-Za-z0-9_]*)\s*[,)\]}]",
    re.M,
)

# What a link target may look like. A path, and nothing else. Without this,
# `` `[` and `]` `` in prose -- "everything between `[` and `]`" -- parses as a
# link whose target is " and ", which strips to the identifier `and`.
PATH_SHAPE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*")

# Roots that mean "not in this crate". `crate`, `self` and `Self` are stripped
# before this is consulted; anything else lowercase with a `::` after it is
# another crate, and every uppercase name is checked normally.
EXTERNAL_ROOTS = {
    "std", "core", "alloc", "libc", "proc_macro", "test",
    "coreutils", "posix", "slateos",
}

# Names that resolve without being defined in the crate: the prelude, the
# primitives, and the handful of std items that get linked bare.
WELL_KNOWN = {
    # primitives
    "bool", "char", "str", "u8", "u16", "u32", "u64", "u128", "usize",
    "i8", "i16", "i32", "i64", "i128", "isize", "f32", "f64", "never",
    # prelude and near-prelude
    "Option", "Some", "None", "Result", "Ok", "Err", "Vec", "String", "Box",
    "Iterator", "IntoIterator", "Clone", "Copy", "Drop", "Default", "Debug",
    "Display", "PartialEq", "Eq", "PartialOrd", "Ord", "Hash", "From", "Into",
    "TryFrom", "TryInto", "AsRef", "AsMut", "Deref", "DerefMut", "Fn", "FnMut",
    "FnOnce", "Send", "Sync", "Sized", "ToString", "ToOwned", "Cow",
    "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque",
    "Path", "PathBuf", "OsStr", "OsString", "CStr", "CString",
    "File", "Command", "Child", "ExitCode", "ExitStatus",
    "Ordering", "Duration", "Instant", "Range", "Rc", "Arc", "Mutex", "RwLock",
    "Cell", "RefCell", "Weak", "Pin", "Wrapping", "NonZero",
    "Write", "Read", "BufRead", "BufReader", "BufWriter", "Seek", "SeekFrom",
    "Error", "ErrorKind", "Metadata", "DirEntry", "ReadDir", "Permissions",
}

# `[`x`]` and `[text](x)`. Rustdoc also accepts `[x]` bare, but that form is
# indistinguishable from ordinary prose in square brackets -- which is the very
# thing the sweep found six of -- so it is left to rustdoc.
#
# The lookahead is load-bearing. In `[`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE)`
# the bracketed part is the link's *display text*, not its target -- the target
# is in the parentheses, and LINK_EXPLICIT already picks it up. Without the
# lookahead every such link is read twice, once correctly and once as a bare
# link to its own label, which is how thirteen working netstack links looked
# dead. `[label][ref]` is the same story with a reference instead.
LINK_BACKTICK = re.compile(r"\[`([^`\]]+)`\](?![(\[])")
LINK_EXPLICIT = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")

DOC_LINE = re.compile(r"^\s*(?:///|//!)(.*)$")

# A markdown *link reference definition*: `[label]: destination`. The label is
# then a working link everywhere in that doc comment, and rustdoc never resolves
# it as an intra-doc path -- `modechange`'s [`ere`] points at a URL this way.
#
# Two findings come from each one if this is not honoured: the use, and the
# definition line itself (which is not a link at all, but looks like one).
#
# Deliberately loose about what follows the colon. CommonMark would demand a
# real destination, and several doc comments here use `[`name`]: prose...` as a
# definition list, which is *not* a valid definition and so does not silence
# rustdoc. But this checker only ever flags a name that exists NOWHERE, and a
# name written in that definition-list style is one the author is describing --
# so treating it as resolved errs in the safe direction, same as everything else
# here.
REF_DEF = re.compile(r"^\s*\[`?([^`\]]+?)`?\]:\s*\S")


def crate_roots(tree: Tree) -> list[str]:
    """Every crate directory (one holding a Cargo.toml) under the scanned roots.

    `files_under` already skips `target/`, so the manifests it yields are the
    project's own -- the `"target" in parts` filter this used to carry is the
    seam's job now, spelled once in `gittree.py` for both implementations
    rather than once per checker.
    """
    out = []
    for root in ROOTS:
        if not tree.is_dir(root):
            continue
        for manifest in tree.files_under(root):
            if manifest.rsplit("/", 1)[-1] == "Cargo.toml":
                out.append(parent_of(manifest))
    return sorted(out)


def parent_of(rel: str) -> str:
    """The directory holding `rel`, or `""` for a path at the repository root.

    `PurePosixPath` is not used for this and the four other one-line path
    manipulations here on purpose: a [`Tree`] path is a `/`-joined string on
    both implementations, and a `PurePath` would tempt the next reader into
    `.resolve()` or `.exists()`, neither of which a revision has.
    """
    head, sep, _ = rel.rpartition("/")
    return head if sep else ""


def units(tree: Tree, crate: str) -> list[tuple[str, list[str], list[str]]]:
    """The crate's compilation units: the library, and each `src/bin` target.

    A package is not a namespace. `coreutils` has about a hundred binaries under
    `src/bin`, and each is its own crate that cannot see the others' items.
    Scanning the package as one blob makes every binary's private types visible
    to every other, which is how `ls.rs` came to be judged against the unrelated
    `enum Style` that `nl.rs` declares -- while `ls`'s actual `Style` is
    `quoting::Style`, whose `quote_with` exists and was reported dead.

    Returns `(name, own, shared)`: the files the unit is responsible for, and
    the library's files, which every unit gets as *definitions* because a binary
    really can reach the library's public items and routinely links to them.
    """
    src = f"{crate}/src" if crate else "src"
    if not tree.is_dir(src):
        return []
    bindir = f"{src}/bin"
    lib = [
        p for p in tree.files_under(src)
        if p.endswith(".rs") and not p.startswith(f"{bindir}/")
    ]
    out: list[tuple[str, list[str], list[str]]] = []
    if lib:
        out.append((crate.rsplit("/", 1)[-1], lib, []))
    if tree.is_dir(bindir):
        for entry, is_dir in tree.entries(bindir):
            name = entry.rsplit("/", 1)[-1]
            if not is_dir and name.endswith(".rs"):
                out.append((name[: -len(".rs")], [entry], lib))
            elif is_dir:
                own = [p for p in tree.files_under(entry) if p.endswith(".rs")]
                if own:
                    out.append((name, own, lib))
    return out


class Defs:
    """What reading a set of Rust files says about the names in them.

    `types` is what is defined by `struct`/`enum`/`trait`/`union`/`type`, and
    answers "can I read this type's members?" -- it is used only for a link's
    head. `scope` is every name defined or imported, and answers "does this name
    exist at all?" -- used only for a link's final segment. Keeping them apart is
    what makes the gate quiet; see DEF_RE for why the head needs the stricter
    test.

    `macros` and `macro_args` are held unresolved because resolving them needs
    the *whole* unit: a binary may invoke a `#[macro_export]`ed macro its library
    declares, so the declaration and the invocation can be in different files.
    """

    def __init__(self) -> None:
        self.types: set[str] = set()
        self.scope: set[str] = set()
        self.macros: set[str] = set()
        self.macro_args: list[tuple[str, str]] = []

    def union(self, other: Defs) -> Defs:
        out = Defs()
        out.types = self.types | other.types
        out.scope = self.scope | other.scope
        out.macros = self.macros | other.macros
        out.macro_args = self.macro_args + other.macro_args
        return out

    def resolve(self) -> Defs:
        """Fold in the item names the crate's own declarative macros generate."""
        for mname, arg in self.macro_args:
            if mname in self.macros:
                self.scope.add(arg)
        return self


def defs_in_text(text: str, d: Defs) -> None:
    """Add one file's definitions and imports to `d`."""
    for m in DEF_RE.finditer(text):
        ty = m.group("ty")
        if ty is not None:
            d.types.add(ty)
            d.scope.add(ty)
            continue
        mc = m.group("mc")
        if mc is not None:
            d.macros.add(mc)
            d.scope.add(mc)
            continue
        for g in ("it", "cn", "st", "va", "fl"):
            v = m.group(g)
            if v is not None:
                d.scope.add(v)
                break
    for m in USE_STMT.finditer(text):
        d.scope.update(IDENT.findall(m.group(1)))
    for m in MACRO_CALL.finditer(text):
        d.macro_args.append((m.group("mname"), m.group("marg")))


def definitions(tree: Tree, files: list[str]) -> Defs:
    """Everything `files` say about the names in them, left unresolved.

    A file that cannot be read is skipped rather than raising, which is
    [`Tree`]'s contract everywhere: a missing file is an answer here, not an
    error. It was a caught `OSError` before the seam and means the same thing.
    """
    d = Defs()
    for f in files:
        text = tree.read_text(f)
        if text is None:
            continue
        defs_in_text(text, d)
    return d


def dependencies(tree: Tree, crate: str) -> set[str]:
    """Names in the crate's `[dependencies]`, which link like crate roots.

    `modechange`'s docs point at [`ere`], the regex crate it depends on. That is
    a working link and reading only `src/` cannot tell.
    """
    manifest = f"{crate}/Cargo.toml" if crate else "Cargo.toml"
    text = tree.read_text(manifest)
    if text is None:
        return set()
    names: set[str] = set()
    in_deps = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith("["):
            in_deps = "dependencies" in line
            continue
        if in_deps and "=" in line and not line.startswith("#"):
            name = line.split("=", 1)[0].strip().strip('"')
            if IDENT.fullmatch(name.replace("-", "_")):
                names.add(name.replace("-", "_"))
                names.add(name)
    return names


def link_targets(line: str):
    """The path each intra-doc link on this line points at."""
    for m in LINK_BACKTICK.finditer(line):
        yield m.group(1)
    for m in LINK_EXPLICIT.finditer(line):
        yield m.group(1)


def split_path(path: str) -> list[str] | None:
    """The path's segments, or None if this is not a path we can judge."""
    # Method-call and macro spellings, generics, and leading sigils. No strip:
    # whitespace anywhere means this was never a path (see PATH_SHAPE).
    p = path.split("<", 1)[0]
    p = p.removesuffix("()").removesuffix("!")
    p = p.lstrip("&")
    if not p or p.startswith("http") or "/" in p:
        return None
    if not PATH_SHAPE.fullmatch(p):
        return None
    segs = [x for x in p.split("::") if x]
    # Strip the roots that mean "in this crate" -- they say where to start
    # looking, not what to look for.
    while segs and segs[0] in ("crate", "self", "Self", "super"):
        segs.pop(0)
    return segs or None


def dead_link(path: str, types: set[str], scope: set[str]) -> bool:
    """Whether `path` names something that exists nowhere in the crate.

    `types` is what the crate defines with `struct`/`enum`/`trait`/`union`/
    `type`; `scope` is every name it defines or imports, plus its declared
    dependencies. A multi-segment path is judged only when its head is in
    `types`, because only then is the member list something reading this crate
    can see. `Stdio::null` and `Style::quote_with` both have heads defined in
    another crate, so their tails are unknowable here -- and guessing would
    produce exactly the false positive that gets a gate switched off.

    A lowercase head is never in `types` either, so `authlib::Authenticator` and
    `deflate::inflate_limited` -- module paths into dependencies -- fall out by
    the same rule rather than needing one of their own.
    """
    segs = split_path(path)
    if segs is None:
        return False
    head, last = segs[0], segs[-1]
    if head in EXTERNAL_ROOTS or head in WELL_KNOWN:
        return False
    if len(segs) > 1 and head not in types:
        return False
    return last not in scope and last not in WELL_KNOWN


class Coverage:
    """How much of the corpus a scan actually looked at.

    WHY THIS EXISTS. "0 dead links" is spelled identically whether the gate
    read nine hundred files or none, so every way of reading nothing is a way
    of passing. `crate_roots` returning nothing is already refused in `main`,
    but that is only the outermost of them: `units` yielding no compilation
    unit, `DOC_LINE` stopping matching, `LINK_BACKTICK`/`LINK_EXPLICIT`
    stopping matching, or `split_path` rejecting every path each empty the
    scan one layer further in, and each looks exactly like a clean tree.

    None of that can be caught by a fixture, because the fixture *is* the
    input that went missing (lane A's discovery-floor rule, requests/
    a-b-yes-to-the-self-test-rule-and-one-half-it-does-not-cover.md S2). The
    only defence is for the scan to say what it inspected and for `main` to
    refuse a verdict when that is implausibly small.

    The counts narrow, outermost first, so a breach names the layer that broke
    rather than merely reporting that something did.
    """

    __slots__ = ("crates", "units", "files", "doc_lines", "targets", "judged")

    def __init__(self) -> None:
        self.crates = 0     # Cargo.toml roots scanned
        self.units = 0      # compilation units (lib + each bin) within them
        self.files = 0      # source files read
        self.doc_lines = 0  # lines matching DOC_LINE -- i.e. doc comments
        self.targets = 0    # link targets extracted from those lines
        self.judged = 0     # targets that were a path shape and got resolved

    def __str__(self) -> str:
        return (f"{self.crates} crate(s), {self.units} unit(s), "
                f"{self.files} file(s), {self.doc_lines} doc line(s), "
                f"{self.targets} link(s), {self.judged} judged")


def scan_file(tree: Tree, f: str, unit: str, types: set[str], scope: set[str],
              cov: Coverage):
    """Every dead link in one file, judged in the unit that compiles it."""
    text = tree.read_text(f)
    if text is None:
        return
    cov.files += 1
    lines = text.splitlines()
    # Labels this file defines by hand. Collected up front because a definition
    # may sit below its use (markdown does not care, and the convention in this
    # tree is to park them at the end of the block).
    labels: set[str] = set()
    for raw in lines:
        m = DOC_LINE.match(raw)
        if m:
            d = REF_DEF.match(m.group(1))
            if d:
                labels.add(d.group(1))
    for n, raw in enumerate(lines, 1):
        m = DOC_LINE.match(raw)
        if not m:
            continue
        cov.doc_lines += 1
        for target in link_targets(m.group(1)):
            cov.targets += 1
            if target in labels:
                continue
            # Counted here rather than inside `dead_link` so the number means
            # "this target was a path and was resolved against the crate", not
            # "this target was looked at". A `split_path` that started
            # rejecting everything would leave `targets` untouched and this at
            # zero, which is the layer that would then be named.
            if split_path(target) is not None:
                cov.judged += 1
            if dead_link(target, types, scope):
                # `f` is already the repo-relative, `/`-separated spelling every
                # [`Tree`] method takes and returns, so there is nothing to
                # convert here. It used to be a `Path` put through `as_posix`,
                # because on this project's Windows hosts a native `Path`
                # renders with backslashes that a shell then eats as escapes,
                # and a finding is meant to be pasted into
                # `git log -S ... -- <path>` or an editor. The seam settles
                # that question once for every checker.
                yield (f, n, target, unit)


#: Floors for a WHOLE-TREE run. Not targets: each fires on a scan that
#: COLLAPSED and never on one that merely shrank. See `coverage_breach` for
#: the measurements they are derived from and why the crate floor is so much
#: slacker than the rest.
MIN_TREE_CRATES = 5
MIN_TREE_FILES = 200
MIN_TREE_DOC_LINES = 10000
MIN_TREE_TARGETS = 200
MIN_TREE_JUDGED = 200

#: What `scripts/mutate-gate.py` breaks to check that the floor above is
#: load-bearing rather than decorative. Each row is (label, exact source text,
#: replacement); the sweep applies one at a time and requires `--selftest` to
#: go red.
#:
#: The table is here rather than in the sweeper because every needle is a
#: quotation of this file. Kept elsewhere, a reworded line silently orphans its
#: needle, which then matches nothing, is skipped, and reads as coverage.
#:
#: It earns its keep. On 2026-09-03 nine of these seventeen survived: the
#: doc-line, target and judged floors were never consulted (the fixtures were
#: multiples of the constants, so zeroing a constant shrank its fixture to
#: match), all four coverage counters could stop counting unnoticed, and
#: neither direction of the whole-tree/subset routing was driven end to end.
#: The self-test was 74/74 green throughout.
SELFTEST_MUTANTS = [
    # The constants: is each floor read at all?
    ("MIN_TREE_CRATES gutted to 0", "MIN_TREE_CRATES = 5", "MIN_TREE_CRATES = 0"),
    ("MIN_TREE_FILES gutted to 0", "MIN_TREE_FILES = 200", "MIN_TREE_FILES = 0"),
    ("MIN_TREE_DOC_LINES gutted to 0", "MIN_TREE_DOC_LINES = 10000",
     "MIN_TREE_DOC_LINES = 0"),
    ("MIN_TREE_TARGETS gutted to 0", "MIN_TREE_TARGETS = 200",
     "MIN_TREE_TARGETS = 0"),
    ("MIN_TREE_JUDGED gutted to 0", "MIN_TREE_JUDGED = 200",
     "MIN_TREE_JUDGED = 0"),

    # The structural floors, which are the only ones a subset run gets.
    ("the no-crate check never fires", "if cov.crates < 1:", "if False:"),
    ("the crate-yields-a-unit check never fires",
     "if cov.units < cov.crates:", "if False:"),
    ("the unit-yields-a-file check never fires",
     "if cov.files < cov.units:", "if False:"),

    # The regime split: the absolute floors must apply to one side only.
    ("absolute floors are skipped for EVERY run", "if not whole_tree:",
     "if True:"),
    ("absolute floors are applied to SUBSET runs too", "if not whole_tree:",
     "if False:"),
    ("a subset run is mistaken for a whole-tree run",
     "\n        whole_tree = not paths\n", "\n        whole_tree = True\n"),
    ("a whole-tree run is mistaken for a subset run",
     "\n        whole_tree = not paths\n", "\n        whole_tree = False\n"),

    # The wiring in main(): is the breach acted on?
    ("main() computes the breach and ignores it", "        if breach:",
     "        if False:"),

    # The counters. A floor over a counter that does not count is the same
    # decoration as a constant nothing reads, and fails the same silent way.
    ("files are never counted", "cov.files += 1", "cov.files += 0"),
    ("doc lines are never counted", "cov.doc_lines += 1", "cov.doc_lines += 0"),
    ("link targets are never counted", "cov.targets += 1", "cov.targets += 0"),
    ("every target counts as judged, resolvable or not",
     "if split_path(target) is not None:", "if True:"),
]


def coverage_breach(cov: Coverage, whole_tree: bool) -> str | None:
    """Why this scan is too small to be worth a verdict, or None if it is fine.

    Two regimes, because the gate is invoked two ways and a single set of
    numbers cannot serve both. `boot-test.sh` and `pre-boot.py` scan the whole
    tree, where the corpus is known and an absolute floor is exactly right.
    The push hook scans only the crates a push touched, which is legitimately
    as little as one small crate with no doc comments in it at all -- flooring
    that absolutely would fail honest pushes, which is how a gate gets removed
    from a hook.

    So the subset run is floored STRUCTURALLY instead: every crate must yield
    at least one compilation unit and every unit at least one file. Those hold
    for any real crate regardless of size, and they still catch the failure
    that matters here -- `units` or the file walk returning nothing, which
    empties the scan while leaving the crate list intact.

    CALIBRATION (measured 2026-09-03). Two scans matter, not one:

        whole tree      2766 crates, 5218 files, 183958 doc lines,
                        5745 links, 5647 judged
        coreutils +     3 crates, 2431 files, 122404 doc lines,
        posix + init    2511 links, 2483 judged

    The floors are set an order of magnitude below the SECOND row, not the
    first, and the gap between the rows is the whole reason. 2757 of those
    2766 crates are the thin per-command CLI crates whose future is an open
    question for the operator; if the answer is "delete them" the crate count
    drops by a factor of five hundred in a single commit. A floor calibrated
    against 2766 would turn red on that commit and be read as the deletion
    having broken the gate. So the corpus this gate is allowed to assume is
    the part no plausible decision removes -- and it is also, conveniently,
    the part that carries nearly all the doc comments: those 2757 crates
    contribute ~1.2 links apiece.

    `MIN_TREE_CRATES` is therefore slack where the others are not. It cannot
    be derived from today's count at all; it is floored against what ROOTS
    irreducibly holds (posix, init, services, coreutils -- about ten crates),
    because the crate count is the one dimension that can legitimately fall
    off a cliff. The other four are floored ~12x below the surviving corpus
    and ~25x below today's, which is enough room for years of churn and
    still nowhere near the zero that a broken regex or an empty walk yields.
    """
    if cov.crates < 1:
        return "no crate was scanned"
    if cov.units < cov.crates:
        return (f"{cov.crates} crate(s) yielded only {cov.units} compilation "
                f"unit(s); every crate has at least a lib or a bin, so the "
                f"unit walk found nothing")
    if cov.files < cov.units:
        return (f"{cov.units} unit(s) yielded only {cov.files} readable "
                f"file(s); the file walk found nothing")
    if not whole_tree:
        # Everything below is an absolute count, and a one-crate push is
        # allowed to be tiny. The structural floors above already ran.
        return None
    for got, floor, what in (
        (cov.crates, MIN_TREE_CRATES, "crate(s)"),
        (cov.files, MIN_TREE_FILES, "source file(s)"),
        (cov.doc_lines, MIN_TREE_DOC_LINES, "doc comment line(s)"),
        (cov.targets, MIN_TREE_TARGETS, "intra-doc link(s)"),
        (cov.judged, MIN_TREE_JUDGED, "resolvable link path(s)"),
    ):
        if got < floor:
            return (f"a whole-tree scan saw only {got} {what}, below the floor "
                    f"of {floor}")
    return None


def crates_touching(all_crates: list[str], paths: list[str]) -> list[str]:
    """The crates that own `paths` -- the innermost Cargo.toml above each.

    Restricting a run to these is sound, not merely a shortcut. An intra-doc
    link is resolved inside one crate, and this checker only ever judges names
    it can see in that crate's own text; a rename in crate X therefore cannot
    turn a link in crate Y dead. (If Y `use`s the renamed item, Y stops
    compiling, which is a louder gate than this one.)

    A prefix comparison on `/`-joined strings replaces the old
    `Path.relative_to`, and needs the trailing separator to be sound:
    `userspace/coreutils-extra/x.rs` starts with `userspace/coreutils` as a
    *string* and is not in that crate. The old code got this right by asking
    `pathlib`; the new code has to say it, so it does, in one place.

    The crate list is passed in rather than derived here. `main` needs it
    anyway, to tell "this push touched no crate of ours" (a pass) from "this
    tree has no crates of ours" (no verdict), and `crate_roots` walks all four
    scanned roots -- doing it twice is a whole second enumeration of ~14k paths
    to reach the same answer.
    """
    out: list[str] = []
    for raw in paths:
        p = raw.replace("\\", "/").strip("/")
        best = None
        for c in all_crates:
            if p != c and not p.startswith(f"{c}/"):
                continue
            if best is None or len(c) > len(best):
                best = c
        if best is not None and best not in out:
            out.append(best)
    return out


def scan(tree: Tree, crates: list[str]) -> list[tuple[str, int, str, str]]:
    """Every dead intra-doc link in `crates`, read from `tree`.

    `crates` is required rather than defaulting to "all of them". The
    `None`-means-everything sentinel it used to carry made an empty list and a
    missing one look alike at the call site, and those are the two cases this
    gate most has to keep apart -- see `main`.
    """
    findings = []
    cov = Coverage()
    cov.crates = len(crates)
    for crate in crates:
        deps = dependencies(tree, crate)
        # The library's definitions are the same for all ~100 of `coreutils`'
        # binaries, and re-deriving them per binary makes the scan quadratic in
        # a package whose library is the big part. Compute once, union per unit.
        all_units = units(tree, crate)
        cov.units += len(all_units)
        lib_files = all_units[0][2] if all_units else []
        for _, _, shared in all_units:
            if shared:
                lib_files = shared
                break
        lib_defs = definitions(tree, lib_files)
        for unit, own, shared in all_units:
            d = definitions(tree, own)
            if shared:
                d = d.union(lib_defs)
            d.resolve()
            types = d.types
            scope = d.scope | deps
            # Only `own` is scanned for links. `shared` (the library) is here to
            # define names, not to be re-judged once per binary -- and a library
            # file's own docs are resolved in the library's context, which is the
            # unit where it appears as `own`.
            for f in own:
                findings.extend(scan_file(tree, f, unit, types, scope, cov))
    return sorted(findings, key=lambda x: (x[0], x[1], x[2])), cov


SELFTESTS = (
    # (link path, types defined by keyword here, other names in scope, should_flag)
    # The class this gate exists for: a rename left the prose behind.
    ("destination_is_older", {"destination_is_up_to_date"}, set(), True),
    ("destination_is_older", {"destination_is_older"}, set(), False),
    ("Self::add_to_archive", {"add"}, set(), True),
    ("Self::add", {"add"}, set(), False),
    ("Df::replace_problematic_chars", {"Df", "scrub"}, set(), True),
    ("Df::scrub", {"Df", "scrub"}, set(), False),
    ("crate::diag!", {"diag"}, set(), False),
    ("crate::diag!", {"other"}, set(), True),
    ("crate::guard_std_fds!", {"guard_std_fds"}, set(), False),
    # In scope without being defined here: a `use` is as good as a definition.
    ("escape_os", set(), {"escape_os"}, False),
    ("ere", set(), {"ere"}, False),
    # Not ours to judge: an external root, or a head we do not own.
    ("std::fs::set_permissions", set(), set(), False),
    ("coreutils::human::humblock", set(), set(), False),
    ("io::ErrorKind::InvalidInput", set(), set(), False),
    ("Stdio::null", set(), {"Stdio"}, False),
    ("PathBuf::join", set(), set(), False),
    ("Vec", set(), set(), False),
    ("Option", set(), set(), False),
    ("https://example.com", set(), set(), False),
    # ...but a head we DO own is judged, which is how a renamed method is caught.
    ("Shell::read_all_bytes", {"Shell"}, set(), True),
    # A lowercase head is a module path into a dependency, never a type we can
    # read: `authlib::Authenticator` lives in authlib's source, not ours.
    ("authlib::Authenticator", set(), {"authlib"}, False),
    ("deflate::inflate_limited", set(), {"deflate"}, False),
    # A head that is only re-exported is unjudgeable even though its name occurs
    # in our text: `Ch` is `pub use ere::ch::Ch` and `U` is a variant of *ere's*
    # enum, so no amount of reading oils can find it. Same for `Style`, which is
    # `quoting::Style` re-exported as `coreutils::quote::Style` -- and
    # `quote_with` is a real method of it.
    ("Ch::U", set(), {"Ch"}, False),
    ("Style::quote_with", set(), {"Style"}, False),
    # A type name that occurs only as some *other* enum's variant is likewise
    # not a type we defined, so it cannot be a judgeable head.
    ("Ch::B", set(), {"Ch", "B"}, False),
    # Generics and call spellings must not defeat the match.
    ("Creator<'a>", {"Creator"}, set(), False),
    ("run_text()", {"run_text"}, set(), False),
    # Prose that merely looks bracketed. "everything between `[` and `]`"
    # parses as a link to " and " unless a target must be shaped like a path.
    (" and ", set(), set(), False),
    ("Self::add ", {"add"}, set(), False),
)


def selftest() -> int:
    bad = 0
    checks = 0

    def check(ok: bool, msg: str) -> None:
        nonlocal bad, checks
        checks += 1
        if not ok:
            print(f"selftest FAIL: {msg}", file=sys.stderr)
            bad += 1

    for path, types, extra, want in SELFTESTS:
        got = dead_link(path, types, types | extra)
        check(
            got == want,
            f"{path!r} types={sorted(types)} scope+={sorted(extra)} "
            f"-> flagged={got}, expected {want}",
        )
    # The link extractor must find both spellings and neither more nor less.
    got = sorted(link_targets("/// see [`a::b`] and [text](c::d) and `not_a_link`"))
    check(got == ["a::b", "c::d"], f"extractor got {got}")
    # An explicit link's label is display text. Yield the target once, not the
    # label as well -- `[`X`](m::X)` is one link, and it is to `m::X`.
    got = sorted(link_targets("/// [`ERR_BROKEN_PIPE`](netipc::ring::ERR_BROKEN_PIPE)"))
    check(
        got == ["netipc::ring::ERR_BROKEN_PIPE"],
        f"explicit-link label taken as a target: {got}",
    )
    # ...and the same for the reference form `[label][ref]`.
    check(
        not list(link_targets("/// [`OP_SEND`][op] here")),
        "reference-link label taken as a target",
    )
    # Prose in brackets is rustdoc's to complain about, not ours.
    check(
        not list(link_targets("/// set[ug]id bits")),
        "bare-bracket prose was treated as a link",
    )

    # The definition scan. Tested directly because `dead_link` cannot see a
    # mistake here: a name the scan misses simply becomes a finding, and the
    # `const fn` bug below invented ten of them while every case above passed.
    src = """
        pub struct Outcome {
            pub reason: u8,
        }
        pub enum Nested {
            Sub,
            Other(u8),
        }
        pub type Alias = u8;
        impl Outcome {
            pub const fn user_message(self) -> &'static str { "" }
            pub const LIMIT: usize = 8;
            const fn reserved_ok(&self) -> bool { true }
            pub async unsafe fn slurp(&self) {}
        }
        static mut COUNTER: u64 = 0;
        macro_rules! diag { () => {} }
        mod inner {}
        use quoting::escape_os;
    """
    d = Defs()
    defs_in_text(src, d)
    d.resolve()
    want_types = {"Outcome", "Nested", "Alias"}
    check(
        d.types == want_types,
        f"types={sorted(d.types)}, expected {sorted(want_types)}",
    )
    for name in (
        "user_message", "reserved_ok", "slurp", "LIMIT", "COUNTER",
        "diag", "inner", "escape_os", "Sub", "reason", "Outcome",
    ):
        check(name in d.scope, f"{name!r} missing from scope")
    # ...and the keyword itself is never a definition.
    for name in ("fn", "mut", "pub", "const"):
        check(
            name not in d.scope and name not in d.types,
            f"keyword {name!r} captured as a name",
        )

    # An item whose name exists only as an argument to the crate's own
    # `macro_rules!`. `oils` generates `nested_parts`/`nested_parts_mut` this
    # way and links to both; no keyword scan can see either name.
    gen = """
        macro_rules! nested_parts_fn {
            ($name:ident, $slice:ident,) => { pub(crate) fn $name() {} };
        }
        nested_parts_fn!(nested_parts, as_slice,);
        nested_parts_fn!(
            nested_parts_mut,
            as_mut_slice,
        );
        fn caller() {
            assert_eq!(status, 0);
        }
    """
    d = Defs()
    defs_in_text(gen, d)
    d.resolve()
    for name in ("nested_parts", "nested_parts_mut"):
        check(name in d.scope, f"macro-generated {name!r} missing from scope")
    # ...but an invocation of a macro this crate did NOT declare contributes
    # nothing, or every `assert_eq!(x, ...)` would make its local `x` an item.
    check("status" not in d.scope, "an std macro's argument was taken as a name")

    # THE EXIT-CODE CONTRACT, which is not a detail of link-finding and is the
    # one thing above that nothing above tests.
    #
    # Every case up to here asks "does the checker *find* the dead link?" On
    # 2026-09-02 the answer was yes and it did not matter: a bare run -- which
    # is how `pre-boot.py`'s `check-*.py` glob invokes every gate -- printed the
    # findings and then returned 0 for a help screen. The detection was perfect
    # and the gate could not fail. A suite that only tests the finder cannot see
    # that, so these drive `main()` itself and assert on the status.
    #
    # `scan` is stubbed rather than given a fixture tree: the contract under
    # test is "findings => non-zero", and a real scan would spend ~400s
    # re-testing the finder instead.
    real_scan = globals()["scan"]
    real_argv = sys.argv
    finding = [("f.rs", 1, "a::b", "crate")]

    def plausible() -> Coverage:
        """Coverage comfortably above every whole-tree floor."""
        c = Coverage()
        c.crates, c.units, c.files = 10 * MIN_TREE_CRATES, 200, 10 * MIN_TREE_FILES
        c.doc_lines = 10 * MIN_TREE_DOC_LINES
        c.targets, c.judged = 10 * MIN_TREE_TARGETS, 10 * MIN_TREE_JUDGED
        return c

    try:
        for argv, findings_, cov_, want, why in (
            (["x"], finding, plausible(), 1, "a bare run with a dead link must FAIL"),
            (["x"], [], plausible(), 0, "a bare run with a clean tree must pass"),
            (["x", "--check"], finding, plausible(), 1, "--check is still accepted"),
            (["x", "--check"], [], plausible(), 0, "--check on a clean tree still passes"),
            (["x", "--list"], finding, plausible(), 0, "--list reports without failing"),
            # THE FLOOR, driven through main() rather than asserted on its
            # constants. `coverage_breach` is unit-tested below, but that
            # proves only that it can say no -- not that main() ever asks it.
            # A main() with the floor block deleted passes every case above
            # with the constants perfectly intact, which is the shape of hole
            # that a verified-but-unconsulted table always leaves.
            #
            # Note the direction: the tree is CLEAN in this case. An empty
            # scan with no findings is precisely the state that used to print
            # "ok", and it must now be exit 2 instead.
            (["x"], [], Coverage(), 2, "a clean tree scanned empty must NOT pass"),
        ):
            globals()["scan"] = lambda repo, only, _f=findings_, _c=cov_: (list(_f), _c)
            sys.argv = argv
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
                got = main()
            check(got == want, f"{why}: argv={argv[1:]} -> exit {got}, want {want}")
    finally:
        globals()["scan"] = real_scan
        sys.argv = real_argv

    # `coverage_breach` itself, layer by layer. Each case keeps every other
    # count plausible so the message names the layer that actually broke --
    # a floor that reports the wrong layer sends the reader to the wrong file.
    def cov_with(**kw) -> Coverage:
        c = plausible()
        for k, v in kw.items():
            setattr(c, k, v)
        return c

    check(coverage_breach(plausible(), True) is None,
          "a plausible whole-tree scan is not a breach")
    check(coverage_breach(plausible(), False) is None,
          "...nor is it one as a subset run")
    check(coverage_breach(Coverage(), True) is not None,
          "an empty whole-tree scan is a breach")
    check(coverage_breach(Coverage(), False) is not None,
          "an empty subset run is a breach too -- it has no crates")

    # The structural floors bite in BOTH regimes, which is the whole reason a
    # subset run is safe to exempt from the absolute ones.
    for mode in (True, False):
        where = "whole-tree" if mode else "subset"
        b = coverage_breach(cov_with(units=1), mode)
        check(b is not None and "compilation unit" in b,
              f"{where}: crates without units is a breach naming that layer")
        b = coverage_breach(cov_with(files=0), mode)
        check(b is not None and "readable file" in b,
              f"{where}: units without files is a breach naming that layer")

    # ...and the absolute ones bite ONLY in whole-tree mode. Both halves are
    # asserted: a subset run that failed these would break every honest
    # single-crate push, and a whole-tree run that passed them would be the
    # collapsed scan this floor exists to catch.
    for field, floor, layer in (
        ("files", MIN_TREE_FILES, "source file"),
        ("doc_lines", MIN_TREE_DOC_LINES, "doc comment line"),
        ("targets", MIN_TREE_TARGETS, "intra-doc link"),
        ("judged", MIN_TREE_JUDGED, "resolvable link path"),
    ):
        # One below the floor, with the structural counts kept consistent so
        # only the absolute floor can object.
        c = cov_with(**{field: floor - 1})
        c.units = min(c.units, c.files)
        c.crates = min(c.crates, c.units)
        b = coverage_breach(c, True)
        check(b is not None and layer in b,
              f"whole-tree: {field} below its floor is a breach naming it")
        check(coverage_breach(c, False) is None,
              f"subset: {field} below the whole-tree floor is NOT a breach")

    # ...and now the same floors asserted against ABSOLUTE numbers rather than
    # against the constants.
    #
    # Everything above scales with MIN_TREE_*: `plausible()` is ten times the
    # constants and the breach fixtures are one below them, so setting a floor
    # to zero shrinks the fixtures to match and every case above still passes.
    # Mutation testing found exactly that -- gutting MIN_TREE_DOC_LINES,
    # MIN_TREE_TARGETS and MIN_TREE_JUDGED to 0 left the suite green. A test
    # whose fixtures are derived from the value under test cannot see that
    # value change; it can only see the code stop *using* it.
    #
    # So this case hardcodes a scan that is plainly not a whole tree -- five
    # crates holding fifty doc lines between them, ten links, five of them
    # resolvable -- and demands a refusal. It is a statement about the corpus,
    # not about the constants: a floor low enough to call this a credible
    # whole-tree scan of `posix/` and `userspace/` is a floor set wrong,
    # whatever number it holds.
    collapsed = Coverage()
    (collapsed.crates, collapsed.units, collapsed.files) = 5, 5, 10
    (collapsed.doc_lines, collapsed.targets, collapsed.judged) = 50, 10, 5
    check(coverage_breach(collapsed, True) is not None,
          "a whole-tree scan of 10 files/50 doc lines is refused outright")
    # Each floor is then shown to be the one doing that work, by handing it a
    # scan that is generous in every other dimension. Gut any single constant
    # and its own case here goes green while the others stay red, so the
    # survivor names which floor stopped being consulted.
    for field, small, layer in (
        ("crates", 2, "crate"),
        ("files", 20, "source file"),
        ("doc_lines", 300, "doc comment line"),
        ("targets", 30, "intra-doc link"),
        ("judged", 20, "resolvable link path"),
    ):
        c = Coverage()
        # Generous everywhere else, and above every floor this file sets.
        (c.crates, c.units, c.files) = 5000, 5000, 20000
        (c.doc_lines, c.targets, c.judged) = 500000, 20000, 20000
        setattr(c, field, small)
        # Keep the structural chain consistent so only the absolute floor can
        # object -- otherwise a shrunken `crates` would be caught by the
        # units-per-crate rule instead and the case would pass for the wrong
        # reason.
        c.units = min(c.units, c.files)
        c.crates = min(c.crates, c.units)
        b = coverage_breach(c, True)
        check(b is not None and layer in b,
              f"whole-tree: {small} {layer}(s) is below any defensible floor")

    # THE COVERAGE ITSELF, over a real tree on disk.
    #
    # Everything above tests `coverage_breach` as a pure function, handed a
    # `Coverage` written down by the test. That leaves the floor resting on an
    # object nothing proves is real: mutation testing found that `cov.files`,
    # `cov.doc_lines`, `cov.targets` and `cov.judged` could each stop counting
    # altogether -- `+= 0` in place of `+= 1` -- and all 74 cases stayed green,
    # because no case ever asked `scan` for a number. A floor over a counter
    # that does not count is the same decoration as a constant nothing reads,
    # and it fails the same way: silently, reporting "0 dead links" forever.
    #
    # `scan` is stubbed above for the exit-code cases and that is still right
    # there -- those test the status contract and a real scan would spend ~400s
    # re-testing the finder. Here the tree is three files, so the real thing is
    # affordable and the counts can be pinned exactly.
    with tempfile.TemporaryDirectory() as td:
        def write(rel: str, text: str) -> None:
            p = os.path.join(td, rel.replace("/", os.sep))
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "w", encoding="utf-8") as fh:
                fh.write(text)

        # Six doc lines, three link targets, two of them resolvable. The URL is
        # the point of the third: `split_path` refuses it, so `targets` and
        # `judged` must disagree. Without a target that is looked at but not
        # judged, `judged` could simply be a second name for `targets` -- which
        # is exactly the mutant `if True:` in place of the `split_path` test,
        # and it survived until this case existed.
        write("userspace/one/Cargo.toml", '[package]\nname = "one"\n')
        write("userspace/one/src/lib.rs",
              "//! A crate doc.\n"
              "//!\n"
              "//! Links: [`Thing`] and [`https://example.com/x`].\n"
              "\n"
              "/// A struct.\n"
              "///\n"
              "/// See [`Thing`].\n"
              "pub struct Thing;\n")

        real_root = globals()["ROOT"]
        try:
            globals()["ROOT"] = td
            with gittree.open_tree(td, None) as t:
                _findings, cov = scan(t, crate_roots(t))
            for got, want, what in (
                (cov.crates, 1, "crates"),
                (cov.units, 1, "units"),
                (cov.files, 1, "files"),
                (cov.doc_lines, 6, "doc_lines"),
                (cov.targets, 3, "targets"),
                (cov.judged, 2, "judged"),
            ):
                check(got == want,
                      f"scan counts {what}: got {got}, want {want}")

            # ...and the two regimes, driven through `main()` against that same
            # tree rather than asserted on `coverage_breach`'s flag argument.
            # `whole_tree = not paths` is a one-line derivation that no case
            # touched: inverting it left the suite green both ways round. It is
            # the routing decision the whole two-regime design rests on, so it
            # is worth an end-to-end case in each direction.
            #
            # The tree is deliberately tiny. That makes the two regimes give
            # OPPOSITE answers on identical input -- a subset run passes it and
            # a whole-tree run refuses it -- so neither direction of the
            # inversion can pass both.
            # The path is tree-RELATIVE, and that is not a detail. Passed as an
            # absolute temp path it matches no crate prefix, so `main` takes the
            # "no scanned crate was touched" branch and returns 0 having scanned
            # nothing -- the case then passes for a reason that has nothing to
            # do with what it claims to test. It did, until the mutant that
            # forces `whole_tree` true failed to kill it and pointed here.
            #
            # Hence the second half of each assertion: the exit code alone is
            # what let that through, so each case also names the sentence it
            # must have printed. Three exits of 0 are spelled identically and
            # mean entirely different things.
            for argv, want, must_say, why in (
                (["x", "userspace/one/src/lib.rs"], 0, "no dead intra-doc links",
                 "a subset run of one small clean crate scans it and passes"),
                (["x"], 2, "below the floor",
                 "...and the same crate as a WHOLE tree is refused as too small"),
            ):
                sys.argv = argv
                buf = io.StringIO()
                with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
                    got = main()
                text = buf.getvalue()
                check(got == want and must_say in text,
                      f"{why}: exit {got} (want {want}), said {text.strip()!r}")
        finally:
            globals()["ROOT"] = real_root
            sys.argv = real_argv

    if bad:
        print(f"selftest: {bad} of {checks} cases FAILED", file=sys.stderr)
        return 1
    print(f"selftest: {checks}/{checks} cases pass")
    return 0


def read_path_list(source: str) -> list[str]:
    """The path list in `source`, one per line; `-` means stdin.

    WHY THIS EXISTS. The push hook scopes this checker to the crates the push
    touches and passed them as arguments, which is the obvious way and has a
    ceiling. On 2026-09-02 a push carrying 2,568 changed `.rs` files produced
    64,862 bytes of directory names -- almost exactly twice Windows'
    32,767-character command-line limit -- and the gate died with "Argument
    list too long" before reading a single file. That is exit 126, which the
    hook correctly refuses to treat as a pass, so a lane's pushes were blocked
    by a limit that has nothing to do with the code being pushed.

    Batching the arguments was the tempting fix and is the wrong one: the
    verdict would still be right, because crates are scanned independently and
    the findings are a union, but every batch repeats the `rglob("Cargo.toml")`
    crate discovery over four whole roots. Forty batches of that is minutes of
    work to avoid a limit a file does not have.

    `split("\\n")` and not `splitlines()`: the latter also breaks on `\\v`,
    `\\f`, `\\x1c` and `U+2028`, all of which are legal in a path here (this
    filesystem forbids only `/` and NUL), so it would tear one real name into
    two that match nothing. A trailing `\\r` is dropped because a Windows
    producer would have written one -- the same tolerance, for the same
    reason, as `scripts/gittree.py`.
    """
    if source == "-":
        text = sys.stdin.read()
    else:
        with open(source, "r", encoding="utf-8", errors="surrogateescape") as fh:
            text = fh.read()
    lines = [ln[:-1] if ln.endswith("\r") else ln for ln in text.split("\n")]
    return [ln for ln in lines if ln.strip()]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true",
                    help="accepted and ignored: failing on a dead link is the "
                         "default, and was not always (see main)")
    ap.add_argument("--selftest", action="store_true", help="verify the checker itself")
    ap.add_argument("--list", action="store_true", help="print findings and exit 0")
    ap.add_argument(
        "paths",
        nargs="*",
        help="limit the scan to the crates owning these files (a whole-tree run "
        "reads 61 MB of Rust and takes about half a minute; a push usually "
        "touches one crate)",
    )
    ap.add_argument(
        "--paths-from",
        metavar="FILE",
        help="read the path list from FILE, one per line ('-' for stdin), "
        "instead of (or as well as) passing them as arguments",
    )
    ap.add_argument(
        "--head", default=None,
        help="judge this commit instead of the working tree. The push hook "
             "passes the commit being published, so a dead link introduced by "
             "a commit cannot be hidden by a tidied worktree -- nor an "
             "uncommitted one block a push of unrelated clean commits.",
    )
    args = ap.parse_args()

    if args.selftest:
        return selftest()

    paths = list(args.paths)
    if args.paths_from is not None:
        try:
            paths.extend(read_path_list(args.paths_from))
        except OSError as exc:
            print(f"check-doc-links: cannot read {args.paths_from}: {exc}",
                  file=sys.stderr)
            return 2
        if not paths:
            # An explicitly named source of paths that named none is a caller
            # bug, and the one thing it must not do is look like a pass. It
            # cannot be read as "scan everything" either -- that is what an
            # absent list means, and silently widening the scope is how a gate
            # comes to fail on someone else's file. So: refuse, loudly.
            print(f"check-doc-links: {args.paths_from} listed no paths",
                  file=sys.stderr)
            return 2

    try:
        tree = gittree.open_tree(ROOT, args.head)
    except gittree.GitTreeError as exc:
        # Exit 2, not 1. `scripts/run-checker.sh` reads 1 as "the checker found
        # something" and prints the gate's refusal over it; a revision that
        # cannot be opened is not a finding against anyone's code.
        print(f"check-doc-links: cannot read {args.head!r}: {exc}", file=sys.stderr)
        return 2
    with tree:
        all_crates = crate_roots(tree)
        if not all_crates:
            # The corpus half of gate 6's `_inputs_missing` rule. Nothing under
            # any of `ROOTS` holds a Cargo.toml, so there is no crate to resolve
            # a link *inside* and the scan below would find nothing however
            # broken the tree is -- a clean report by accident, which is the one
            # outcome a gate must never produce. Exit 2 (no verdict), not 0.
            #
            # It is a per-tree question, which is why `--head` makes it worth
            # asking: a commit that renames `userspace/` away disarms the gate
            # while the author's working tree, mid-rename, still answers that
            # all is well.
            #
            # There is no baseline half. This checker has nothing to ratchet --
            # it resolves names against the same tree it read them from -- so
            # the corpus is its only input, and the rule is complete here.
            print(f"check-doc-links: no crate found under {'/, '.join(ROOTS)}/ "
                  f"-- nothing to judge.", file=sys.stderr)
            return 2

        if paths:
            crates = crates_touching(all_crates, paths)
            if not crates:
                # NOT the guard above, and the distinction is the whole reason
                # the two are spelled separately: the tree has crates, this push
                # just did not touch any of them. A push that only edits
                # `kernel/` is lane A's, is entirely legitimate, and must pass.
                # Folding this into "no crates" would refuse every lane-A and
                # lane-C push.
                print("ok -- no scanned crate was touched.")
                return 0
        else:
            crates = all_crates
        findings, cov = scan(tree, crates)
        whole_tree = not paths
        breach = coverage_breach(cov, whole_tree)
        if breach:
            # Exit 2, not 1: nothing was established about anyone's links, so
            # this is "could not look", not "looked and found something". The
            # distinction is the one `run-checker.sh` reads, and getting it
            # wrong here would print a refusal naming a crate that is fine.
            print(f"check-doc-links: refusing to report a verdict -- {breach}",
                  file=sys.stderr)
            print(f"       inspected: {cov}", file=sys.stderr)
            print("       A clean report and an empty scan are the same "
                  "sentence, so the scan has to say how much it saw.",
                  file=sys.stderr)
            return 2

    for f, n, target, crate in findings:
        print(f"{f}:{n}: [`{target}`] names nothing in crate `{crate}`")

    if args.list:
        print(f"\n{len(findings)} dead intra-doc link(s).")
        return 0

    # A bare run is a --check, NOT a help screen.
    #
    # It used to be `ap.print_help(); return 0`, and that made this the one
    # gate in scripts/ that could not fail. `pre-boot.py` runs the `check-*.py`
    # glob bare -- that is the convention here, and the other twenty scripts
    # honour it -- so a bare run of this one scanned the whole tree (412s
    # measured 2026-09-02), printed any findings it made, and then returned 0
    # for a help screen. `_report` discarded the output of a passing gate, so
    # the findings were not merely unenforced, they were unseen: seven minutes
    # of every pre-boot run spent proving nothing.
    #
    # The hook (gate 11) passes --check explicitly and was never affected,
    # which is why this survived; --check is kept as an accepted no-op so that
    # call site, and any other, keeps working unchanged.
    #
    # Argparse convention would show help for a bare invocation. That
    # convention loses to this directory's: a script named check-*.py is run
    # bare by a glob whose entire purpose is to collect verdicts, and a gate
    # that answers "here is my usage" to that question is a gate that passes.
    # `--help` still prints the help.
    if findings:
        print(f"\n{len(findings)} dead intra-doc link(s).", file=sys.stderr)
        return 1
    # The pass names its corpus. A reader who sees only "ok" cannot tell a
    # thorough scan from an empty one, and that is the whole failure this
    # gate's floor exists to prevent -- so the number that makes the verdict
    # meaningful travels with it instead of being discarded.
    print(f"ok -- no dead intra-doc links ({cov}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
