#!/usr/bin/env python3
"""Keep the integration tree on `main` from a scheduled task, with no lane running.

The operator reads the project in the integration tree (`os`), and lanes bring
it up to `main` only when one of them runs `check-lane-signals.py`. With no
lane running, or with the tree unable to fast-forward (a commit of its own in
it, a local edit in the way), it silently stops showing what is published.
This runs every minute from a Windows scheduled task, independent of any
session, and each pass:

  1. asks GitHub where `main` is (`git ls-remote`, which changes nothing) and,
     only when that is not the local `origin/main`, fetches `main` alone;
  2. fast-forwards the tree with `check-lane-signals.py`'s
     `sync_integration_tree` -- `merge --ff-only` and nothing else, so the
     operator's own edits there are never touched;
  3. tells the operator, with a Windows notification, when the tree cannot
     follow `main`: at once when git refuses the fast-forward (a commit of the
     tree's own, a local edit in the way, another branch), and after an hour
     when anything else keeps a pass from finishing -- GitHub unreachable, a
     fetch refused, a rebase or merge left part-way there, a stale
     `index.lock`, a pass that crashes. Each reason is shown once, and again
     every six hours while it lasts.

Nothing is changed while an operation is under way in the tree: that is
someone working there, and only its lasting an hour is reported. What a pass
changes, and each new trouble, goes into `slateos-integration-sync.log` in the
git directory; the reason last shown, and the trouble under way, are kept
beside it in `slateos-integration-sync.json`.

usage:
    python scripts/integration-tree-sync.py              one pass (the task's job)
    python scripts/integration-tree-sync.py --install    register the scheduled task
    python scripts/integration-tree-sync.py --uninstall  remove it
    python scripts/integration-tree-sync.py --self-test

`--install` registers the task to run this file's copy in the integration
tree, which every fast-forward keeps current, under `conhost.exe --headless`
so that no console window opens each minute. Run it again after the tree or
Python moves.
"""

from __future__ import annotations

import base64
import datetime as _dt
import json
import os
import re
import subprocess
import sys
import traceback
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import selftestflag  # noqa: E402
import srcload  # noqa: E402

TASK_NAME = "SlateOS integration tree sync"
STATE_NAME = "slateos-integration-sync.json"
LOG_NAME = "slateos-integration-sync.log"
#: How often a reason that has not changed is shown again.
REMIND = _dt.timedelta(hours=6)
#: How long a pass may keep failing to finish before the operator is told.
TROUBLE_AFTER = _dt.timedelta(hours=1)
#: Seconds a network git call may take before the pass gives up on it.
NET_TIMEOUT = 60
#: The log is cut back to its last KEEP lines once it reaches twice that.
KEEP = 1000
#: What the git directory holds while someone is part-way through an operation.
IN_PROGRESS = ("rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD",
               "REVERT_HEAD", "sequencer", "BISECT_LOG")
#: The outcomes after which the tree is on `main`.
FOLLOWING = ("current", "fetched", "done")


def signals():
    """`check-lane-signals.py` as a module: the fast-forward and the git
    helpers live there once, for lanes and for this task."""
    return srcload.load(str(HERE / "check-lane-signals.py"), "check_lane_signals")


def in_progress(git_dir: Path) -> str | None:
    """The operation someone is part-way through in the tree, or None."""
    for name in IN_PROGRESS:
        if (git_dir / name).exists():
            return name
    return None


def reason_key(line: str) -> str:
    """A block's reason without the count of commits behind, which changes
    each time `main` moves while the reason itself does not."""
    return re.sub(r"\b\d+ commit\(s\) behind\b", "behind", line)


def parse_time(value) -> _dt.datetime | None:
    """A stored timestamp, or None for anything that is not one."""
    if not isinstance(value, str):
        return None
    try:
        t = _dt.datetime.fromisoformat(value)
    except ValueError:
        return None
    return t if t.tzinfo is not None else None


def should_notify(state: dict, key: str, now: _dt.datetime) -> bool:
    """Whether to show `key` now: a reason not shown before, or one last
    shown REMIND or longer ago."""
    if state.get("key") != key:
        return True
    last = parse_time(state.get("notified_at"))
    return last is None or now - last >= REMIND


