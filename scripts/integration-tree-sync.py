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
  3. when the tree cannot be brought up to `main`, shows the operator a
     Windows notification: once for each new reason, and again every six
     hours while it lasts, rather than waiting for a lane to notice.

Nothing is done while a rebase, merge, cherry-pick, revert or bisect is under
way in the tree: that is someone working there. A pass that changes
something, or cannot, appends a line to `slateos-integration-sync.log` in the
git directory; the reason last notified is kept beside it in
`slateos-integration-sync.json`.

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
import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import selftestflag  # noqa: E402

TASK_NAME = "SlateOS integration tree sync"
STATE_NAME = "slateos-integration-sync.json"
LOG_NAME = "slateos-integration-sync.log"
#: How often a reason that has not changed is shown again.
REMIND = _dt.timedelta(hours=6)
#: The log is cut back to its last KEEP lines once it reaches twice that.
KEEP = 1000
#: What the git directory holds while someone is part-way through an operation.
IN_PROGRESS = ("rebase-merge", "rebase-apply", "MERGE_HEAD", "CHERRY_PICK_HEAD",
               "REVERT_HEAD", "BISECT_LOG")


def signals():
    """`check-lane-signals.py` as a module: the fast-forward and the git
    helpers live there once, for lanes and for this task."""
    spec = importlib.util.spec_from_file_location(
        "check_lane_signals", HERE / "check-lane-signals.py")
    if spec is None or spec.loader is None:
        raise ImportError("cannot load check-lane-signals.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


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


def should_notify(state: dict, key: str, now: _dt.datetime) -> bool:
    """Whether to show `key` now: a reason not shown before, or one last
    shown REMIND or longer ago."""
    if state.get("key") != key:
        return True
    try:
        last = _dt.datetime.fromisoformat(state.get("notified_at", ""))
    except ValueError:
        return True
    return now - last >= REMIND


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
    import os

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


def one_pass(tree: Path, cls, notify, now: _dt.datetime | None = None) -> str:
    """One pass over the integration tree `tree`; `cls` is check-lane-signals,
    `notify(title, body)` shows the operator a message. Returns what it did,
    in a word: busy, offline, current, fetched, done, blocked, waiting."""
    now = now or _dt.datetime.now().astimezone()
    git_dir = tree / ".git"
    state_path, log_path = git_dir / STATE_NAME, git_dir / LOG_NAME
    busy = in_progress(git_dir)
    if busy:
        return "busy"
    remote = cls._git(["ls-remote", "origin", "refs/heads/main"], tree)
    remote_main = remote.stdout.split()[0] if remote.returncode == 0 and remote.stdout.split() else ""
    if not remote_main:
        return "offline"
    local_main = cls._git(["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main"],
                          tree).stdout.strip()
    fetched = False
    if remote_main != local_main:
        f = cls._git(["fetch", "--quiet", "origin", "refs/heads/main:refs/remotes/origin/main"], tree)
        if f.returncode != 0:
            log(log_path, now, "fetch of main failed: " + " ".join(f.stderr.split())[:300])
            return "offline"
        fetched = True
    result = cls.sync_integration_tree(tree)
    state = read_state(state_path)
    if result is not None and result[0] == "blocked":
        key = reason_key(result[1])
        if should_notify(state, key, now):
            notify("SlateOS: the integration tree is not following main",
                   result[1][:400])
            write_state(state_path, {"key": key, "notified_at": now.isoformat()})
            log(log_path, now, "blocked, operator notified: " + result[1])
        return "blocked"
    if result is not None and result[0] == "done":
        log(log_path, now, result[1])
    head = cls._git(["rev-parse", "--verify", "--quiet", "HEAD"], tree).stdout.strip()
    main = cls._git(["rev-parse", "--verify", "--quiet", "origin/main"], tree).stdout.strip()
    if head and head == main:
        if state:
            write_state(state_path, {})
            log(log_path, now, "following main again")
        return "done" if result is not None else ("fetched" if fetched else "current")
    # Behind, but nothing was said: another process holds the index at the
    # moment. The next pass tries again.
    return "waiting"


def install(tree: Path) -> int:
    """Register the scheduled task to run the integration tree's copy of this
    file every minute, as the current user, with no window."""
    script = tree / "scripts" / "integration-tree-sync.py"
    if not script.is_file():
        print(f"integration-tree-sync: {script} does not exist yet -- publish this "
              "script to main and let the tree catch up first", file=sys.stderr)
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
        shown_now: list[tuple[str, str]] = []

        def notify(title: str, body: str) -> None:
            shown_now.append((title, body))

        check("a tree already on main is current", one_pass(tree, cls, notify, now), "current")
        (origin / "f.txt").write_bytes(b"two\n")
        git(origin, "commit", "-q", "-am", "two")
        check("main moved: fetched and fast-forwarded", one_pass(tree, cls, notify, now), "done")
        check("the tree is at origin's main",
              git(tree, "rev-parse", "HEAD").stdout, git(origin, "rev-parse", "HEAD").stdout)
        (tree / "g.txt").write_bytes(b"mine\n")
        git(tree, "add", "g.txt")
        git(tree, "commit", "-q", "-m", "a commit of the tree's own")
        (origin / "f.txt").write_bytes(b"three\n")
        git(origin, "commit", "-q", "-am", "three")
        check("a commit of its own blocks it", one_pass(tree, cls, notify, now), "blocked")
        check("and the operator is told", len(shown_now), 1)
        one_pass(tree, cls, notify, now + _dt.timedelta(minutes=1))
        check("but not again a minute later", len(shown_now), 1)
        one_pass(tree, cls, notify, now + REMIND)
        check("and again after six hours", len(shown_now), 2)
        check("the tree's own commit is kept",
              git(tree, "log", "-1", "--format=%s").stdout.strip(), "a commit of the tree's own")
        (tree / ".git" / "MERGE_HEAD").write_bytes(b"0" * 40 + b"\n")
        check("nothing is done during a merge", one_pass(tree, cls, notify, now + 2 * REMIND), "busy")
        check("not even a reminder", len(shown_now), 2)
        (tree / ".git" / "MERGE_HEAD").unlink()
        git(tree, "reset", "-q", "--hard", "origin/main")
        check("back on main, it follows main again",
              one_pass(tree, cls, notify, now + 2 * REMIND), "current")
        check("and the reason is forgotten", read_state(tree / ".git" / STATE_NAME), {})
    finally:
        import shutil

        shutil.rmtree(tmp, ignore_errors=True)
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
    cls = signals()
    tree = cls.integration_tree(HERE.parent)
    if tree is None:
        print("integration-tree-sync: cannot find the integration tree", file=sys.stderr)
        return 1
    if "--install" in argv:
        return install(tree)
    if "--uninstall" in argv:
        return uninstall()
    one_pass(tree, cls, toast)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
