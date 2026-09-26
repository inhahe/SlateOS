"""Tests for the pre-push hook's `touches` helper (scripts/hooks/pre-push).

Run: `python scripts/test-pre-push-touches.py` (0 = pass, 1 = fail). No pytest
dependency, matching the other suites in this directory.

What this tests, and why it is worth testing
--------------------------------------------

`touches <paths>` decides whether a path-scoped gate runs at all, so a wrong
"no" publishes a push unjudged and says only "skipped" -- the silent failure
the hook's comments spend most of their length on. It used to be answered by
asking git on every call (`git rev-list $pushed_shas --not --remotes -- <paths>`).
Since 2026-09-25 a push that publishes no merge is answered from one list of
the paths its commits change, matched in the shell, because forty-odd forks per
push were most of the cost of every fixture push through the hook.

The hook's comment argues that the list gives git's answer exactly. This suite
is what holds it to that: for each kind of push below it runs the hook's own
helper, lifted out of the hook verbatim, and asks git the old question
independently, and requires the same answer every time. The kinds are the ones
where a path list and a revision walk could plausibly part company:

* a root commit, whose paths are all "changed" only if `--root` is honoured;
* a rename, which a list built with rename detection would report under the
  new name only, leaving the old directory's gates asleep;
* a deletion, a mode-only change and an empty commit;
* two refs in one push, and a change that a later commit in the same push
  reverts;
* a merge that is already published, which must not force the slow path, and
  one that is not, which must;
* names git has to quote, which must force the slow path;
* nothing published at all.

The path spellings are the ones git itself treats differently -- `dir/` against
`dir`, `name` against `name-longer`, a wildcard against a leading directory,
case -- and the ones the list must refuse to answer (`?`, `[...]`, pathspec
magic, a `*` that is not a leading one), which must fall through to git.

It also reads every `touches` call in the hook and fails if one is spelled in a
way the list cannot answer. Such a gate would still be judged correctly -- the
call goes to git -- but every push would pay for it again, silently. The
failure says how to spell it instead.

Why it is built the way it is
-----------------------------

This suite runs in every boot, so it must not become the cost it removes. The
whole history is written by one `git fast-import`, every scenario runs in one
shell, and git's side of each comparison is asked from Python -- a plain
CreateProcess, not an MSYS fork -- a few at a time. The first version, one
repository and one shell per scenario, took twelve minutes on a loaded host.

Names that no Windows filesystem accepts (a double quote, a tab, a newline, a
backslash) are committed with `core.protectNTFS=false` for that one process:
Git for Windows refuses them otherwise -- and `update-index --index-info`,
which this suite first used, refuses them by printing "Ignoring path" and
exiting 0, so those cases had silently tested commits without the names in
them. Each odd-name commit's tree is now read back and compared.
"""

from __future__ import annotations

import concurrent.futures
import os
import re
import shlex
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field

HERE = os.path.dirname(os.path.abspath(__file__))
HOOK = os.path.join(HERE, "hooks", "pre-push")
sys.path.insert(0, HERE)

import gitenv  # noqa: E402
import msysbash  # noqa: E402

_FAILURES: list[str] = []

for _stream in (sys.stdout, sys.stderr):
    try:
        _stream.reconfigure(errors="replace")
    except (AttributeError, ValueError):
        pass


def check(label: str, got, want) -> bool:
    if got == want:
        print(f"PASS  {label}")
        return True
    print(f"FAIL  {label}: got {got!r}, want {want!r}")
    _FAILURES.append(label)
    return False


def git(repo: str, *args: str, stdin: bytes | None = None) -> bytes:
    proc = subprocess.run(["git", *args], cwd=repo, env=gitenv.clean_env(),
                          input=stdin, capture_output=True, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed in {repo}:\n"
                           + proc.stderr.decode("utf-8", "replace"))
    return proc.stdout


# --------------------------------------------------------------------------
# The helper under test, lifted out of the hook verbatim.
# --------------------------------------------------------------------------

