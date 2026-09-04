#!/usr/bin/env python3
"""Tests for pre-push gate 11 -- the dead-intra-doc-link gate.

Run: `python scripts/test-pre-push-doclinks-gate.py` (0 = pass, 1 = fail). No
pytest dependency, matching the other suites in this directory.

Why this suite exists
---------------------

On 2026-09-02 a push of 84 commits was refused by this gate, and not because
anything was wrong with the prose. The gate passed the crates to scan as
command-line arguments; the push touched 2,568 directories, which is 64,862
bytes of command line against Windows' 32,767-character limit; the shell
answered "Argument list too long" and the checker exited 126 without reading a
file. The hook did the right thing with that -- an exit code that is not a
verdict must never be read as a pass -- so the outcome was a lane that could
not push at all, for a reason that had nothing to do with its code.

Nothing would have caught it. `test-pre-push-gates.py` checks the hook's
*shape* and this was not a shape error; `check-doc-links.py`'s own self-test
checks its parsing rules and this was not a parsing error. The failure lived
exactly in the seam between them -- how the hook hands the checker its scope --
and it only appears above a scale no existing fixture reached.

So the central case here builds a fixture with enough crates to exceed the
limit, and asserts the gate reaches a verdict. It asserts that in *both*
directions, which is the part that is easy to get wrong: a large fixture that
pushes successfully proves nothing on its own, because a gate that silently
skipped would also let it through. The same fixture is therefore pushed twice,
once clean and once with a single dead link buried in one of the many crates,
and the second must be refused.

Why gate 7 is bypassed
----------------------

These fixtures carry over a thousand `.rs` files, and gate 7 would rustfmt
every one of them -- minutes of work to re-test something
`test-pre-push-fmt-gate.py` already covers properly. `ALLOW_FMT_DRIFT=1` turns
it off for these pushes only. Nothing else is bypassed: the other gates skip
themselves because this fixture does not install their checkers, which is the
same arrangement `test-pre-push-fmt-gate.py` relies on.
"""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gitenv  # noqa: E402

_REMOVED = gitenv.scrub_environ()

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HOOK = os.path.join(REPO_ROOT, "scripts", "hooks", "pre-push")
LIB = os.path.join(REPO_ROOT, "scripts", "run-checker.sh")
CHECKER = os.path.join(REPO_ROOT, "scripts", "check-doc-links.py")
# The checker imports this by name from its own directory, so the fixture has
# to install it too. Copied rather than left to be found in the real checkout:
# the fixture's point is that the repository under test is the throwaway one,
# and a `gittree` resolved from outside it would be reading a `ROOT` that is
# not the tree the gate is judging.
GITTREE = os.path.join(REPO_ROOT, "scripts", "gittree.py")
# And `gittree` imports this by name from *its* own directory, by the same
# argument one level down: in a fixture that directory is the copy above. A
# missing `gitenv.py` does not make the gate read the wrong repository, it
# stops the gate starting at all -- and an import traceback exits 1, which the
# hook reads as a finding and refuses the push over, so the symptom arrives
# disguised as a verdict.
GITENV = os.path.join(REPO_ROOT, "scripts", "gitenv.py")

# Windows' CreateProcess command line cannot exceed 32,767 characters. The
# fixture aims comfortably past it rather than at it: the point is to be over
# the limit on the machine that has one, not to discover its exact value.
ARGV_LIMIT = 32767
ARGV_TARGET = 48000

failures: list[str] = []


def check(label: str, got: object, want: object) -> None:
    if got == want:
        print(f"PASS  {label}")
    else:
        print(f"FAIL  {label}\n        got : {got!r}\n        want: {want!r}",
              file=sys.stderr)
        failures.append(label)


def git(cwd: str, *args: str, env: dict[str, str] | None = None
        ) -> subprocess.CompletedProcess:
    """Run git in `cwd`, never inheriting a repository binding.

    `check=False`: a refused push is the thing under test, so some of these are
    expected to exit non-zero.
    """
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env or gitenv.clean_env(),
        capture_output=True, text=True, check=False,
    )


