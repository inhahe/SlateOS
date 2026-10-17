"""Sweep the mutation rows a range of commits touched, one harness at a time.

    python scripts/mutation-sweep.py <base> [<harness> ...]
    python scripts/mutation-sweep.py --list <base> [<harness> ...]
    python scripts/mutation-sweep.py --selftest

A batch that changes code can leave a row's mutant uncaught -- the test the
row names rewritten, or the code moved so that the mutant now breaks something
no test watches -- and `verify_mutations.py` cannot see that: it checks that
each anchor is there, not that its tests still catch the mutant.  Only running
the row does.  Running every row of every crate a batch touched costs hours;
the rows worth running are the ones whose anchors lie in what the batch wrote.
This finds them and runs them.

A row is *touched* when a line of its anchor -- stripped, and longer than six
characters, so that a lone `}` or `});` matches nothing -- is among the lines
`<base>..HEAD` added to a source its harness sweeps, or when the range added
the row or changed it: such a row has not been run since.  Each harness (a
`mutate.py` git knows of) with a touched row is run once, on those rows: one
warm-up and one baseline for the crate, then a build and a run per row.  The
harness waits for any QEMU guest before each build
(`mutation_harness.wait_while_a_guest_is_up`), so a sweep may run beside other
lanes' boot tests.

`<base>`: the commit the range starts after; it ends at `HEAD`.
`<harness>`: only these (`apps/email`, `apps/email/mutate.py`) -- to finish a
sweep that was cut off, name the harnesses it had not reached.
`--list`: print the touched rows, `<harness>\t<row>` a line, and run nothing.

Exit 0 when every touched row was caught by the tests it names, 1 when any
was not, 2 when the range or a harness could not be read or run.
"""

import subprocess
import sys
import types
from pathlib import Path, PurePosixPath

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))

import gitenv  # noqa: E402  (path set above)
import selftestflag  # noqa: E402
import verify_mutations  # noqa: E402

#: An anchor line or an added line this short is too common to tie a row to
#: what a batch wrote: `}`, `});`, `None,`.
SHORTEST = 7


def significant(line):
    """`line` stripped, or None when it is too short to say anything."""
    s = line.strip()
    return s if len(s) >= SHORTEST else None


def added_lines(diff):
    """The lines a `git diff --unified=0` adds, each `significant`."""
    out = set()
    for line in diff.splitlines():
        if line.startswith("+") and not line.startswith("+++"):
            s = significant(line[1:])
            if s:
                out.add(s)
    return out


def same(row):
    """`row` in a form two rows can be compared in, and kept in a set."""
    name, old, new, tests = row
    return name, old, new, tuple(tests)


def touched(rows, added, before=None):
    """The names of `rows` -- `(name, old, new, tests)` -- that one of whose
    anchor's lines is in `added`, or that is not among the rows `before` (a
    set of `same` rows: the harness's rows at the range's base, None when the
    range did not change the harness), in table order and each once."""
    names = []
    for row in rows:
        name, old = row[0], row[1]
        if name in names:
            continue
        new_row = before is not None and same(row) not in before
        if new_row or any(significant(l) in added for l in old.split("\n")):
            names.append(name)
    return names


def git(*args):
    return subprocess.run(
        ["git", "-C", str(REPO), *args],
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        check=True, env=gitenv.clean_env(),
    ).stdout


def repo_relative(path):
    """`path` as git names it, or None outside the repository."""
    try:
        return PurePosixPath(Path(path).resolve().relative_to(REPO).as_posix())
    except ValueError:
        return None


def harness_rows(harness):
    """Every row of the harness at `harness` (repo-relative), and the
    repo-relative sources its tables sweep."""
    module = verify_mutations.load(REPO / harness)
    rows = [r for _name, table in verify_mutations.tables(module) for r in table]
    texts = {p: None for p in verify_mutations.sources(module)}
    files = set(verify_mutations.sources(module)) | set(verify_mutations.table_files(module, texts).values())
    return rows, {r for r in map(repo_relative, files) if r is not None}


def rows_at(base, harness):
    """The rows of the harness at `harness` as it was at commit `base`, as a
    set of `same` rows: empty when it did not exist.

    The old source is run under the harness's own path, so the paths it
    builds from `__file__` and its imports resolve as the harness's do; its
    sweep is behind `__main__`, which this does not set."""
    try:
        source = git("show", f"{base}:{harness}")
    except subprocess.CalledProcessError:
        return set()
    module = types.ModuleType("mutation_sweep_base")
    module.__file__ = str(REPO / harness)
    saved = sys.argv
    sys.argv = [module.__file__]
    try:
        exec(compile(source, module.__file__, "exec"), module.__dict__)  # noqa: S102 (our own tracked file)
    except Exception as e:  # noqa: BLE001 -- any failure means the same: the old rows are unknown
        # Every row then counts as new, which sweeps more than needed and
        # never less.
        print(f"mutation-sweep: {harness} at {base} does not load ({e}); every row of it is swept",
              file=sys.stderr)
        return set()
    finally:
        sys.argv = saved
    return {same(r) for _name, table in verify_mutations.tables(module) for r in table}