def touches_block(text: str) -> str:
    """From `touches_mode=""` to the end of `touches()`, exactly as the hook has
    it. Lifting the text rather than restating it is the point: a copy here
    would go on passing after the hook changed."""
    start = text.find('\ntouches_mode=""\n')
    if start < 0:
        raise RuntimeError('the hook no longer initialises `touches_mode=""`; '
                           "this suite cannot find the helper to test")
    tail = text[start + 1:]
    end = re.search(r"^touches\(\)\s*\{.*?^\}\n", tail, re.MULTILINE | re.DOTALL)
    if end is None:
        raise RuntimeError("the hook has no `touches() { ... }` after "
                           '`touches_mode=""`')
    return tail[:end.end()]


def hook_scopes(text: str) -> list[list[str]]:
    """The path list of every `touches` call in the hook."""
    logical: list[str] = []
    pending = ""
    for line in text.splitlines():
        if not pending and line.lstrip().startswith("#"):
            continue
        if line.endswith("\\"):
            pending += line[:-1] + " "
            continue
        logical.append(pending + line)
        pending = ""
    scopes = []
    for line in logical:
        m = re.match(r"^\s*(?:if\s+)?touches\s+(.+)$", line)
        if m is None:
            continue
        args = re.split(r"\|\||&&|;", m.group(1), maxsplit=1)[0]
        scopes.append(shlex.split(args))
    return scopes


# --------------------------------------------------------------------------
# The fixture history: every commit the cases need, in one fast-import.
# --------------------------------------------------------------------------

X, Y = "x\n", "y\n"


def entry(content: str = X, mode: str = "100644") -> tuple[str, str]:
    return (mode, content)


def _quoted(path: str) -> bytes:
    """A fast-import path, C-quoted so that any byte survives."""
    out = bytearray(b'"')
    for byte in path.encode("utf-8"):
        if byte == 0x22:
            out += b'\\"'
        elif byte == 0x5C:
            out += b"\\\\"
        elif byte == 0x0A:
            out += b"\\n"
        elif byte == 0x09:
            out += b"\\t"
        elif byte < 0x20 or byte == 0x7F:
            out += b"\\%03o" % byte
        else:
            out.append(byte)
    return bytes(out + b'"')


class History:
    def __init__(self, root: str) -> None:
        self.root = root
        os.makedirs(root)
        git(root, "init", "--quiet", "-b", "main", root)
        # The hook passes some flags only to be proof against a user's config.
        # Set each such config to the value the flag is there to defeat, so
        # that dropping the flag fails here instead of on somebody's machine:
        # --root against log.showRoot=false, --no-renames against
        # diff.renames=copies, and core.quotePath=false against
        # core.quotePath=true. color.ui=always has no flag against it: git
        # does not colour `--name-only` names even then, which is why the hook
        # passes no --no-color (a mutation that dropped one went uncaught,
        # 2026-09-26). It stays set so that a git which starts to fails here.
        for key, value in (("log.showRoot", "false"), ("diff.renames", "copies"),
                           ("color.ui", "always"), ("core.quotePath", "true")):
            git(root, "config", key, value)
        self._stream = bytearray()
        self._marks: dict[str, int] = {}
        self._blobs: dict[str, int] = {}
        self._trees: dict[str, dict[str, tuple[str, str]]] = {}
        self.sha: dict[str, str] = {}

    def _mark(self) -> int:
        return len(self._marks) + len(self._blobs) + 1

    def _blob(self, content: str) -> int:
        if content not in self._blobs:
            mark = self._mark()
            data = content.encode("utf-8")
            self._stream += b"blob\nmark :%d\ndata %d\n" % (mark, len(data))
            self._stream += data + b"\n"
            self._blobs[content] = mark
        return self._blobs[content]

    def commit(self, name: str, files: dict[str, tuple[str, str]],
               parents: tuple[str, ...] = ()) -> None:
        """Commit `name`, whose tree is exactly `files` ({path: (mode, content)})."""
        blobs = {path: self._blob(content) for path, (_, content) in files.items()}
        mark = self._mark()
        self._marks[name] = mark
        self._trees[name] = dict(files)
        msg = name.encode()
        self._stream += (b"commit refs/fixture/%s\nmark :%d\n"
                         b"committer Real Person <real@example.org.uk> "
                         b"1700000000 +0000\ndata %d\n%s\n"
                         % (name.encode(), mark, len(msg), msg))
        for i, parent in enumerate(parents):
            verb = b"from" if i == 0 else b"merge"
            self._stream += b"%s :%d\n" % (verb, self._marks[parent])
        self._stream += b"deleteall\n"
        for path, (mode, _) in files.items():
            self._stream += b"M %s :%d %s\n" % (mode.encode(), blobs[path],
                                                _quoted(path))
        self._stream += b"\n"

    def write(self, verify: tuple[str, ...]) -> None:
        """Run the import; read every commit's sha; read back the trees of
        `verify` -- the commits whose names a host could refuse."""
        marks = os.path.join(self.root, ".git", "fixture-marks")
        git(self.root, "-c", "core.protectNTFS=false", "fast-import", "--quiet",
            f"--export-marks={marks}", stdin=bytes(self._stream))
        by_mark = {}
        with open(marks, encoding="ascii") as fh:
            for line in fh:
                mark, sha = line.split()
                by_mark[int(mark[1:])] = sha
        self.sha = {name: by_mark[mark] for name, mark in self._marks.items()}
        for name in verify:
            listed = git(self.root, "ls-tree", "-r", "-z", "--name-only",
                         self.sha[name]).decode("utf-8").split("\0")
            check(f"fixture: commit `{name}` holds exactly the paths asked for",
                  sorted(p for p in listed if p), sorted(self._trees[name]))

    def publish(self, remotes: list[tuple[str, str]]) -> None:
        """Give each remote its main: one `update-ref --stdin` for them all."""
        stdin = "".join(f"update refs/remotes/{remote}/main {self.sha[name]}\n"
                        for remote, name in remotes)
        git(self.root, "update-ref", "--stdin", stdin=stdin.encode())


