#!/usr/bin/env python3
"""Read many blobs out of *one* git process instead of one process per blob.

# The defect this closes

The push hook is supposed to answer "is the code I am about to publish OK?"
Most of its gates answer "is the code on my disk right now OK?" instead —
they name their files from the pushed commit range and then read the contents
off the working tree. Those are the same question until you commit something
and keep editing, at which point the hook can wave a bad commit through, or
block a good one, and say nothing about either. That is not hypothetical:
commits `861f4d80e` and `09a436956` reached `origin/lane-b` unformatted
because gate 7 had exactly this shape.

`known-issues.md` →
`TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH` sets out
the full repair. Its first step is this module, and it is first because it is
the one that makes the rest *affordable*. Reading a blob with a fresh
`git cat-file blob <rev>:<path>` costs ~0.34 s on this machine — almost all of
it process startup, not I/O — so a push carrying 2,568 `.rs` files pays about
fifteen minutes before any checker has looked at anything. `git cat-file
--batch` answers an unbounded number of those requests over one pipe, which
turns the same 2,568 reads into roughly one process and a few seconds.

# Why not just materialise the whole tree

Measured on this machine before gate 7 was written, because "copy the tree and
point the checkers at it" is the obvious idea and it does not work here:

| Approach                                            | Measured                |
|-----------------------------------------------------|-------------------------|
| `git archive HEAD` (whole tree)                      | 86 s, 204 MB            |
| `git archive HEAD -- userspace/zip/src` (80 KB out)  | 23 s — archive walks the whole tree regardless of pathspec |
| `git archive HEAD -- posix/src`                      | 11 s                    |
| `cp -al posix/src <tmp>`                             | 53 s, and fails outright on this filesystem |
| `git cat-file blob` per file                         | ~0.34-0.7 s each        |
| this module                                          | one process, total      |

# Two ways in, on purpose

As a **library**, for the Python checkers: construct a [`GitTree`] and call
`read` / `list_paths`. The intended shape is a `--head <sha>` flag that
selects git, with its absence keeping the checker's existing filesystem walk —
the checkers are also run by hand and by the boot test, where the working tree
is the right thing to read. `scripts/check-request-deletion.py` already has
that flag and is the precedent.

As a **CLI**, for the shell hook, which cannot import Python: `materialise`
lays the pushed bytes of a list of paths out under one root, at their real
relative paths, and prints where each landed. That is what gate 7 needs, and
gate 7 is the reason the `--stub-rust-mods` option exists (see below).

Nothing here writes to the repository or runs a git command that can mutate
it: `cat-file` and `ls-tree` are reads.
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from typing import Iterable, Iterator, Optional

# `mod name;` and `pub mod name;`, and nothing cleverer.
#
# Deliberately not a general model of rustc's module resolution. The
# alternative is approximating `#[path = "..."]`, `cfg`-gated arms and `mod`
# nested inside an inline `mod`, and an approximation that is *wrong* seeds a
# stub that shadows a real sibling and silently drops it from whatever check
# the caller was running. A form this does not recognise instead leaves rustfmt
# unable to resolve it, which is loud: the tool fails and the caller reports it.
# Failing visibly on an unhandled shape beats guessing at it.
_MOD_DECL = re.compile(
    rb"^[ \t]*(?:pub[^ ]*[ \t]+)?mod[ \t]+([A-Za-z_][A-Za-z0-9_]*)[ \t]*;",
    re.MULTILINE,
)

# rustfmt reports a diff on a zero-byte file — it wants the trailing newline —
# so a stub that is genuinely empty would make every stub this creates look
# like a finding. One byte is the smallest thing that is *clean*.
_STUB = b"\n"


class GitTreeError(RuntimeError):
    """git could not be started, or died mid-conversation."""


class GitTree:
    """A long-lived `git cat-file --batch`, plus the `ls-tree` calls that pair
    with it.

    Use it as a context manager; the git process is closed on exit. Reads are
    strictly ordered — the protocol is a single request/response pipe — so an
    instance is **not** safe to share between threads.
    """

    def __init__(self, repo: Optional[str] = None) -> None:
        self._repo = repo
        try:
            # `-c core.quotePath=false` for the same reason the hook passes it:
            # git otherwise octal-escapes a non-ASCII path into something that
            # does not name a file, and this repository has utilities whose
            # whole purpose is handling those names.
            self._proc = subprocess.Popen(
                ["git", "-c", "core.quotePath=false", "cat-file", "--batch"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                cwd=repo,
            )
        except OSError as exc:
            raise GitTreeError(f"cannot start git cat-file: {exc}") from exc

    # -- context manager ---------------------------------------------------

    def __enter__(self) -> "GitTree":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        """Shut the batch process down. Idempotent."""
        proc = getattr(self, "_proc", None)
        if proc is None:
            return
        self._proc = None
        try:
            if proc.stdin is not None:
                proc.stdin.close()
        except OSError:
            # The pipe is already gone, which is the state we were asking for.
            pass
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

    # -- reading -----------------------------------------------------------

    def read(self, rev: str, path: str) -> Optional[bytes]:
        """The bytes of `path` at `rev`, or `None` if it is not there.

        `None` is a real answer, not an error: a file added by one commit in a
        push and deleted by a later one is still enumerated by
        `git log --name-only`, and asking for it is normal.
        """
        if self._proc is None:
            raise GitTreeError("read after close")
        spec = f"{rev}:{path}".encode("utf-8", "surrogateescape")
        stdin = self._proc.stdin
        stdout = self._proc.stdout
        if stdin is None or stdout is None:
            raise GitTreeError("git cat-file has no pipes")
        try:
            stdin.write(spec + b"\n")
            stdin.flush()
        except OSError as exc:
            raise GitTreeError(f"git cat-file died: {exc}") from exc
        header = stdout.readline()
        if not header:
            raise GitTreeError("git cat-file closed its output")
        fields = header.rstrip(b"\n").split(b" ")
        # "<oid> <type> <size>" on success; "<what-was-asked> missing" (and,
        # for an ambiguous name, "... ambiguous") otherwise. Anything that is
        # not the three-field success line means "no bytes", and the caller
        # cannot tell the difference from `None` because there is nothing it
        # would do differently.
        if len(fields) != 3:
            return None
        try:
            size = int(fields[2])
        except ValueError:
            return None
        data = stdout.read(size)
        if data is None or len(data) != size:
            raise GitTreeError("git cat-file truncated a blob")
        stdout.read(1)  # the newline git appends after the payload
        return data

    def read_many(
        self, rev: str, paths: Iterable[str]
    ) -> Iterator[tuple[str, Optional[bytes]]]:
        """`read` over an iterable, yielding `(path, bytes-or-None)` in order."""
        for path in paths:
            yield path, self.read(rev, path)

    def list_paths(self, rev: str, *pathspec: str) -> list[str]:
        """Every file path under `rev` matching `pathspec` (all files if none).

        A separate `git ls-tree -r` call rather than part of the batch: the
        batch protocol answers "give me this object", not "enumerate". 1.5-4 s
        per directory, so callers should ask once for a wide pathspec rather
        than per-directory.
        """
        cmd = [
            "git",
            "-c",
            "core.quotePath=false",
            "ls-tree",
            "-r",
            "-z",
            "--name-only",
            rev,
        ]
        if pathspec:
            cmd.append("--")
            cmd.extend(pathspec)
        try:
            out = subprocess.run(
                cmd, cwd=self._repo, stdout=subprocess.PIPE, check=True
            ).stdout
        except (OSError, subprocess.CalledProcessError) as exc:
            raise GitTreeError(f"git ls-tree failed: {exc}") from exc
        # `-z` rather than newline separation: a path in this repository may
        # contain any byte except `/` and NUL, newline included, and utilities
        # that handle exactly those names live in this tree.
        return [
            p.decode("utf-8", "surrogateescape")
            for p in out.split(b"\0")
            if p
        ]


# --------------------------------------------------------------------------
# Tree: the seam the checkers read through
# --------------------------------------------------------------------------
#
# `GitTree` above is a transport. This is the thing a checker is written
# against: "give me the files under `posix/src`" and "give me the text of
# this one", answered either by the working tree or by a revision, with the
# checker unable to tell which.
#
# It exists because seven push gates enumerate their files from the pushed
# commit range and then read the contents off the disk -- see
# `known-issues.md` ->
# TD-B-PRE-PUSH-GATES-2-6-8-11-JUDGE-THE-WORKING-TREE-NOT-THE-PUSH. Each of
# those checkers is also run by hand and by the boot test, where the working
# tree is exactly the right thing to read, so the fix cannot be "read git
# instead". It has to be "read whichever the caller asked for", and that is a
# seam, not a flag.
#
# The API is deliberately four methods and no filtering. `rglob("*.rs")` is
# not offered, because the checkers do not all mean the same thing by it --
# one excludes `target/`, one excludes `src/bin`, one wants `Cargo.toml` by
# name -- and a shared glob would have to grow an option for each. They get
# a list of paths and apply their own predicate, which keeps every one of
# those judgements visible in the checker that makes it.
#
# Paths in and out are always **repo-relative with forward slashes**, because
# that is what git speaks, what the checkers already print in their findings,
# and what their baseline files already record. A `Path` would be the natural
# Python type and is the wrong one here: it would make the working-tree
# implementation trivially correct and the git one a translation layer, and
# on Windows it would spell half the separators the other way round.


# Directory names that are never any checker's subject matter, skipped by
# every listing on both sides of the seam.
#
# `target` is Rust build output: tens of gigabytes, and every checker being
# converted already drops it by hand.
#
# `.git` is here because without it the seam breaks its own promise at the
# most obvious call site. `files_under("")` on the disk walks the repository
# root, and the repository root contains `.git` -- thousands of loose objects
# and packs that no revision can ever list, because git will not track its own
# directory. The two sides would then disagree by the entire object store on
# the one query a checker is likeliest to make.
_PRUNE = ("target", ".git")


def _pruned(rel: str) -> bool:
    """True for a path inside a directory no listing should descend into.

    A component-wise test, not `"target" in rel`: `posix/src/target_arch.rs`
    is source, and a substring test would hide it from every listing. The
    same rule spelled the same way for both implementations, because the one
    thing a seam must not do is answer differently on its two sides.
    """
    parts = rel.split("/")
    return any(name in parts for name in _PRUNE)


class Tree:
    """Read-only access to a set of files, by repo-relative path.

    Two implementations: [`WorkTree`], which is the disk, and [`RevTree`],
    which is a git revision. A checker written against this interface asks
    the same questions of either.

    Every method takes and returns repo-relative paths with `/` separators,
    and no method raises for a path that is simply not there -- `read_text`
    answers `None`, the predicates answer `False`, and the listings answer
    an empty list. A checker's job is to judge the files that exist, and a
    missing file is an answer rather than an error at every one of these
    call sites.

    **Where the two implementations legitimately disagree.** The disk holds
    build output; a commit does not, because `target/` is gitignored. That
    is not a defect in the seam, it is the difference the seam exists to
    expose, and it cannot be papered over -- `WorkTree.is_file` telling you
    a `target/` file is absent when it is right there on disk would be a
    lie, not a normalisation. What *is* normalised is enumeration: both
    `files_under` and `entries` skip `target/` and `.git` on both sides (see
    `_PRUNE`), so a checker that walks a subtree sees the same set from
    either unless the difference is real source. The
    predicates and the readers answer for the exact path they were handed,
    truthfully, on whichever tree they are.
    """

    def files_under(self, prefix: str) -> list[str]:
        """Every file at or below `prefix`, sorted, `/`-separated.

        `target/` and `.git` are skipped (see `_PRUNE`). A `prefix` that is
        itself inside one yields nothing, on both implementations, rather
        than the disk saying yes and git saying no.
        """
        raise NotImplementedError

    def entries(self, prefix: str) -> list[tuple[str, bool]]:
        """The immediate children of `prefix` as `(path, is_dir)`, sorted.

        Skips build directories for the same reason and by the same rule as
        `files_under`; see the class docstring for why the predicates below
        deliberately do not.
        """
        raise NotImplementedError

    def is_file(self, rel: str) -> bool:
        raise NotImplementedError

    def is_dir(self, rel: str) -> bool:
        raise NotImplementedError

    def read_bytes(self, rel: str) -> Optional[bytes]:
        raise NotImplementedError

    def read_text(self, rel: str, errors: str = "replace") -> Optional[str]:
        """The file decoded as UTF-8, or `None` if it is not there.

        `errors="replace"` by default because that is what every checker
        being converted already passes: they scan source for patterns, and a
        file with a bad byte in it should be scanned, not skipped. Callers
        that need the bytes intact use `read_bytes`.
        """
        data = self.read_bytes(rel)
        if data is None:
            return None
        return data.decode("utf-8", errors)

    # -- context manager, so callers can use either one the same way --------

    def __enter__(self) -> "Tree":
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        """Release anything held. Idempotent; the disk holds nothing."""


def _norm(rel: str) -> str:
    """A repo-relative path in the one spelling everything here uses."""
    return rel.replace("\\", "/").strip("/")


class WorkTree(Tree):
    """The files on disk, under `root`.

    This is what every checker does today, moved behind the interface
    unchanged, so that a checker run by hand or by the boot test keeps
    reading exactly what it read before the conversion.
    """

    def __init__(self, root: str) -> None:
        self.root = root.replace("\\", "/").rstrip("/")

    def _abs(self, rel: str) -> str:
        rel = _norm(rel)
        return f"{self.root}/{rel}" if rel else self.root

    def files_under(self, prefix: str) -> list[str]:
        base = self._abs(prefix)
        prefix = _norm(prefix)
        # Looks redundant and is not. The per-file prune below already drops
        # everything a walk of a build directory could find, so deleting this
        # changes no *answer* -- it changes the *work*, from nothing to a full
        # descent of a directory that is tens of gigabytes on this tree, inside
        # a push gate. `case_a_build_dir_prefix_is_not_walked` is what stops it
        # being tidied away, because no assertion about results can see it.
        if prefix and _pruned(prefix):
            return []
        if os.path.isfile(base):
            return [prefix]
        out: list[str] = []
        for dirpath, dirnames, filenames in os.walk(base):
            # Prune rather than filter: descending into `target/` to throw
            # the results away is minutes of stat() on this tree, and
            # descending into `.git` is the whole object store.
            dirnames[:] = sorted(d for d in dirnames if d not in _PRUNE)
            rel_dir = _norm(dirpath.replace("\\", "/")[len(self.root):])
            for name in filenames:
                rel = f"{rel_dir}/{name}" if rel_dir else name
                # The filenames are filtered too, not just the directories,
                # because in a linked worktree `.git` is a *file* -- it holds
                # the line `gitdir: …/worktrees/os-lane-b` -- and pruning only
                # `dirnames` would miss it. Every lane works in such a
                # worktree, so that is not the exotic case here, it is the
                # only case: the seam would have listed `.git` from the disk
                # and never from a revision, breaking its own equality
                # promise everywhere it actually runs while passing a fixture
                # built by `git init`, where `.git` is a directory.
                if _pruned(rel):
                    continue
                out.append(rel)
        return sorted(out)

    def entries(self, prefix: str) -> list[tuple[str, bool]]:
        base = self._abs(prefix)
        prefix = _norm(prefix)
        # No prefix guard here, deliberately, unlike `files_under`: one
        # `listdir` of a build directory costs a single syscall, and the
        # per-entry prune below already gives the right answer. The guard
        # there buys a whole avoided tree walk; here it would buy nothing and
        # would be a second copy of a rule with no reason of its own.
        try:
            names = os.listdir(base)
        except OSError:
            return []
        out = []
        for name in names:
            rel = f"{prefix}/{name}" if prefix else name
            if _pruned(rel):
                continue
            out.append((rel, os.path.isdir(f"{base}/{name}")))
        return sorted(out)

    def is_file(self, rel: str) -> bool:
        return os.path.isfile(self._abs(rel))

    def is_dir(self, rel: str) -> bool:
        return os.path.isdir(self._abs(rel))

    def read_bytes(self, rel: str) -> Optional[bytes]:
        try:
            with open(self._abs(rel), "rb") as handle:
                return handle.read()
        except OSError:
            return None


class RevTree(Tree):
    """The files as they are at a git revision.

    The path index is built once, from a single `git ls-tree -r -z <rev>` of
    the whole tree -- 13,821 paths in 3.8 s on this machine, measured while a
    push was competing for the disk -- and every listing and predicate is
    then answered from memory. It indexes the whole tree rather than the
    caller's prefix because a checker asks about several prefixes and the
    per-call cost of `ls-tree` (1.5-4 s) is the whole cost; asking once for
    everything is cheaper than asking three times for a third of it.

    Only `read_bytes` talks to git again, over the shared
    `git cat-file --batch`, and that is what makes reading a whole crate
    affordable: 2,305 blobs of `posix/src` in 46 s, against the ~27 minutes
    `known-issues.md` records for one `git cat-file` process per file. That
    measurement is the reason gate 11 -- which must see a whole crate,
    because a link in one file is satisfied by a definition in another --
    can be fixed at all.

    Directories are inferred from the path list, because git has no
    directory entries in a `-r` listing -- a directory here is exactly "a
    prefix that some file has", which is also what git itself means by one.
    """

    def __init__(self, rev: str, repo: Optional[str] = None) -> None:
        self.rev = rev
        self._git = GitTree(repo)
        try:
            paths = self._git.list_paths(rev)
        except GitTreeError:
            self._git.close()
            raise
        # `target/` is gitignored, so it should never appear -- but a tree
        # that once committed one would otherwise make the two Tree
        # implementations disagree about a listing, which is the one thing
        # the seam promises they do not.
        self._files = sorted(p for p in paths if not _pruned(p))
        self._fileset = set(self._files)
        self._dirs: set[str] = set()
        for path in self._files:
            parts = path.split("/")
            for i in range(1, len(parts)):
                self._dirs.add("/".join(parts[:i]))

    def close(self) -> None:
        self._git.close()

    def files_under(self, prefix: str) -> list[str]:
        prefix = _norm(prefix)
        if not prefix:
            return list(self._files)
        if prefix in self._fileset:
            return [prefix]
        head = prefix + "/"
        return [p for p in self._files if p.startswith(head)]

    def entries(self, prefix: str) -> list[tuple[str, bool]]:
        prefix = _norm(prefix)
        head = prefix + "/" if prefix else ""
        seen: dict[str, bool] = {}
        for path in self._files:
            if not path.startswith(head):
                continue
            rest = path[len(head):]
            if not rest:
                continue
            name, sep, _ = rest.partition("/")
            seen[head + name] = bool(sep)
        return sorted(seen.items())

    def is_file(self, rel: str) -> bool:
        return _norm(rel) in self._fileset

    def is_dir(self, rel: str) -> bool:
        rel = _norm(rel)
        return rel == "" or rel in self._dirs

    def read_bytes(self, rel: str) -> Optional[bytes]:
        return self._git.read(self.rev, _norm(rel))


def open_tree(root: str, head: Optional[str] = None) -> Tree:
    """The tree a checker should read: `head`'s revision, or the disk.

    The one place the `--head` flag turns into a choice, so that no checker
    has to spell the fallback rule out again -- and so that "absent means the
    working tree" is stated once rather than seven times, each free to drift.
    """
    if head is None:
        return WorkTree(root)
    return RevTree(head, root)


# --------------------------------------------------------------------------
# materialise: the CLI the shell hook uses
# --------------------------------------------------------------------------


def _under(root: str, rel: str) -> str:
    """`root` joined to a git path, with forward slashes throughout.

    Not `os.path.join`. On Windows that yields `C:/tmp/x\\userspace\\zip.rs` —
    a mix of separators that Python and rustfmt both accept, and that breaks
    the two things the *caller* does with the result: MSYS tools do not agree
    with Windows about a backslash, and gate 7 strips the mirror root back off
    with a shell prefix removal, which cannot match a prefix whose separators
    were rewritten. A path that names the right file but cannot have its root
    stripped shows the pusher a diff headed with a temp directory they cannot
    open, which is the failure this whole mirror exists to avoid.
    """
    return root.replace("\\", "/").rstrip("/") + "/" + rel


def _seed_rust_mod_stubs(root: str, written: Iterable[str]) -> None:
    """Give each materialised file the submodules rustfmt will look for.

    rustfmt loads submodules relative to the file it is handed and refuses
    outright if one is missing ("failed to resolve mod `backup`"). Siblings
    that are themselves in the caller's list are already on disk; the rest get
    a one-byte stub.

    That stub is not a workaround, it is the thing that makes the caller's
    stated scope true: a stub contributes no diff, so a module root can no
    longer drag its untouched children into a verdict that was supposed to
    cover only the files in the push. It is sound because rustfmt's verdict on
    a file does not depend on its children — which is exactly the property that
    does *not* hold for a checker resolving names across a crate, and is why
    gate 11 cannot reuse this.

    Run after every real blob is on disk, never interleaved with them, so a
    stub can never be mistaken for a sibling that had not been written yet.
    """
    for rel in written:
        abs_path = _under(root, rel)
        parent = abs_path.rsplit("/", 1)[0]
        try:
            with open(abs_path, "rb") as handle:
                body = handle.read()
        except OSError:
            continue
        for match in _MOD_DECL.finditer(body):
            name = match.group(1).decode("ascii")
            # Never overwrite: a sibling already here is a real pushed file and
            # must keep its own bytes and its own verdict.
            if os.path.exists(f"{parent}/{name}.rs"):
                continue
            if os.path.exists(f"{parent}/{name}/mod.rs"):
                continue
            try:
                with open(f"{parent}/{name}.rs", "wb") as handle:
                    handle.write(_STUB)
            except OSError:
                # A stub we could not write shows up as rustfmt failing to
                # resolve the mod, which the caller reports. Silence here would
                # only be wrong if it could turn a finding green, and it cannot.
                pass


def materialise(
    rev: str,
    dest: str,
    paths: Iterable[str],
    stub_rust_mods: bool,
    repo: Optional[str] = None,
) -> list[str]:
    """Lay the bytes of `paths` at `rev` out under `dest`, at their real
    relative paths. Returns the *absolute* destination paths actually written,
    in input order; a path not present at `rev` is skipped.

    The relative layout matters: a tool handed `/tmp/x.1234/userspace/zip/src/
    main.rs` can resolve its siblings, and a diff can have the root stripped
    back off so it names a file the caller can actually open.
    """
    written: list[str] = []
    with GitTree(repo) as tree:
        for path, data in tree.read_many(rev, paths):
            if data is None:
                continue
            out_path = _under(dest, path)
            os.makedirs(out_path.rsplit("/", 1)[0], exist_ok=True)
            with open(out_path, "wb") as handle:
                handle.write(data)
            written.append(path)
    if stub_rust_mods:
        _seed_rust_mod_stubs(dest, written)
    return [_under(dest, p) for p in written]


def _read_path_list(stream: object) -> list[str]:
    """One path per line, blank lines dropped. Newline-separated rather than
    NUL, because the caller is `git log --name-only | sort -u` in a POSIX
    shell, which is line-oriented itself — a NUL-separated protocol here would
    be precise about a case the producer cannot represent anyway.

    `split("\\n")` and not `splitlines()`: the latter also breaks on `\\v`,
    `\\f`, `\\x1c` and `U+2028`, every one of which is a legal byte in a path
    here (this filesystem forbids only `/` and NUL), so it would silently tear
    a real filename into two paths that name nothing. A trailing `\\r` is
    stripped because a Windows Python writing the list would have added one.
    """
    data = stream.read()  # type: ignore[attr-defined]
    if isinstance(data, bytes):
        text = data.decode("utf-8", "surrogateescape")
    else:
        text = data
    lines = [line[:-1] if line.endswith("\r") else line for line in text.split("\n")]
    return [line for line in lines if line.strip()]


def _print_path(path: str) -> None:
    """One result path, LF-terminated, whatever platform this is.

    Not `print`. On Windows, `print` writes through a text-mode stdout that
    turns the `\\n` into `\\r\\n`, and the consumer is a POSIX shell running
    `IFS= read -r`, which strips the `\\n` and keeps the `\\r`. The result is a
    filename with an invisible carriage return on the end, which rustfmt
    reports as "file does not exist" — and gate 7 reads any rustfmt failure as
    drift, so *every clean file in the push* was refused. Caught by
    `test-pre-push-fmt-gate.py`'s batched run, which is the entire reason that
    suite runs every case under both mirror modes.
    """
    sys.stdout.buffer.write(path.encode("utf-8", "surrogateescape") + b"\n")


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        prog="gittree.py", description=__doc__.splitlines()[0]
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    mat = sub.add_parser(
        "materialise",
        help="write the pushed bytes of a list of paths under one root",
    )
    mat.add_argument("--rev", default="HEAD", help="revision to read (default HEAD)")
    mat.add_argument("--dest", required=True, help="root to write under")
    mat.add_argument(
        "--stub-rust-mods",
        action="store_true",
        help="seed one-byte stubs for `mod name;` siblings not in the list",
    )

    lst = sub.add_parser("list", help="list paths under a revision")
    lst.add_argument("--rev", default="HEAD")
    lst.add_argument("pathspec", nargs="*")

    args = parser.parse_args(argv)

    try:
        if args.cmd == "materialise":
            paths = _read_path_list(sys.stdin)
            os.makedirs(args.dest, exist_ok=True)
            for out_path in materialise(
                args.rev, args.dest, paths, args.stub_rust_mods
            ):
                _print_path(out_path)
            sys.stdout.buffer.flush()
            return 0
        with GitTree() as tree:
            for path in tree.list_paths(args.rev, *args.pathspec):
                _print_path(path)
        sys.stdout.buffer.flush()
        return 0
    except GitTreeError as exc:
        # Non-zero and a named reason, so a shell caller can fall back to its
        # slow path and say why rather than silently getting slower.
        print(f"gittree.py: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