def write(work: str, rel: str, text: str) -> None:
    path = os.path.join(work, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as fh:
        fh.write(text)


def push_output(work: str) -> tuple[str, str]:
    """(verdict, everything the push printed) for a push of main.

    `allowed`, `refused`, or `error:<...>`. Only gate 11's refusal counts as
    `refused`; anything else is an `error`, because a suite that accepted any
    refusal would pass on a fixture that trips a different gate and never
    reach the logic under test at all.
    """
    env = gitenv.clean_env()
    env["ALLOW_FMT_DRIFT"] = "1"
    proc = git(work, "push", "origin", "main", env=env)
    blob = proc.stdout + proc.stderr
    if proc.returncode == 0:
        return "allowed", blob
    # Matched on one line's worth: the refusal text wraps, so a whole-sentence
    # probe never matches and every correct refusal reads as an unrelated
    # error.
    if "links to a name that does not" in blob:
        return "refused", blob
    return "error:" + blob.strip().replace("\n", " | ")[:600], blob


def crate_source(link: str) -> str:
    """A tiny crate body whose one doc comment links to `link`."""
    return (
        "//! A crate.\n"
        "\n"
        f"/// See [`{link}`] for the rule.\n"
        "pub fn real_helper() {}\n"
    )


def build_fixture(tmp: str) -> str:
    """A repo with the real hook and the real checker installed."""
    os.makedirs(tmp, exist_ok=True)
    remote = os.path.join(tmp, "remote.git")
    work = os.path.join(tmp, "w")
    git(tmp, "init", "--quiet", "--bare", remote)
    git(tmp, "init", "--quiet", "-b", "main", work)
    git(work, "config", "user.name", "Real Person")
    git(work, "config", "user.email", "real@example.org.uk")
    # No signing key here; an inherited `commit.gpgsign=true` would fail every
    # commit below and surface as a gate result rather than as what it is.
    git(work, "config", "commit.gpgsign", "false")

    hooks = os.path.join(work, ".git", "hooks")
    os.makedirs(hooks, exist_ok=True)
    for src_path, dst_path in (
        (HOOK, os.path.join(hooks, "pre-push")),
        (LIB, os.path.join(work, "scripts", "run-checker.sh")),
        (CHECKER, os.path.join(work, "scripts", "check-doc-links.py")),
        (GITTREE, os.path.join(work, "scripts", "gittree.py")),
        (GITENV, os.path.join(work, "scripts", "gitenv.py")),
    ):
        os.makedirs(os.path.dirname(dst_path), exist_ok=True)
        with open(src_path, "r", encoding="utf-8", newline="") as src:
            body = src.read()
        with open(dst_path, "w", encoding="utf-8", newline="") as out:
            out.write(body)
    os.chmod(os.path.join(hooks, "pre-push"), 0o755)

    git(work, "remote", "add", "origin", remote)
    write(work, "a.txt", "one\n")
    git(work, "add", "--", "a.txt")
    git(work, "commit", "--quiet", "-m", "clean commit")
    git(work, "push", "--quiet", "origin", "main")
    return work


def add_crate(work: str, name: str, link: str) -> str:
    """One crate under `userspace/`. Returns the directory the gate will name."""
    write(work, f"userspace/{name}/Cargo.toml",
          f'[package]\nname = "{name}"\nversion = "0.1.0"\n')
    write(work, f"userspace/{name}/src/lib.rs", crate_source(link))
    return f"userspace/{name}/src"


# --------------------------------------------------------------------------
# The cases.
#
# Each builds its own fixture. Sharing one would let an earlier case's push
# move `origin/main`, and the pushed range the gate reads is defined against
# exactly that ref -- so a shared fixture would make the cases order-dependent
# in a way that is invisible until one is reordered.
# --------------------------------------------------------------------------


def case_live_link_passes(tmp: str) -> None:
    """Baseline: a link naming an item the crate defines is fine."""
    work = build_fixture(os.path.join(tmp, "live"))
    add_crate(work, "one-cli", "real_helper")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "add a crate")
    check("a live doc link pushes", push_output(work)[0], "allowed")


def case_dead_link_refused(tmp: str) -> None:
    """Baseline, mirrored: a link naming nothing is refused."""
    work = build_fixture(os.path.join(tmp, "dead"))
    add_crate(work, "two-cli", "renamed_away_helper")
    git(work, "add", "--all")
    git(work, "commit", "--quiet", "-m", "add a crate with a dead link")
    check("a dead doc link is refused", push_output(work)[0], "refused")


# Long enough that a few hundred crates clear the limit: a fixture of
# thousands of tiny crates would spend its time in `git add`, not in the gate.
_STEM = "a-deliberately-long-crate-directory-name"


def _crate_name(i: int) -> str:
    return f"{_STEM}-{i:04d}-cli"


def _wide_scope() -> tuple[int, int]:
    """`(how many crates, how many bytes of argv they would have needed)`.

    Computed before any of them is written, because the caller has to know the
    count in order to poison one in the *middle* -- and because a fixture whose
    size is only known after the fact cannot be asserted to be over the limit
    before it is used.
    """
    total = 0
    n = 0
    while total < ARGV_TARGET:
        total += len(f"userspace/{_crate_name(n)}/src") + 1
        n += 1
    return n, total


def _many_crates(work: str, count: int, dead_in: int | None) -> None:
    """Write `count` tiny crates; the one at `dead_in` gets a dead link."""
    for i in range(count):
        link = "renamed_away_helper" if i == dead_in else "real_helper"
        add_crate(work, _crate_name(i), link)