BASE = {
    "a.md/b.txt": entry(),
    "docs/x.md": entry(),
    "userspace/coreutils": entry(),
    "userspace/coreutils-extra/x": entry(),
    "Cargo.toml": entry(),
    "sub/Cargo.toml": entry(),
    "x.RS": entry(),
    "we ird/f.rs": entry(),
    "\u00fcn\u00ef/\u00e7.rs": entry(),
    "requests/.deletions-allowed": entry(),
    "gui/app.rs": entry(),
    "scripts/check-text-ink.py": entry(),
    "kernel/keep.txt": entry(),
}

ODD_NAMES = {"a double quote": 'q"uote/x.txt', "a tab": "ta\tb/x.txt",
             "a newline": "new\nline/x.txt", "a backslash": "back\\slash/x.txt"}


def build(h: History) -> None:
    h.commit("base", BASE)
    tree = dict(BASE)
    tree["docs/x.md"] = entry(Y)
    h.commit("modify", tree, ("base",))
    del tree["x.RS"]
    h.commit("delete", tree, ("modify",))
    tree["apps/app.rs"] = tree.pop("gui/app.rs")
    h.commit("rename", tree, ("delete",))
    tree["scripts/check-text-ink.py"] = entry(X, "100755")
    h.commit("chmod", tree, ("rename",))
    h.commit("empty", tree, ("chmod",))
    h.commit("ref-a", {**BASE, "posix/src/lib.rs": entry()}, ("base",))
    h.commit("ref-b", {**BASE, "gui/app.rs": entry(Y)}, ("base",))
    h.commit("add", {**BASE, "kernel/new.rs": entry()}, ("base",))
    h.commit("revert", BASE, ("add",))
    h.commit("side", {**BASE, "gui/app.rs": entry(Y)}, ("base",))
    h.commit("main", {**BASE, "docs/x.md": entry(Y)}, ("base",))
    merged = {**BASE, "gui/app.rs": entry(Y), "docs/x.md": entry(Y)}
    h.commit("merge", merged, ("main", "side"))
    h.commit("after", {**merged, "we ird/f.rs": entry(Y)}, ("merge",))
    h.commit("leading", {**BASE, "a.md/b.txt": entry(Y)}, ("base",))
    for i, name in enumerate(ODD_NAMES.values()):
        h.commit(f"odd{i}", {**BASE, name: entry(), "gui/app.rs": entry(Y)},
                 ("base",))


# --------------------------------------------------------------------------
# Scenarios: a push (the shas) against a remote (what it already has).
# --------------------------------------------------------------------------