def read_state(path: Path) -> dict:
    try:
        data = json.loads(path.read_bytes().decode("utf-8"))
    except (OSError, ValueError):
        return {}
    return data if isinstance(data, dict) else {}


def write_state(path: Path, state: dict) -> None:
    path.write_bytes(json.dumps(state, indent=1).encode("utf-8"))


def log(path: Path, now: _dt.datetime, line: str) -> None:
    """Append `line` stamped with `now`, keeping the file to its last lines."""
    entry = f"{now.isoformat(timespec='seconds')} {line}\n".encode("utf-8")
    try:
        old = path.read_bytes().splitlines(keepends=True)
    except OSError:
        old = []
    old.append(entry)
    if len(old) >= 2 * KEEP:
        old = old[-KEEP:]
    path.write_bytes(b"".join(old))


def net_git(cls, args: list[str], tree: Path) -> subprocess.CompletedProcess[str]:
    """`git <args>` that reaches GitHub, as the task runs it: never asking for
    a credential (there is no one to answer, and a dialog each minute is
    worse than a failure), and given up after NET_TIMEOUT seconds."""
    env = cls._clean_git_env()
    env.update(GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="never")
    cmd = ["git", "-c", "credential.interactive=never", *args]
    try:
        return subprocess.run(cmd, cwd=str(tree), env=env, capture_output=True, text=True,
                              encoding="utf-8", errors="replace", check=False,
                              timeout=NET_TIMEOUT)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(cmd, 124, "", f"no answer in {NET_TIMEOUT}s")


def one_line(text: str) -> str:
    return " ".join(text.split())[:300]


def attempt(tree: Path, cls, environ: dict[str, str] | None) -> tuple[str, str]:
    """Try to bring `tree` up to `main`. Returns `(outcome, detail)`: an
    outcome in FOLLOWING, "blocked" (git refused, and `detail` says why), or
    one of busy, offline, fetch-failed and waiting, a pass that could not
    finish."""
    busy = in_progress(tree / ".git")
    if busy:
        return "busy", f"{busy} is left in {tree / '.git'}: an operation under way, or abandoned"
    remote = net_git(cls, ["ls-remote", "origin", "refs/heads/main"], tree)
    words = remote.stdout.split()
    if remote.returncode != 0 or not words:
        return "offline", "git ls-remote origin failed: " + one_line(remote.stderr or remote.stdout)
    local = cls._git(["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"], tree)
    fetched = False
    if words[0] != local.stdout.strip():
        f = net_git(cls, ["fetch", "--quiet", "origin", "+refs/heads/main:refs/remotes/origin/main"],
                    tree)
        if f.returncode != 0:
            return "fetch-failed", "git fetch of main failed: " + one_line(f.stderr)
        fetched = True
    result = cls.sync_integration_tree(tree, environ=environ)
    if result is not None and result[0] == "blocked":
        return "blocked", result[1]
    head = cls._git(["rev-parse", "--verify", "--quiet", "HEAD"], tree).stdout.strip()
    main = cls._git(["rev-parse", "--verify", "--quiet", "origin/main"], tree).stdout.strip()
    if head and head == main:
        if result is not None and result[0] == "done":
            return "done", result[1]
        return ("fetched" if fetched else "current"), ""
    return "waiting", (f"{tree} is behind main and git did not take its index; a "
                       f"{tree / '.git' / 'index.lock'} left by a killed git keeps it there")