def as_harness(arg):
    p = PurePosixPath(Path(arg).as_posix())
    return p if p.name == "mutate.py" else p / "mutate.py"


def plan(base, only=()):
    """`[(harness, [row names])]`: every harness with a row `base..HEAD`
    touched, those in `only` alone when it names any."""
    changed = {PurePosixPath(p) for p in git("diff", "--name-only", f"{base}..HEAD").split("\n") if p}
    harnesses = sorted(PurePosixPath(p) for p in git("ls-files", "*mutate.py").split("\n") if p)
    wanted = {as_harness(a) for a in only}
    missing = wanted - set(harnesses)
    if missing:
        raise SystemExit(f"mutation-sweep: no harness at {', '.join(map(str, sorted(missing)))}")
    out = []
    for harness in harnesses:
        if wanted and harness not in wanted:
            continue
        rows, sources = harness_rows(harness)
        swept = sorted(str(s) for s in sources & changed)
        if not swept and harness not in changed:
            continue
        added = added_lines(git("diff", "--unified=0", f"{base}..HEAD", "--", *swept)) if swept else set()
        names = touched(rows, added, rows_at(base, harness) if harness in changed else None)
        if names:
            out.append((harness, names))
    return out


def run(base, only, list_only):
    try:
        git("rev-parse", "--verify", "--quiet", f"{base}^{{commit}}")
    except subprocess.CalledProcessError:
        print(f"mutation-sweep: {base!r} names no commit")
        return 2
    todo = plan(base, only)
    if list_only:
        for harness, names in todo:
            for name in names:
                print(f"{harness}\t{name}")
        return 0
    print(f"mutation-sweep: {sum(len(n) for _h, n in todo)} row(s) in {len(todo)} harness(es) "
          f"touched by {base}..HEAD", flush=True)
    worst, verdicts = 0, []
    for harness, names in todo:
        print(f"\n==== {harness}: {len(names)} row(s) ====", flush=True)
        code = subprocess.run([sys.executable, "-u", str(REPO / harness), *names], cwd=REPO).returncode
        verdicts.append((harness, code))
        # A harness exits 1 for a row not caught as named and 2 for a table
        # or a crate it could not use; anything else is a harness that died.
        worst = max(worst, code if code in (0, 1) else 2)
    print("\n==== mutation-sweep ====")
    for harness, code in verdicts:
        print(f"  {'ok  ' if code == 0 else 'FAIL'}  {harness} (exit {code})")
    return worst


def _self_test():
    added = added_lines(
        "diff --git a/x.rs b/x.rs\n+++ b/x.rs\n@@ -1 +1,3 @@\n"
        "+        if edit.changed {\r\n+    }\n-        self.gone();\n+        self.search_changed();\n"
    )
    cases = [
        ("an added line is read, stripped and without its CR",
         added, {"if edit.changed {", "self.search_changed();"}),
        ("a removed line, a short one and the file header are not",
         {"self.gone();", "}", "++ b/x.rs"} & added, set()),
    ]
    rows = [
        ("follows", "        if edit.changed {\n            self.search_changed();\n        }\n", "", ["t"]),
        ("closes", "    }\n", "", ["t"]),
        ("elsewhere", "        self.untouched_by_the_range();\n", "", ["t"]),
        ("follows", "        self.search_changed();\n", "", ["t"]),
    ]
    cases += [
        ("a row is touched by one added line of its anchor, once by name",
         touched(rows, added), ["follows"]),
        ("a short anchor line ties a row to nothing", touched(rows[1:2], {"}"}), []),
        ("a row the range added or changed is touched, one it left alone not",
         touched(rows[1:3], set(), {same(rows[1]), same(("elsewhere", "", "", ["t"]))}), ["elsewhere"]),
        ("a harness is named by its directory or its file",
         [as_harness("apps/email"), as_harness("apps/email/mutate.py")],
         [PurePosixPath("apps/email/mutate.py")] * 2),
    ]
    bad = 0
    for label, got, want in cases:
        ok = got == want
        bad += not ok
        print(f"  {'ok  ' if ok else 'FAIL'}  {label}" + ("" if ok else f": got {got!r}, want {want!r}"))
    print("mutation-sweep self-test: " + ("all passed" if not bad else f"{bad} FAILED"))
    return 1 if bad else 0


def main(argv):
    if selftestflag.wants_selftest(argv):
        return _self_test()
    unknown = selftestflag.unknown_options(argv, ("--list",))
    if unknown:
        print(f"mutation-sweep: unrecognised option {unknown[0]!r}")
        return 2
    list_only = "--list" in argv
    args = [a for a in argv if a != "--list"]
    if not args:
        print(__doc__)
        return 2
    return run(args[0], args[1:], list_only)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