# Every spelling the list answers, and how git reads each: `dir/` is only
# below dir, `name` is name or below it, `*suffix` crosses `/` but never
# matches a leading directory, and all of it is case-sensitive.
ANSWERABLE = [
    ["*.md"], ["*.rs"], ["*.RS"], ["*.toml"], ["*.txt"], ["*.py"],
    ["a.md"], ["a.md/"], ["docs"], ["docs/"], ["doc"],
    ["userspace/coreutils"], ["userspace/coreutils/"], ["userspace/"],
    ["Cargo.toml"], ["we ird/"], ["\u00fcn\u00ef/"],
    ["gui/", "apps/"], ["gui/"], ["apps/"], ["kernel/"], ["kernel/new.rs"],
    ["posix/src/"], ["scripts/"], ["requests/.deletions-allowed"],
    ["nothing/", "none.txt"],
]

# Spellings the list must refuse, so that git answers them.
UNANSWERABLE = [
    ["d?cs/"], ["[d]ocs/x.md"], [":(glob)**/*.md"], ["*/Cargo.toml"],
    ["./docs"], ["*"], ["docs/*.md"], ["**.md"], ["docs//x.md"],
    ["nothing/", "d?cs/x.md"],
]


@dataclass
class Scenario:
    label: str
    pushed: tuple[str, ...]
    published: str | None
    want_mode: str
    # What git must answer, spelling by spelling. This is what makes a case
    # non-vacuous: two answers that agreed by both being "no" pass anything.
    truths: dict[tuple[str, ...], bool]
    extra: list[list[str]] = field(default_factory=list)
    # Asked of `touches_from_list` alone; the case says what it must return.
    listed: list[list[str]] = field(default_factory=list)
    remote: str = ""
    # Filled in by the run.
    mode: str = ""
    hook: dict[tuple[str, ...], bool] = field(default_factory=dict)
    gits: dict[tuple[str, ...], bool] = field(default_factory=dict)
    list_rc: dict[tuple[str, ...], int] = field(default_factory=dict)

    def asked(self) -> list[tuple[str, ...]]:
        seen = list(self.truths)
        seen += [tuple(x) for x in self.extra if tuple(x) not in self.truths]
        return seen