def case_many_crates_reach_a_verdict(tmp: str) -> None:
    """The regression. A push wider than the command line still gets judged.

    Both directions on the same fixture size, and the second is the one that
    matters: "the big clean push was allowed" is also what a gate that quietly
    skipped would produce, so it proves nothing by itself. The dead link is
    buried in the middle of the list rather than at either end, where a
    truncating implementation would be most likely to still find it.
    """
    count, size = _wide_scope()
    check("the fixture really exceeds the command-line limit",
          size > ARGV_LIMIT, True)

    clean = build_fixture(os.path.join(tmp, "manyclean"))
    _many_crates(clean, count, dead_in=None)
    git(clean, "add", "--all")
    git(clean, "commit", "--quiet", "-m", "many clean crates")
    verdict, blob = push_output(clean)
    check("a push wider than argv is allowed when it is clean", verdict,
          "allowed")
    check("...and did not die on the argument list",
          "Argument list too long" in blob, False)
    # `note_gate` prints one line per gate saying whether it ran. A skip here
    # would make the assertion above meaningless, so it is checked directly
    # rather than inferred.
    check("...and the gate actually ran rather than skipping",
          "doc-links" in blob and "doc-links: skipped" not in blob, True)

    dirty = build_fixture(os.path.join(tmp, "manydirty"))
    _many_crates(dirty, count, dead_in=count // 2)
    git(dirty, "add", "--all")
    git(dirty, "commit", "--quiet", "-m", "many crates, one dead link")
    check("one dead link among that many is still found",
          push_output(dirty)[0], "refused")


def case_empty_list_is_not_a_whole_tree_scan(tmp: str) -> None:
    """`--paths-from` naming nothing is an error, not a pass and not a sweep.

    Two wrong answers were available and both are silent. Treating it as "scan
    everything" widens the gate onto files the pusher cannot fix, which is how
    a gate gets bypassed by habit; treating it as "scan nothing" turns the gate
    off, so a bug that empties the list disables the check with no trace. The
    hook never sends an empty list -- it skips first -- so anything that gets
    here is a caller bug and should say so.
    """
    empty = os.path.join(tmp, "empty.txt")
    with open(empty, "w", encoding="utf-8", newline="") as fh:
        fh.write("\n  \n")
    proc = subprocess.run(
        [sys.executable, CHECKER, "--check", "--paths-from", empty],
        capture_output=True, text=True, check=False, env=gitenv.clean_env(),
    )
    check("an empty --paths-from exits non-zero", proc.returncode, 2)
    check("...and says which file was empty",
          "listed no paths" in proc.stdout + proc.stderr, True)

    missing = os.path.join(tmp, "no-such-list.txt")
    proc = subprocess.run(
        [sys.executable, CHECKER, "--check", "--paths-from", missing],
        capture_output=True, text=True, check=False, env=gitenv.clean_env(),
    )
    check("an unreadable --paths-from exits non-zero", proc.returncode, 2)


def case_paths_from_matches_arguments(tmp: str) -> None:
    """The two ways of naming a scope must reach the same verdict.

    The flag was added to route around a limit, not to change what is scanned;
    if the file-based path disagreed with the argument-based one on a small
    input, every verdict above would be about a different question.
    """
    work = build_fixture(os.path.join(tmp, "equiv"))
    rel = add_crate(work, "three-cli", "renamed_away_helper")
    listing = os.path.join(tmp, "equiv-list.txt")
    with open(listing, "w", encoding="utf-8", newline="") as fh:
        fh.write(rel + "\n")
    # The checker resolves its repo root from its own location, so it must be
    # the copy inside the fixture for these paths to mean anything.
    checker = os.path.join(work, "scripts", "check-doc-links.py")
    as_args = subprocess.run(
        [sys.executable, checker, "--check", rel], cwd=work,
        capture_output=True, text=True, check=False, env=gitenv.clean_env(),
    )
    as_file = subprocess.run(
        [sys.executable, checker, "--check", "--paths-from", listing], cwd=work,
        capture_output=True, text=True, check=False, env=gitenv.clean_env(),
    )
    check("both spellings agree on the exit code",
          as_args.returncode, as_file.returncode)
    check("both spellings print the same findings",
          as_args.stdout, as_file.stdout)
    check("and that verdict is the dead link, not a skip",
          as_args.returncode, 1)


def main() -> int:
    with tempfile.TemporaryDirectory() as tmp:
        for case in (case_live_link_passes,
                     case_dead_link_refused,
                     case_empty_list_is_not_a_whole_tree_scan,
                     case_paths_from_matches_arguments,
                     case_many_crates_reach_a_verdict):
            case(tmp)

    if failures:
        print(f"\n{len(failures)} doc-links gate test(s) failed:",
              file=sys.stderr)
        for name in failures:
            print(f"  - {name}", file=sys.stderr)
        return 1
    print("\nall pre-push doc-links gate tests passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