def one_pass(tree: Path, cls, notify, now: _dt.datetime | None = None,
             environ: dict[str, str] | None = None) -> str:
    """One pass over the integration tree `tree`; `cls` is check-lane-signals,
    `notify(title, body)` shows the operator a message and says whether it
    could, and `environ` is the environment git would see (None: this
    process's). Returns the outcome `attempt` reached, or "error"."""
    now = now or _dt.datetime.now().astimezone()
    git_dir = tree / ".git"
    state_path, log_path = git_dir / STATE_NAME, git_dir / LOG_NAME
    try:
        outcome, detail = attempt(tree, cls, environ)
    except Exception as e:  # noqa: BLE001 -- a crashing pass is a trouble to report
        outcome = "error"
        detail = "the pass crashed: " + one_line("".join(
            traceback.format_exception_only(type(e), e)))
    state = read_state(state_path)
    if outcome in FOLLOWING:
        if state:
            write_state(state_path, {})
            log(log_path, now, "following main again")
        if outcome == "done":
            log(log_path, now, detail)
        return outcome
    if outcome == "blocked":
        key, title, body = reason_key(detail), "SlateOS: the integration tree is not following main", detail
        kept = {"key": state.get("key"), "notified_at": state.get("notified_at")}
    else:
        since = parse_time(state.get("trouble_since")) if state.get("trouble") == outcome else None
        if since is None:
            write_state(state_path, {**state, "trouble": outcome, "trouble_since": now.isoformat()})
            log(log_path, now, f"{outcome}: {detail}")
            return outcome
        if now - since < TROUBLE_AFTER:
            return outcome
        minutes = int((now - since).total_seconds() // 60)
        key = f"stuck:{outcome}"
        title = "SlateOS: the integration tree has not followed main for an hour"
        body = f"For {minutes} minutes: {detail}"
        kept = {k: state.get(k) for k in ("key", "notified_at", "trouble", "trouble_since")}
    if should_notify(state, key, now):
        if notify(title, body[:400]):
            kept.update(key=key, notified_at=now.isoformat())
            log(log_path, now, "operator notified: " + body)
        else:
            log(log_path, now, "the notification could not be shown: " + body)
    write_state(state_path, {k: v for k, v in kept.items() if v is not None})
    return outcome


def toast(title: str, body: str) -> bool:
    """A Windows notification, through Windows PowerShell's WinRT types (no
    module to install). The texts travel in the environment, so nothing in
    them is parsed as PowerShell. True if PowerShell reported no error."""
    script = (
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, "
        "ContentType = WindowsRuntime] > $null\n"
        "$t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent("
        "[Windows.UI.Notifications.ToastTemplateType]::ToastText02)\n"
        "$n = $t.GetElementsByTagName('text')\n"
        "$n.Item(0).AppendChild($t.CreateTextNode($env:SLATEOS_TOAST_TITLE)) > $null\n"
        "$n.Item(1).AppendChild($t.CreateTextNode($env:SLATEOS_TOAST_BODY)) > $null\n"
        "$x = [Windows.UI.Notifications.ToastNotification]::new($t)\n"
        "[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("
        "'{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe'"
        ").Show($x)\n"
    )
    return run_powershell(script, {"SLATEOS_TOAST_TITLE": title, "SLATEOS_TOAST_BODY": body})


def run_powershell(script: str, extra_env: dict[str, str] | None = None) -> bool:
    """`script` in Windows PowerShell, passed encoded so that no quoting can
    change it. True if it exited 0."""
    encoded = base64.b64encode(script.encode("utf-16-le")).decode("ascii")
    env = dict(os.environ)
    env.update(extra_env or {})
    try:
        r = subprocess.run(
            ["powershell.exe", "-NoProfile", "-NonInteractive", "-EncodedCommand", encoded],
            env=env, capture_output=True, check=False, timeout=120)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return r.returncode == 0


def install(tree: Path) -> int:
    """Register the scheduled task to run the integration tree's copy of this
    file every minute, as the current user, with no window."""
    script = tree / "scripts" / "integration-tree-sync.py"
    if not script.is_file():
        print(f"integration-tree-sync: {script} does not exist yet: the integration "
              "tree has not caught up with the commit that adds this script. Bring "
              "it up to main first (check-lane-signals.py says why it cannot be).",
              file=sys.stderr)
        return 1
    py = Path(sys.executable).resolve()

    def ps(s: str) -> str:
        return "'" + s.replace("'", "''") + "'"

    args = f'--headless "{py}" "{script}"'
    body = (
        f"$a = New-ScheduledTaskAction -Execute 'conhost.exe' -Argument {ps(args)} "
        f"-WorkingDirectory {ps(str(tree))}\n"
        "$t = New-ScheduledTaskTrigger -Once -At (Get-Date).AddMinutes(1) "
        "-RepetitionInterval (New-TimeSpan -Minutes 1)\n"
        "$s = New-ScheduledTaskSettingsSet -MultipleInstances IgnoreNew "
        "-ExecutionTimeLimit (New-TimeSpan -Minutes 5) -StartWhenAvailable "
        "-AllowStartIfOnBatteries -DontStopIfGoingOnBatteries\n"
        "$me = [System.Security.Principal.WindowsIdentity]::GetCurrent().Name\n"
        "$p = New-ScheduledTaskPrincipal -UserId $me -LogonType Interactive -RunLevel Limited\n"
        f"Register-ScheduledTask -TaskName {ps(TASK_NAME)} -Action $a -Trigger $t -Settings $s "
        "-Principal $p -Force -Description "
        f"{ps('Fast-forwards the SlateOS integration tree to main every minute, and notifies when it cannot. ' + str(script))}"
        " > $null\n"
    )
    if not run_powershell(body):
        print("integration-tree-sync: registering the task failed", file=sys.stderr)
        return 1
    print(f"integration-tree-sync: '{TASK_NAME}' runs {script} every minute with {py}")
    return 0


def uninstall() -> int:
    ok = run_powershell(
        f"Unregister-ScheduledTask -TaskName '{TASK_NAME}' -Confirm:$false -ErrorAction Stop")
    print(f"integration-tree-sync: '{TASK_NAME}' " + ("removed" if ok else "was not removed"))
    return 0 if ok else 1


def self_test() -> int:
    import tempfile

    bad: list[str] = []

    def check(label: str, got, want) -> None:
        if got != want:
            bad.append(label)
            print(f"FAIL  {label}\n        got:  {got!r}\n        want: {want!r}")
        else:
            print(f"ok    {label}")

    now = _dt.datetime(2026, 10, 21, 12, 0, tzinfo=_dt.timezone.utc)
    a = "the integration tree X is 3 commit(s) behind main, and git would not"
    b = "the integration tree X is 41 commit(s) behind main, and git would not"
    check("the count behind is not part of the reason", reason_key(a), reason_key(b))
    check("a new reason is shown", should_notify({}, "k", now), True)
    shown = {"key": "k", "notified_at": now.isoformat()}
    check("the same reason within six hours is not",
          should_notify(shown, "k", now + _dt.timedelta(hours=5)), False)
    check("the same reason after six hours is again",
          should_notify(shown, "k", now + REMIND), True)
    check("another reason is shown at once",
          should_notify(shown, "other", now + _dt.timedelta(minutes=1)), True)
    check("a stored time that is not one is no time",
          should_notify({"key": "k", "notified_at": None}, "k", now), True)

    cls = signals()
    env = cls._clean_git_env()

    def git(cwd: Path, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["git", "-c", "user.name=fixture", "-c", "user.email=fixture@invalid",
             "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false", *args],
            cwd=str(cwd), env=env, capture_output=True, text=True,
            encoding="utf-8", errors="replace", check=False)

    tmp = Path(tempfile.mkdtemp(prefix="integration-sync-"))
    try:
        origin = tmp / "origin"
        origin.mkdir()
        git(origin, "init", "-q", "-b", "main")
        gd = git(origin, "rev-parse", "--absolute-git-dir").stdout.strip()
        if not gd or Path(gd).resolve() != (origin / ".git").resolve():
            check("the fixture repository is the scratch one", gd, str(origin / ".git"))
            return 1
        (origin / "f.txt").write_bytes(b"one\n")
        git(origin, "add", "f.txt")
        git(origin, "commit", "-q", "-m", "one")
        tree = tmp / "os"
        git(tmp, "clone", "-q", str(origin), str(tree))
        state_path = tree / ".git" / STATE_NAME
        shown_now: list[tuple[str, str]] = []
        works = [True]

        def notify(title: str, body: str) -> bool:
            if works[0]:
                shown_now.append((title, body))
            return works[0]

        def run(at: _dt.datetime) -> str:
            # environ={}: as the task runs it, whoever runs this self-test --
            # a git hook's GIT_DIR would make the fast-forward stand aside.
            return one_pass(tree, cls, notify, at, environ={})

        check("a tree already on main is current", run(now), "current")
        (origin / "f.txt").write_bytes(b"two\n")
        git(origin, "commit", "-q", "-am", "two")
        check("main moved: fetched and fast-forwarded", run(now), "done")
        check("the tree is at origin's main",
              git(tree, "rev-parse", "HEAD").stdout, git(origin, "rev-parse", "HEAD").stdout)

        # A commit of the tree's own: git refuses at once, and says why.
        (tree / "g.txt").write_bytes(b"mine\n")
        git(tree, "add", "g.txt")
        git(tree, "commit", "-q", "-m", "a commit of the tree's own")
        (origin / "f.txt").write_bytes(b"three\n")
        git(origin, "commit", "-q", "-am", "three")
        works[0] = False
        check("a commit of its own blocks it", run(now), "blocked")
        check("a notification that cannot be shown is not counted as shown",
              read_state(state_path).get("key"), None)
        works[0] = True
        run(now)
        check("so the next pass shows it", len(shown_now), 1)
        run(now + _dt.timedelta(minutes=1))
        check("but not again a minute later", len(shown_now), 1)
        run(now + REMIND)
        check("and again after six hours", len(shown_now), 2)
        check("the tree's own commit is kept",
              git(tree, "log", "-1", "--format=%s").stdout.strip(), "a commit of the tree's own")
        git(tree, "reset", "-q", "--hard", "origin/main")
        check("back on main, it follows main again", run(now + REMIND), "current")
        check("and the reason is forgotten", read_state(state_path), {})

        # Anything that keeps a pass from finishing is told after an hour.
        t0 = now + 2 * REMIND
        (tree / ".git" / "MERGE_HEAD").write_bytes(b"0" * 40 + b"\n")
        before = len(shown_now)
        check("a merge left in the tree: nothing is done", run(t0), "busy")
        run(t0 + _dt.timedelta(minutes=59))
        check("and nothing said within the hour", len(shown_now), before)
        run(t0 + TROUBLE_AFTER)
        check("but after an hour it is", len(shown_now), before + 1)
        check("naming what is left there", "MERGE_HEAD" in shown_now[-1][1], True)
        run(t0 + TROUBLE_AFTER + _dt.timedelta(minutes=1))
        check("once", len(shown_now), before + 1)
        (tree / ".git" / "MERGE_HEAD").unlink()
        check("cleared, it follows main again", run(t0 + 2 * TROUBLE_AFTER), "current")

        # So is a pass that crashes, rather than dying unseen each minute.
        class Broken:
            def __getattr__(self, name):
                if name == "sync_integration_tree":
                    raise AttributeError("sync_integration_tree is gone")
                return getattr(cls, name)

        t1 = t0 + 3 * TROUBLE_AFTER
        (origin / "f.txt").write_bytes(b"four\n")
        git(origin, "commit", "-q", "-am", "four")
        before = len(shown_now)
        check("a crashing pass is an outcome, not a crash",
              one_pass(tree, Broken(), notify, t1, environ={}), "error")
        one_pass(tree, Broken(), notify, t1 + TROUBLE_AFTER, environ={})
        check("and is told after an hour", len(shown_now), before + 1)
        check("with what crashed", "sync_integration_tree is gone" in shown_now[-1][1], True)
    finally:
        cls._gitenv().remove_tree(tmp)
    print("integration-tree-sync self-test: " + ("ok" if not bad else f"{len(bad)} failed"))
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return self_test()
    known = ("--install", "--uninstall")
    unknown = selftestflag.unknown_options(argv, known)
    if unknown:
        print(f"integration-tree-sync: unknown option(s): {' '.join(unknown)}\n"
              + __doc__.split("usage:")[1].split("\n\n")[0], file=sys.stderr)
        return 2
    if "--uninstall" in argv:
        return uninstall()
    cls = signals()
    tree = cls.integration_tree(HERE.parent)
    if tree is None:
        print("integration-tree-sync: cannot find the integration tree", file=sys.stderr)
        return 1
    if "--install" in argv:
        return install(tree)
    one_pass(tree, cls, toast)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