def scenarios(scopes: list[list[str]]) -> list[Scenario]:
    out = [
        # Nothing published yet: every path in the tree is changed, so the
        # full table of spellings is asked here, where each has something to
        # find.
        Scenario("root commit", ("base",), None, "list", {
            ("*.md",): True, ("*.RS",): True, ("*.rs",): True,
            ("*.txt",): True, ("a.md",): True, ("a.md/",): True,
            ("docs",): True, ("doc",): False,
            ("userspace/coreutils",): True, ("userspace/coreutils/",): False,
            ("we ird/",): True, ("\u00fcn\u00ef/",): True, ("apps/",): False,
            ("requests/.deletions-allowed",): True,
            ("nothing/", "none.txt"): False,
            # Left to git, so its answer is git's by construction -- asked
            # anyway, so that a wrong classification shows as a disagreement
            # too. `d?cs/` finds nothing: a wildcard is matched against whole
            # paths and, as `*.md` against `a.md/b.txt` shows, never a
            # leading directory.
            ("d?cs/",): False, ("d?cs/x.md",): True, ("*/Cargo.toml",): True,
            ("nothing/", "d?cs/x.md"): True,
        }, extra=ANSWERABLE, listed=UNANSWERABLE + scopes),
        # gui/ is the rename's *old* side: only a list built with --no-renames
        # has it, so this is the case that fails if that flag is ever dropped.
        Scenario("every kind of change", ("empty",), "base", "list", {
            ("gui/",): True, ("apps/",): True, ("*.RS",): True,
            ("docs/",): True, ("*.md",): True, ("scripts/",): True,
            ("kernel/",): False, ("Cargo.toml",): False,
            ("userspace/",): False,
        }),
        Scenario("a mode change alone", ("chmod",), "rename", "list", {
            ("scripts/",): True, ("*.py",): True, ("docs/",): False,
            ("gui/",): False, ("*.md",): False,
        }),
        Scenario("an empty commit alone", ("empty",), "chmod", "list", {
            ("*.md",): False, ("scripts/",): False, ("*.py",): False,
            ("gui/", "apps/"): False,
        }),
        # `$pushed_shas` holds one sha per ref; the list must cover them all.
        # `src/`, `src` and `lib.rs` all occur in `posix/src/lib.rs`, but not
        # at its start: a scope is anchored at the top of the tree, so each is
        # no -- one per way of matching (below a `dir/`, a name, and below a
        # name).
        Scenario("two refs in one push", ("ref-a", "ref-b"), "base", "list", {
            ("posix/src/",): True, ("gui/",): True, ("docs/",): False,
            ("*.rs",): True, ("*.md",): False, ("src/",): False,
            ("src",): False, ("lib.rs",): False,
        }),
        # The push's net effect on kernel/ is nothing, but it carries two
        # commits that change it -- and rev-list counts commits, not effect.
        Scenario("a change and its revert", ("revert",), "base", "list", {
            ("kernel/",): True, ("kernel/new.rs",): True, ("*.rs",): True,
            ("docs/",): False,
        }),
        # Merge main, then push on top: the ordinary lane push. The merge is
        # already on the remote, so it is not in the push and the list answers.
        Scenario("a published merge", ("after",), "merge", "list", {
            ("we ird/",): True, ("gui/",): False, ("docs/",): False,
            ("*.rs",): True, ("*.md",): False,
        }),
        # History simplification can act, so the list must not answer.
        Scenario("an unpublished merge", ("merge",), "base", "git", {
            ("gui/",): True, ("docs/",): True, ("kernel/",): False,
        }),
        # The only changed path holding `.md` holds it as a directory, which
        # `*.md` must not match. In the root commit `docs/x.md` answers `*.md`
        # first, so a suffix matched anywhere in the path would pass there;
        # it cannot pass here (a mutation that did so went uncaught until
        # this scenario, 2026-09-26).
        Scenario("a wildcard against a leading directory", ("leading",),
                 "base", "list", {
            ("*.md",): False, ("*.txt",): True, ("a.md",): True,
            ("a.md/",): True, ("docs/",): False,
        }),
        Scenario("no sha pushed", (), "base", "none", {
            ("*.md",): False, ("docs/",): False,
        }),
        # Something is pushed, and nothing published.
        Scenario("a sha the remote has", ("base",), "base", "list", {
            ("*.md",): False, ("docs/",): False, ("kernel/",): False,
        }),
    ]
    for i, label in enumerate(ODD_NAMES):
        out.append(Scenario(f"a name with {label}", (f"odd{i}",), "base",
                            "git", {("gui/",): True, ("docs/",): False,
                                    ("*.txt",): True}))
    for i, sc in enumerate(out):
        sc.remote = f"r{i:02d}"
    return out


def run_hook_side(h: History, block: str, all_: list[Scenario]) -> None:
    """Each scenario in a shell of its own, through the hook's own helper.

    A few shells at a time rather than one shell for everything: what a
    scenario costs here is almost entirely MSYS forks -- `touches_prepare`'s
    two command substitutions, and every answer in a scenario git answers --
    and on 2026-09-26, six lanes building, a fork cost about ten seconds. One
    shell for all fifteen scenarios took 637 s; the same questions asked of
    git from Python took 32.
    """
    q = shlex.quote
    env = gitenv.clean_env()
    env["LC_ALL"] = "C.UTF-8"

    def one(index: int, sc: Scenario) -> None:
        shas = "".join(" " + h.sha[name] for name in sc.pushed)
        lines = ["set -u", block,
                 f"remote_name={q(sc.remote)}", f"pushed_shas={q(shas)}",
                 "touches_prepare",
                 'echo "MODE $touches_mode"',
                 'ans() { touches "$@"; echo "ANS $?"; }',
                 'lst() { touches_from_list "$@"; echo "LST $?"; }']
        lines += ["ans " + " ".join(q(s) for s in specs) for specs in sc.asked()]
        lines += ["lst " + " ".join(q(s) for s in specs) for specs in sc.listed]
        script = os.path.join(h.root, ".git", f"touches-harness-{index:02d}.sh")
        with open(script, "w", encoding="utf-8", newline="\n") as fh:
            fh.write("\n".join(lines) + "\n")
        # `--posix`: the hook is `#!/bin/sh`, and Git for Windows runs it with
        # its sh, which is this bash in POSIX mode.
        proc = subprocess.run([msysbash.bash(), "--posix", script], cwd=h.root,
                              env=env, capture_output=True, check=False)
        out = proc.stdout.decode("utf-8", "replace").splitlines()
        want = 1 + len(sc.asked()) + len(sc.listed)
        if proc.returncode != 0 or len(out) != want:
            raise RuntimeError(
                f"{sc.label}: the harness printed {len(out)} of {want} lines:\n"
                + proc.stdout.decode("utf-8", "replace")[-2000:]
                + proc.stderr.decode("utf-8", "replace")[-2000:])
        sc.mode = out.pop(0).split()[1]
        for specs in sc.asked():
            sc.hook[specs] = out.pop(0).split()[1] == "0"
        for specs in sc.listed:
            sc.list_rc[tuple(specs)] = int(out.pop(0).split()[1])

    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        # list(): so an exception in any scenario is raised here, not lost.
        list(pool.map(lambda job: one(*job), enumerate(all_)))


