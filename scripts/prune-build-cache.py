#!/usr/bin/env python3
"""Prune the parts of a cargo `target/` that no recent build has used.

Why this exists
---------------

`scripts/reclaim-space.py` is the emergency valve: when the volume is nearly
full it deletes whole `target/` trees, and whoever owned one pays a full cold
rebuild.  That is the right shape for an emergency and the wrong shape for
routine housekeeping, and routine housekeeping is what was actually missing.
On 2026-09-02 the three lane caches held 48 GB, 180 GB and ~50 GB with 77 GiB
free on the volume -- so far from the floor that `reclaim-space.py` reported
"Nothing to do", yet lane B's cache alone contained 19,769 units that no build
had touched in over a week.  CLAUDE.md's rule ("clean up build artifacts after
yourself") was being obeyed to the letter: there were no stray scratch target
dirs at all.  The growth was *inside* the sanctioned caches, which no rule and
no tool covered.

Cargo never garbage-collects `target/`.  When a unit's inputs change it mints a
new `-<hash>` filename and writes beside the old one; the superseded artifact
and its `.fingerprint/<name>-<hash>/` directory stay forever.  Over a few
thousand commits that is most of the tree.  Lane B's `deps/` held 63,841 files
to lane A's 12,900, with `chown` alone at 28 distinct hashes.

What makes a unit safe to delete
--------------------------------

The discriminator is cargo's own record, not a guess: every
`.fingerprint/<name>-<hash>/` contains an `invoked.timestamp` that cargo
touches whenever that unit takes part in a build -- *including* builds where it
was found fresh and nothing recompiled.  So "invoked.timestamp is 30 days old"
means precisely "no build in 30 days has needed this unit", which is the
question we actually want answered.

Two rejected alternatives, both of which delete live data:

* **mtime** ("when was it built") is what `cargo-sweep` uses.  It ages out a
  stable dependency's `.rlib` that was compiled once and has been linked by
  every build since.  Common, current, and by that rule "old".
* **atime** is enabled on this volume (`fsutil behavior query
  DisableLastAccess` -> `2 (System Managed, ENABLED)`), and is still wrong: a
  current artifact that has not changed shows `atime == mtime`, because cargo
  only `stat`s it rather than reading it.  Measuring gave "6.6 GB never read
  since built" -- a figure that includes plenty of live artifacts.

Note also that several hashes per crate is *not* itself evidence of garbage.
`a2ps-cli-154eb5188e622895/` holds `bin-a2ps-cli` and
`a2ps-cli-fc15d6b797050592/` holds `test-bin-a2ps-cli`: two unit kinds of one
crate, both current.  Only the timestamp separates them.

The `incremental/` lever
------------------------

`incremental/` is sized and pruned separately because it fails differently.  It
was 31.6 GB of lane A's 48.4 GB -- 65% of the tree -- and none of it is needed
to *link*: it is rustc's per-crate memoisation, read only when that crate is
recompiled.  So a crate that has not been recompiled in weeks is carrying a
cache that costs disk continuously and can only ever repay it if that crate
changes again.  Deleting it costs exactly one non-incremental rebuild of that
one crate, and nothing at all if the crate never changes.  Its age is the
newest mtime inside the unit directory (cargo rewrites the session dir on every
incremental compile), which for this cache genuinely does mean "last compiled".

That is the answer to "prune only what is unlikely to be used for incremental
rebuilds": hot crates keep both their artifacts and their incremental caches,
and only units that have sat out every recent build are taken.

Deleting safely
---------------

Two failure modes, both avoided by construction.

*Half-pruned units.*  Deleting an artifact but leaving its fingerprint is the
one way to actually break a cache: cargo reads the fingerprint, concludes the
unit is fresh, and then fails to link something that is not there.  The reverse
is harmless -- a missing fingerprint just means "rebuild it".  So the
fingerprint is always removed *first* and the artifacts after, which makes even
an interrupted run leave a cache that is merely colder, never broken.

*Pruning under a running build.*  Every candidate is *renamed* into a staging
directory before anything is deleted.  Windows refuses to rename a directory
with any file open inside it and the rename is atomic, so a success proves
nothing held it at that instant; a failure means "in use" and the unit is
skipped, not forced.  This is the same fact-based test `reclaim-space.py` uses,
and it is why this script does not need to guess whether another lane is
building.  Staging also makes the delete cheap: 20,000 units become one
`rd /s /q` at the end rather than 20,000 separate recursive removals.

Only `target/` is ever touched.  It is regenerable by definition and gitignored,
so the worst outcome of any decision here is a slower build, never lost work.

Usage
-----

    python scripts/prune-build-cache.py                    # report, change nothing
    python scripts/prune-build-cache.py --yes              # prune this worktree
    python scripts/prune-build-cache.py --age-days 30      # be more conservative
    python scripts/prune-build-cache.py --target-dir "D:/.../os-lane-b/target" --yes

Exit codes: 0 fine (including "nothing to prune"), 1 something could not be
removed, 2 bad usage.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time

# Cargo's unit hashes are 16 lowercase hex digits.  Anchoring to exactly 16
# rather than "8 or more" matters: a crate legitimately named `base64` or
# `adler32` would otherwise have part of its own name eaten by a loose pattern.
# The name part is greedy so that the *last* `-<16 hex>` group wins, which is
# the hash even when the crate name itself contains a hex-looking segment.
_FP_RE = re.compile(r"^(?P<name>.+)-(?P<hash>[0-9a-f]{16})$")
_DEP_RE = re.compile(r"^(?:lib)?(?P<name>.+)-(?P<hash>[0-9a-f]{16})(?P<ext>\..*)?$")

GIB = 1024.0 ** 3


def norm(name):
    """Join key for a unit.

    A package is `a2ps-cli` in `.fingerprint/` but its crate artifacts are
    `a2ps_cli-<hash>.d` in `deps/`, because cargo hyphenates package names and
    underscores crate names.  Folding `-` to `_` makes the two spellings meet;
    the hash then makes the match exact.
    """
    return name.replace("-", "_")


def run(argv):
    try:
        p = subprocess.run(argv, capture_output=True, text=True)
        return p.returncode, (p.stdout or "") + (p.stderr or "")
    except OSError as exc:
        return 1, str(exc)


def rmtree_fast(path):
    """Delete `path` recursively with the platform's native bulk remover.

    Same reasoning as `reclaim-space.py`: `shutil.rmtree` issues one
    interpreter-crossing unlink per entry and on this volume cleared ~1.4 GiB
    in ten minutes, because a cargo `target/` is hundreds of thousands of tiny
    files and every operation pays the filesystem-filter tax.  `rd /s /q` does
    the same walk in one native process.
    """
    if not os.path.exists(path):
        return
    if os.name == "nt":
        rc, _ = run(["cmd", "/c", "rd", "/s", "/q", path])
        if rc == 0 and not os.path.exists(path):
            return
    shutil.rmtree(path, ignore_errors=True)


def tree_size(path):
    """Bytes under `path`, following no links and swallowing races.

    Only ever called on the *candidates*, never on the whole cache: sizing
    250 GB of tiny files is what made the exploratory measurements take hours,
    and the number we need is "how much would this run free", which is a walk
    of the pruned set alone.
    """
    total = 0
    stack = [path]
    while stack:
        cur = stack.pop()
        try:
            with os.scandir(cur) as it:
                for entry in it:
                    try:
                        if entry.is_dir(follow_symlinks=False):
                            stack.append(entry.path)
                        else:
                            total += entry.stat(follow_symlinks=False).st_size
                    except OSError:
                        pass
        except OSError:
            pass
    return total


def file_size(path):
    try:
        return os.lstat(path).st_size
    except OSError:
        return 0


def newest_mtime(path, max_depth=1):
    """Newest mtime at or under `path`, as a POSIX timestamp.

    Used for `incremental/` units, where the meaningful age is "when did rustc
    last write a session here" -- i.e. when this crate was last *compiled*.
    The unit directory's own mtime will not do on its own, because Windows does
    not propagate a nested write up to the parent.

    Depth 1 -- the unit directory and its immediate children -- is not an
    approximation.  An `incremental/<crate>-<hash>/` holds exactly a session
    directory and a lock file, and a directory's mtime *is* updated when an
    entry is created or removed inside it.  So the session directory's own
    mtime already records the compile that wrote its files, and reading the
    thousands of files inside cannot produce an earlier or later answer than
    reading the directory that contains them.

    That distinction is worth a whole order of magnitude here.  Recursing
    without a limit spent 11 minutes on a 31.6 GB `incremental/`; descending
    one level further than this, into the session directories, still meant
    stat-ing millions of files to learn what ~24,000 directory stats already
    said.
    """
    newest = 0.0
    stack = [(path, 0)]
    while stack:
        cur, depth = stack.pop()
        try:
            newest = max(newest, os.lstat(cur).st_mtime)
        except OSError:
            pass
        if depth >= max_depth:
            continue
        try:
            with os.scandir(cur) as it:
                for entry in it:
                    try:
                        if entry.is_dir(follow_symlinks=False):
                            stack.append((entry.path, depth + 1))
                        else:
                            newest = max(
                                newest, entry.stat(follow_symlinks=False).st_mtime
                            )
                    except OSError:
                        pass
        except OSError:
            pass
    return newest


_BUCKETS = (1.0, 3.0, 7.0, 14.0, 30.0, 90.0)


def histogram(ages):
    """Render an age distribution as `<1d:12 <3d:0 <7d:4 ... older:9`.

    This exists because a pruner that reports "nothing to prune" is
    indistinguishable, from the outside, from one whose staleness test is
    silently broken -- and that is not a hypothetical failure here: an earlier
    measurement tonight reported "0.0 GB incremental" for a directory holding
    31.6 GB, because a regex had been mangled by shell quoting, and the wrong
    answer was believed.  Printing the distribution makes the negative result
    falsifiable: "every unit is under a day old" is a claim the reader can
    check against when the lane last built, whereas a bare "0 stale" is not a
    claim about anything.
    """
    counts = [0] * (len(_BUCKETS) + 1)
    for age in ages:
        for i, edge in enumerate(_BUCKETS):
            if age < edge:
                counts[i] += 1
                break
        else:
            counts[-1] += 1
    parts = [f"<{int(e)}d:{c}" for e, c in zip(_BUCKETS, counts) if c]
    if counts[-1]:
        parts.append(f"older:{counts[-1]}")
    return " ".join(parts) if parts else "(none)"


class Candidate:
    """One prunable thing: a set of paths that must be removed together.

    `fingerprint` is separated from `artifacts` because the removal order
    between them is the whole crash-safety argument (see the module docstring):
    fingerprint first, artifacts second.
    """

    __slots__ = ("label", "age_days", "fingerprint", "artifacts", "nbytes")

    def __init__(self, label, age_days, fingerprint, artifacts):
        self.label = label
        self.age_days = age_days
        self.fingerprint = fingerprint
        self.artifacts = artifacts
        self.nbytes = 0


def scan_profile(profile_dir, age_days, incr_age_days, now, note=lambda _s: None):
    """Classify one `<target>/[<triple>/]<profile>/` directory.

    Returns `(stale, live_units, live_bytes_unknown)` where `stale` is a list of
    `Candidate`.

    `note` is called with a one-line progress string as each pass finishes.
    One profile can take minutes on a large cache, so a caller that only
    printed a summary per profile would still show nothing for most of the run
    -- the same blind spot as the buffering bug, one level down.
    """
    fp_root = os.path.join(profile_dir, ".fingerprint")
    deps_root = os.path.join(profile_dir, "deps")
    build_root = os.path.join(profile_dir, "build")
    incr_root = os.path.join(profile_dir, "incremental")

    if not os.path.isdir(fp_root):
        return [], 0, 0

    # --- pass 1: age every fingerprint unit -------------------------------
    units = {}  # (norm_name, hash) -> (dir_path, age_days)
    live_keys = set()
    stale_units = []
    n_live = 0
    try:
        entries = list(os.scandir(fp_root))
    except OSError:
        return [], 0, 0
    n_undated = 0
    empty_dirs = []
    for entry in entries:
        if not entry.is_dir(follow_symlinks=False):
            continue
        m = _FP_RE.match(entry.name)
        if not m:
            # Not a `name-<hash>` unit directory.  Unknown shape: leave it.
            continue
        key = (norm(m.group("name")), m.group("hash"))
        stamp = os.path.join(entry.path, "invoked.timestamp")
        try:
            mtime = os.lstat(stamp).st_mtime
        except OSError:
            # No `invoked.timestamp` means cargo never recorded a use for this
            # unit in a form we can read.  Absence of evidence is not evidence
            # of staleness, so treat it as live and leave it alone -- but count
            # it, because a unit the discriminator could not read is a unit this
            # scan did not actually examine, and folding it into "live" without
            # saying so is how "0 stale" comes to mean two different things.
            #
            # There are two real populations here, and only one is interesting.
            # Build-script *run* units (`run-build-script-*`) legitimately have
            # no `invoked.timestamp` and are a handful per profile.  The other
            # is an entirely *empty* unit directory, of which the bare-metal
            # profile in this worktree had 3016 out of 3104 -- cargo creates
            # the directory for every unit in the resolve graph and only fills
            # it in for the ones it actually builds for that target.
            try:
                is_empty = not any(os.scandir(entry.path))
            except OSError:
                is_empty = False
            if is_empty:
                empty_dirs.append(entry.path)
            live_keys.add(key)
            n_live += 1
            n_undated += 1
            continue
        age = (now - mtime) / 86400.0
        units[key] = (entry.path, age)
        if age > age_days:
            stale_units.append(key)
        else:
            live_keys.add(key)
            n_live += 1

    # `dated` rather than the unit total, because the histogram only describes
    # the units the discriminator could actually read.  Saying "0 of 3104 stale"
    # beside a distribution covering 85 of them invites exactly the reading the
    # histogram exists to prevent: that all 3104 were examined and found fresh.
    dated = len(units)
    undated_note = f"; {n_undated} undated (kept)" if n_undated else ""
    if empty_dirs:
        undated_note += f", {len(empty_dirs)} of them empty"
    note(
        f"{len(stale_units)} of {len(stale_units) + n_live} units stale "
        f"by invoked.timestamp{undated_note}; ages of the {dated} dated: "
        f"{histogram(u[1] for u in units.values())}"
    )

    # --- pass 2: index the artifacts by the same key ----------------------
    by_key = {}
    for root in (deps_root, build_root):
        if not os.path.isdir(root):
            continue
        try:
            entries = list(os.scandir(root))
        except OSError:
            continue
        for entry in entries:
            m = _DEP_RE.match(entry.name)
            if not m:
                continue
            key = (norm(m.group("name")), m.group("hash"))
            by_key.setdefault(key, []).append(entry.path)

    # --- pass 3: build candidates ----------------------------------------
    stale = []
    for key in stale_units:
        fp_dir, age = units[key]
        # Belt and braces: never take an artifact that a *live* unit also
        # claims.  Hash collision across two unit names is vanishingly
        # unlikely, but the cost of the check is one set lookup and the cost of
        # being wrong is a broken cache.
        if key in live_keys:
            continue
        arts = by_key.get(key, [])
        stale.append(Candidate(f"{key[0]}-{key[1]}", age, fp_dir, arts))

    # --- pass 4: incremental caches, aged independently -------------------
    # `incr_age_days is None` means "leave incremental/ alone", and it short-
    # circuits before the walk rather than after it: dating these directories
    # costs a recursive stat of tens of thousands of files, which is most of
    # this script's runtime.
    if incr_age_days is not None and os.path.isdir(incr_root):
        try:
            entries = list(os.scandir(incr_root))
        except OSError:
            entries = []
        note(f"dating {len(entries)} incremental caches")
        n_incr = 0
        incr_ages = []
        for entry in entries:
            if not entry.is_dir(follow_symlinks=False):
                continue
            mtime = newest_mtime(entry.path)
            age = (now - mtime) / 86400.0 if mtime else 1e9
            incr_ages.append(age)
            if age > incr_age_days:
                # An incremental unit has no fingerprint of its own and no
                # artifact anyone links against, so it is a standalone
                # candidate: the directory is the whole of it.
                stale.append(
                    Candidate(f"incremental/{entry.name}", age, entry.path, [])
                )
                n_incr += 1
        note(f"{n_incr} incremental caches cold; ages {histogram(incr_ages)}")

    return stale, n_live, 0


def find_profiles(target_dir):
    """Every `<profile>/` under `target/` that looks like a cargo output dir.

    Handles both layouts: `target/debug` (host builds) and
    `target/<triple>/debug` (cross builds).  The test is the presence of
    `.fingerprint`, so a directory that merely shares the name is not mistaken
    for one.
    """
    found = []
    try:
        level1 = list(os.scandir(target_dir))
    except OSError:
        return found
    for a in level1:
        if not a.is_dir(follow_symlinks=False):
            continue
        if os.path.isdir(os.path.join(a.path, ".fingerprint")):
            found.append(a.path)
            continue
        try:
            level2 = list(os.scandir(a.path))
        except OSError:
            continue
        for b in level2:
            if b.is_dir(follow_symlinks=False) and os.path.isdir(
                os.path.join(b.path, ".fingerprint")
            ):
                found.append(b.path)
    return found


def prune(candidates, staging, verbose):
    """Remove `candidates`, newest-safe order, skipping anything in use.

    Returns `(taken, skipped, errors)`.  Fingerprints are renamed into
    `staging` first -- that rename is simultaneously the in-use test and the
    removal -- and the artifacts are unlinked only once it has succeeded.
    """
    os.makedirs(staging, exist_ok=True)
    taken = skipped = errors = 0
    for i, cand in enumerate(candidates):
        dest = os.path.join(staging, f"{i:07d}")
        try:
            os.rename(cand.fingerprint, dest)
        except OSError as exc:
            # In use, or gone since the scan.  Either way: leave the artifacts
            # alone, because a unit with artifacts but no fingerprint is the
            # only state cargo cannot recover from.
            skipped += 1
            if verbose:
                print(f"  skip (in use) {cand.label}: {exc}")
            continue
        for j, path in enumerate(cand.artifacts):
            try:
                if os.path.isdir(path):
                    os.rename(path, os.path.join(staging, f"{i:07d}.d{j}"))
                else:
                    os.remove(path)
            except OSError as exc:
                # The fingerprint is already gone, so cargo will rebuild this
                # unit regardless; a stuck artifact is a space leak, not a
                # correctness problem.  Report it and move on.
                errors += 1
                if verbose:
                    print(f"  leaked {path}: {exc}")
        taken += 1
    return taken, skipped, errors


def self_test():
    """Prove the classification and the pairing on a synthetic `target/`.

    This exists because the interesting failure of a pruner is silent.  A run
    that deletes too little merely wastes disk and says "0 stale", which is
    indistinguishable from a correct run on a hot cache -- and lane A's cache
    *is* entirely hot, so the live tree cannot tell the two apart.  A run that
    deletes too much is worse and equally quiet: cargo simply rebuilds, and the
    only symptom is a slow build nobody attributes to this script.

    So the guarantees are asserted here against a tree with known contents
    rather than inferred from a real one:

      * the name/hash join survives cargo's own inconsistency, where a package
        is `beta-cli` in `.fingerprint/` but `beta_cli-<hash>` in `deps/`;
      * an artifact belonging to a *live* unit is never taken as collateral,
        including when it shares a crate name with a stale one;
      * an artifact no fingerprint claims is left alone, because "unknown" is
        not "garbage";
      * `incremental/` is aged on its own threshold, independently of units;
      * after a prune, no artifact is ever left behind a *surviving*
        fingerprint -- the one state cargo cannot recover from.

    Returns `(checks_run, failures)`.  The count is returned rather than just a
    pass/fail because a suite that silently stops asserting looks identical,
    from the outside, to one that passes -- so the caller can put a floor under
    it.
    """
    import tempfile

    fails = []
    checks = []

    def check(cond, what):
        checks.append(what)
        if not cond:
            fails.append(what)
        print(f"  {'ok  ' if cond else 'FAIL'}  {what}")

    now = time.time()
    old = now - 60 * 86400
    with tempfile.TemporaryDirectory() as tmp:
        prof = os.path.join(tmp, "target", "x86_64-unknown-none", "debug")
        fp = os.path.join(prof, ".fingerprint")
        deps = os.path.join(prof, "deps")
        incr = os.path.join(prof, "incremental")
        for d in (fp, deps, incr, os.path.join(prof, "build")):
            os.makedirs(d)

        def unit(name, hash_, when):
            d = os.path.join(fp, f"{name}-{hash_}")
            os.makedirs(d)
            stamp = os.path.join(d, "invoked.timestamp")
            with open(stamp, "w", newline="") as fh:
                fh.write("x")
            os.utime(stamp, (when, when))
            return d

        def artifact(fname, size=1024):
            p = os.path.join(deps, fname)
            with open(p, "wb") as fh:
                fh.write(b"\0" * size)
            return p

        # A live unit, and a stale one whose package name is hyphenated where
        # its artifacts are underscored -- the join that a naive matcher gets
        # wrong.
        live_fp = unit("alpha", "0123456789abcdef", now)
        stale_fp = unit("beta-cli", "fedcba9876543210", old)
        # A second unit of the *same crate*, still live, on a different hash:
        # the `bin-` vs `test-bin-` case that made multiple hashes per crate
        # look like garbage when it is not.
        live2_fp = unit("beta-cli", "aaaabbbbccccdddd", now)

        live_art = artifact("libalpha-0123456789abcdef.rlib")
        stale_arts = [
            artifact("beta_cli-fedcba9876543210.d"),
            artifact("libbeta_cli-fedcba9876543210.rlib"),
            artifact("beta_cli-fedcba9876543210.exe"),
        ]
        live2_art = artifact("beta_cli-aaaabbbbccccdddd.exe")
        # Claimed by no fingerprint at all.
        orphan_art = artifact("gamma-1111111111111111.d")

        def incr_unit(name, when):
            d = os.path.join(incr, name)
            sess = os.path.join(d, "s-abcdefgh-0000")
            os.makedirs(sess)
            with open(os.path.join(sess, "dep-graph.bin"), "wb") as fh:
                fh.write(b"\0" * 4096)
            for p in (sess, d):
                os.utime(p, (when, when))
            return d

        hot_incr = incr_unit("alpha-1abcdefghij", now)
        cold_incr = incr_unit("beta_cli-2abcdefghij", old)

        # Two units the discriminator cannot read.  Both must be kept -- absence
        # of evidence is not evidence of staleness -- and both must be *said*,
        # because a unit folded silently into "live" is one the scan did not
        # examine while reporting as though it had.  This worktree's bare-metal
        # profile is 3016 empty directories out of 3104, so a report that
        # elided them would describe 3% of the profile as though it were all
        # of it.
        empty_fp = os.path.join(fp, "delta-9999999999999999")
        os.makedirs(empty_fp)
        script_fp = os.path.join(fp, "epsilon-8888888888888888")
        os.makedirs(script_fp)
        with open(os.path.join(script_fp, "run-build-script-build-script-build"),
                  "w", newline="") as fh:
            fh.write("x")

        notes = []
        cands, n_live, _ = scan_profile(prof, 14.0, 7.0, now, note=notes.append)
        labels = {c.label for c in cands}
        first = notes[0] if notes else ""

        check(os.path.isdir(empty_fp), "an undated unit is kept, not pruned")
        check(os.path.isdir(script_fp),
              "and so is a build-script unit that never gets a timestamp")
        check("2 undated (kept)" in first,
              f"both undated units are reported, not folded into live ({first})")
        check("1 of them empty" in first,
              f"and the empty one is called out separately ({first})")
        check("ages of the 3 dated" in first,
              f"the histogram says how many units it actually covers ({first})")

        check(n_live == 4, f"two units are live, plus the two undated (got {n_live})")
        check(
            "beta_cli-fedcba9876543210" in labels,
            "the stale unit is a candidate despite the hyphen/underscore split",
        )
        check(
            not any(c.label.startswith("alpha-") for c in cands),
            "the live unit is not a candidate",
        )
        check(
            "beta_cli-aaaabbbbccccdddd" not in labels,
            "a second live unit of the same crate is not a candidate",
        )
        check(
            "incremental/beta_cli-2abcdefghij" in labels,
            "the cold incremental cache is a candidate",
        )
        check(
            "incremental/alpha-1abcdefghij" not in labels,
            "the hot incremental cache is not",
        )

        stale_cand = next(
            (c for c in cands if c.label == "beta_cli-fedcba9876543210"), None
        )
        check(
            stale_cand is not None and len(stale_cand.artifacts) == 3,
            "all three of the stale unit's artifacts are paired to it "
            f"(got {len(stale_cand.artifacts) if stale_cand else 'no candidate'})",
        )

        # Snapshot which units actually *had* an artifact before the prune, so
        # the invariant below can distinguish "lost its artifacts" from "never
        # had any".  Some units legitimately have none -- a build-script run
        # unit, and an empty directory cargo created for a unit it did not
        # build for this target -- and an invariant that cannot tell those from
        # a real orphan fires on a healthy cache, which is how a check gets
        # weakened instead of the code being fixed.
        def artifact_keys():
            keys = set()
            for art in os.listdir(deps):
                dm = _DEP_RE.match(art)
                if dm:
                    keys.add((norm(dm.group("name")), dm.group("hash")))
            return keys

        had_artifacts = artifact_keys()

        prune(cands, os.path.join(tmp, "target", ".prune-staging"), False)
        rmtree_fast(os.path.join(tmp, "target", ".prune-staging"))

        check(not os.path.exists(stale_fp), "the stale fingerprint is gone")
        check(
            not any(os.path.exists(p) for p in stale_arts),
            "and so are its artifacts",
        )
        check(os.path.exists(live_fp), "the live fingerprint survives")
        check(os.path.exists(live_art), "and its artifact")
        check(os.path.exists(live2_fp), "so does the same crate's other unit")
        check(os.path.exists(live2_art), "and that unit's artifact")
        check(os.path.exists(orphan_art), "an unclaimed artifact is left alone")
        check(os.path.exists(hot_incr), "the hot incremental cache survives")
        check(not os.path.exists(cold_incr), "the cold one does not")

        # The invariant that matters most: no fingerprint that survived may have
        # lost the artifacts it had.  That is the one state cargo cannot recover
        # from -- it reads the fingerprint, concludes the unit is fresh, and
        # fails at link time -- so it is checked directly rather than inferred
        # from the individual assertions above.
        #
        # Scoped to units that had an artifact to begin with: see
        # `had_artifacts`.
        still = artifact_keys()
        broken = []
        for name in set(os.listdir(fp)):
            m = _FP_RE.match(name)
            if not m:
                continue
            key = (norm(m.group("name")), m.group("hash"))
            if key in had_artifacts and key not in still:
                broken.append(name)
        check(
            not broken,
            f"no surviving fingerprint lost its artifacts (orphaned: {broken})",
        )

    print(f"self-test: {len(checks)} check(s), {len(fails)} failure(s)")
    return len(checks), len(fails)


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="Prune cargo build-cache units that no recent build has used."
    )
    ap.add_argument(
        "--target-dir",
        action="append",
        default=None,
        help="target/ directory to prune (repeatable). Default: ./target next "
        "to this script's worktree root.",
    )
    ap.add_argument(
        "--age-days",
        type=float,
        default=14.0,
        help="A unit is stale when cargo has not invoked it in this many days "
        "(default: 14).",
    )
    ap.add_argument(
        "--incremental-age-days",
        type=float,
        default=7.0,
        help="An incremental cache is stale when its crate has not been "
        "recompiled in this many days (default: 7). Lower than --age-days "
        "because incremental data is only ever read when the crate is "
        "recompiled, so a cold one has no other way to earn its space.",
    )
    ap.add_argument(
        "--no-incremental",
        action="store_true",
        help="Leave incremental/ alone entirely.",
    )
    ap.add_argument("--yes", action="store_true", help="Actually delete (default: dry run).")
    ap.add_argument("--verbose", action="store_true", help="Name every skip and leak.")
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="Check the classification and pairing against a synthetic target/ "
        "tree and exit. Touches nothing real.",
    )
    args = ap.parse_args(argv)

    # A scan of a 180 GB cache is minutes of pure metadata I/O, and Python
    # block-buffers stdout the moment it is a pipe rather than a console -- so
    # a run under `scripts/run-timeout.py` printed *nothing at all* for eleven
    # minutes and was indistinguishable from a hang, defeating the very
    # heartbeat that runner exists to provide.  Reconfiguring the stream here
    # rather than relying on `python -u` at the call site means the property
    # belongs to the script, and cannot be lost by whoever invokes it.
    try:
        sys.stdout.reconfigure(line_buffering=True)
    except (AttributeError, OSError):
        pass

    if args.self_test:
        _checks, failures = self_test()
        return 1 if failures else 0

    if args.target_dir:
        targets = [os.path.abspath(t) for t in args.target_dir]
    else:
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        targets = [os.path.join(root, "target")]

    now = time.time()
    grand_bytes = 0
    grand_units = 0
    rc = 0

    for target in targets:
        if not os.path.isdir(target):
            print(f"{target}: no such directory -- skipped")
            continue
        print(f"=== {target}")
        profiles = find_profiles(target)
        if not profiles:
            print("    no cargo output directories found")
            continue
        all_cands = []
        for profile in profiles:
            incr_age = None if args.no_incremental else args.incremental_age_days
            rel = os.path.relpath(profile, target)

            def note(msg, _rel=rel):
                print(f"    [{_rel}] {msg}")

            cands, n_live, _ = scan_profile(
                profile, args.age_days, incr_age, now, note
            )
            # Sizing is a real walk and the slowest phase, so it reports too --
            # but only on a large set, to keep a small cache's output terse.
            chatty = len(cands) > 2000
            if chatty:
                note(f"sizing {len(cands)} candidates")
            for n, c in enumerate(cands, 1):
                c.nbytes = tree_size(c.fingerprint) + sum(
                    tree_size(p) if os.path.isdir(p) else file_size(p)
                    for p in c.artifacts
                )
                if chatty and n % 2000 == 0:
                    note(f"sized {n}/{len(cands)}")
            nbytes = sum(c.nbytes for c in cands)
            print(
                f"    {rel}: {n_live} live units, "
                f"{len(cands)} stale -> {nbytes / GIB:.2f} GiB"
            )
            all_cands.extend(cands)

        nbytes = sum(c.nbytes for c in all_cands)
        grand_bytes += nbytes
        grand_units += len(all_cands)

        if not all_cands:
            continue
        if not args.yes:
            print(f"    (dry run -- pass --yes to reclaim {nbytes / GIB:.2f} GiB)")
            continue

        staging = os.path.join(target, ".prune-staging")
        taken, skipped, errors = prune(all_cands, staging, args.verbose)
        print(f"    removing staged units ...")
        rmtree_fast(staging)
        left = os.path.exists(staging)
        print(
            f"    pruned {taken} units, skipped {skipped} in use, "
            f"{errors} artifacts left behind"
            + (" (staging not fully removed)" if left else "")
        )
        if errors or left:
            rc = 1

    verb = "would reclaim" if not args.yes else "reclaimed"
    print(f"total: {grand_units} stale units, {verb} {grand_bytes / GIB:.2f} GiB")
    return rc


if __name__ == "__main__":
    sys.exit(main())