def run_git_side(h: History, all_: list[Scenario]) -> None:
    """git's own answer to each question, asked the way the hook used to ask
    it -- and independently of the hook's text, so a change there cannot
    change this side too."""
    def ask(sc: Scenario, specs: tuple[str, ...]) -> tuple[Scenario, tuple, bool]:
        shas = [h.sha[name] for name in sc.pushed]
        out = git(h.root, "rev-list", *shas, "--not", f"--remotes={sc.remote}",
                  "--", *specs)
        return sc, specs, bool(out.strip())

    jobs = [(sc, specs) for sc in all_ for specs in sc.asked()]
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        for sc, specs, answer in pool.map(lambda job: ask(*job), jobs):
            sc.gits[specs] = answer


def judge(all_: list[Scenario], scopes: list[list[str]]) -> None:
    for sc in all_:
        check(f"{sc.label}: answered by {sc.want_mode}", sc.mode, sc.want_mode)
        for specs in sc.asked():
            check(f"{sc.label}: `{' '.join(specs)}` -> the hook agrees with git",
                  sc.hook[specs], sc.gits[specs])
        for specs, want in sc.truths.items():
            check(f"{sc.label}: `{' '.join(specs)}` -> "
                  f"{'touched' if want else 'untouched'}",
                  sc.gits[specs], want)
    root = all_[0]
    for specs in UNANSWERABLE:
        check(f"root commit: `{' '.join(specs)}` is left to git",
              root.list_rc[tuple(specs)], 2)
    empty = next(sc for sc in all_ if sc.label == "an empty commit alone")
    check("an empty commit alone: touches nothing at all",
          any(empty.hook.values()), False)
    for specs in scopes:
        rc = root.list_rc[tuple(specs)]
        if rc == 2:
            print(f"      `touches {' '.join(specs)}` goes to git on every "
                  "push. Spell each path as `dir/`, `name` or `*suffix` (no "
                  "other wildcard, no pathspec magic), or teach "
                  "touches_from_list the new form and add it to ANSWERABLE "
                  "here.")
        check(f"hook scope `{' '.join(specs)}` is answered by the list",
              rc in (0, 1), True)


def main() -> int:
    gitenv.scrub_environ()
    with open(HOOK, encoding="utf-8", newline="") as fh:
        text = fh.read()
    block = touches_block(text)
    scopes = hook_scopes(text)
    # A parser that finds nothing passes everything; the hook has had ~40.
    if len(scopes) < 30:
        print(f"FATAL: found {len(scopes)} `touches` calls in the hook; it has "
              "had about forty. The parser is broken, not the hook.")
        return 1
    all_ = scenarios(scopes)
    # A table that lost its rows would pass; assert a floor, as the sibling
    # suites do.
    if len(all_) < 15:
        print(f"FATAL: only {len(all_)} scenarios; the suite has at least 15. "
              "The table is broken, not the code.")
        return 1
    with tempfile.TemporaryDirectory() as tmp:
        h = History(os.path.join(tmp, "repo"))
        build(h)
        h.write(verify=("base", "rename", "chmod",
                        *(f"odd{i}" for i in range(len(ODD_NAMES)))))
        h.publish([(sc.remote, sc.published) for sc in all_
                   if sc.published is not None])
        run_hook_side(h, block, all_)
        run_git_side(h, all_)
    judge(all_, scopes)
    print()
    if _FAILURES:
        print(f"{len(_FAILURES)} FAILED: {', '.join(_FAILURES)}")
        return 1
    print(f"all {len(all_)} touches scenarios passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
