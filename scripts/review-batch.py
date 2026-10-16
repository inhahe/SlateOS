#!/usr/bin/env python3
"""Review a batch of commits at the level its contents call for; gate `main` on it.

Every batch of commits gets an independent review before it is published
(`claude-global.md`, "Code review"): Opus at `xhigh`, or -- where a miss is
expensive -- Opus at `max` and then Fable at `xhigh`. This script makes that
call from the batch itself, the same way every time, so that nobody has to
watch for it, and checks that what is published to `main` was reviewed so:

    python scripts/review-batch.py <base>..<tip>          review it, print the record
    python scripts/review-batch.py --tier <base>..<tip>   only say which level, and why
    python scripts/review-batch.py --check <sha>...       the pre-push gate for `main`
    python scripts/review-batch.py --stats [--since DATE] fixed findings per changed line
    python scripts/review-batch.py --self-test

The batch is what `<base>..<tip>` adds that `<remote>/main` does not have, so
the merges of `main` a lane makes before publishing bring nothing of the other
lanes' into it. A merge's own conflict resolution is part of it.

A review runs in a scratch worktree at the tip, so that the lane can go on
working in its own while it runs; the scratch worktree and anything built in it
are deleted afterwards. Each reviewer's report is kept in
`<git-common-dir>/reviews/`. Run it in the background: a `max` review of a
large batch takes an hour.

After the reviews, cargo-mutants breaks the batch's changed Rust lines one at
a time -- `<` for `<=`, a function's body for a default value -- and runs the
crate's tests against each (`--mutants-budget`, half an hour by default, at
idle priority; a large batch is sampled). A mutant no test notices is a
finding like a reviewer's: add the test that catches it, or say why the change
cannot matter (`|` for `^` on flags that never share a bit cannot).

THE RECORD. A run ends by printing trailers for the follow-up commit -- the one
that fixes what the review found, or an empty one (`git commit --allow-empty`)
when nothing needed fixing -- to go in its message's last paragraph, beside
`Co-Authored-By`, with `Review-findings` filled in after triage:

    Reviewed: <base>..<tip>
    Review-tier: max
    Review-runs: opus-max <report>, fable-xhigh <report>
    Review-lane: F
    Review-written-at: xhigh
    Review-findings: <n> fixed, <m> dismissed

`--check` reads them. A commit published to `main` that changes code must lie in
some record's range, at a tier at least its own, with a report on this machine
for each review the tier runs (Fable's may be `unavailable`: when its allowance
is spent the Opus review stands alone). A report counts only if the review in it
finished, which is when it ends with `/code-review`'s findings array: a reviewer
that cannot review exits 0 all the same, saying why instead (out of credit, at
its session limit, cut off at its output limit). The commits after the reviewed tip up to
and including the record's own are the follow-up fixes, which are not reviewed
again. `--stats` reads the records to say whether the effort level code is
written at shows in how much its reviews find. That level is read from the
session's transcript, which records it for every request, at each commit's
time; `CLAUDE_EFFORT`, the session's level now, only where it cannot say.

Exit status: 0 done (or, for `--check`, every commit reviewed); 1 a review that
could not run, or a commit `--check` refuses; 2 a usage error.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path, PurePosixPath

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gitenv  # noqa: E402
import selftestflag  # noqa: E402

#: Where a missed bug is expensive, by the list in `claude-global.md`: kernel
#: code; the C library; the system services and PID 1; logins, privilege and
#: credentials; the network stack, its daemons and its clients; crypto; the
#: parsers of untrusted input (decoders, decompressors, document viewers, the
#: remote desktop's protocol); and the tools that destroy or rewrite data on
#: disk. Repo-relative directory prefixes: a change to code under one is
#: reviewed at `max`. Extend it when a lane adds such a crate.
MAX_PATHS = (
    "kernel/", "posix/", "services/", "init/",
    # Logins, privilege, credentials.
    "userspace/authlib/", "userspace/chage/", "userspace/chpasswd/", "userspace/doas/",
    "userspace/getty/", "userspace/login/", "userspace/logind/", "userspace/newgrp/",
    "userspace/passwd/", "userspace/polkit/", "userspace/pwdb/", "userspace/su/",
    "userspace/sudo/", "userspace/useradd/", "userspace/useradm/", "userspace/userdb/",
    "gui/credentials/", "gui/credentialsd/", "gui/secret/", "gui/loginusers/",
    "apps/credvault/", "apps/credmanager/", "apps/lockscreen/", "apps/passwordgen/",
    "readpass/", "pwkdf/",
    # Networking.
    "net/", "net80211/", "netipc/", "netproto/", "netring/",
    "userspace/ssh/", "userspace/sshd/", "userspace/sshwire/", "userspace/ssh-keygen/",
    "userspace/scp/", "userspace/sftp/", "userspace/ftp/", "userspace/ftpd/",
    "userspace/inetd/", "userspace/telnet/", "userspace/curl/", "userspace/wget/",
    "userspace/dhcpcd/", "userspace/ntpd/", "userspace/iptables/", "userspace/nftables/",
    "gui/remote/", "gui/compositor/", "apps/remotedesktop/", "apps/email/", "apps/torrent/",
    # Crypto.
    "aes/", "hmac/", "sha1/", "sha2/", "blake2/", "sm3/", "md5/", "rustcrypto/",
    # Parsers of untrusted input.
    "gui/video/", "gui/imagecodec/", "gui/font/", "apps/videoplayer/", "apps/pdfviewer/",
    "deflate/", "bzip2/", "xz/", "sevenz/", "ziparchive/", "elfparse/",
    "userspace/gzip/", "userspace/zip/", "userspace/cpio/", "userspace/file/",
    "userspace/libmagic/", "apps/archivemanager/", "apps/tararchive/",
    "userspace/readelf/", "userspace/objdump/", "apps/exif/",
    # What destroys or rewrites data on disk.
    "userspace/fsck/", "userspace/mkfs/", "userspace/fdisk/", "userspace/wipefs/",
    "userspace/blkdiscard/", "userspace/mount/", "userspace/ulmount/", "userspace/losetup/",
    "userspace/rsync/", "userspace/backup/", "apps/partmanager/", "apps/diskimager/",
    "apps/systemrestore/", "apps/undelete/", "apps/backup/",
)

#: What makes a change high-stakes wherever it is, in an added line of code
#: that is not a comment: unsafe Rust and inline assembly; shared mutable
#: state and locking; deleting or truncating files, in Rust, C, Python and
#: the shells.
MAX_CONTENT = re.compile(
    r"\bunsafe\b|\b(?:global_|naked_)?asm!|\bstatic\s+mut\b"
    r"|\bAtomic[A-Z]\w*|\bOrdering::(?:Relaxed|Acquire|Release|AcqRel|SeqCst)\b"
    r"|\b(?:Mutex|RwLock|Condvar|SpinLock)\b|\bspin::|\bthreading\.R?Lock\b|\bflock\b"
    r"|\bremove_(?:file|dir|dir_all|tree)\b|\b(?:unlink|unlinkat|rmdir|ftruncate|truncate)\s*\("
    r"|\.set_len\s*\(|\bshutil\.rmtree\b|\bos\.(?:remove|unlink|rmdir|removedirs)\b"
    r"|\brm\s+-[A-Za-z]*[rf]|\bRemove-Item\b|\b(?:rd|rmdir)\s+/s\b|\bdel\s+/"
    r"|\bworktree\s+remove\b|\bbranch\s+-D\b|\breset\s+--hard\b|\bpush\s+(?:-f|--force)\b"
)

#: Inside unsafe code already there: a change to a line of a function that
#: contains this is as risky as adding it.
UNSAFE = re.compile(r"\bunsafe\b")

#: Comment syntax by suffix. A Rust lint attribute (`#![forbid(unsafe_code)]`)
#: says what code may not do, so it counts as a comment too; Rust 2024's
#: `#[unsafe(no_mangle)]` does not.
LINE_COMMENT = {
    ".rs": re.compile(r"^\s*(?://|/\*|\*(?:\s|/|$)|#!?\[\s*(?:forbid|deny|warn|allow)\s*\()"),
    ".c": re.compile(r"^\s*(?://|/\*|\*(?:\s|/|$))"),
    ".py": re.compile(r"^\s*#"),
    ".sh": re.compile(r"^\s*#"),
    ".ps1": re.compile(r"^\s*#"),
    ".toml": re.compile(r"^\s*#"),
    ".bat": re.compile(r"^\s*(?:@?REM\b|::)", re.I),
    ".asm": re.compile(r"^\s*(?:;|#|//)"),
    ".ada": re.compile(r"^\s*--"),
}
COMMENT_STYLE = {
    ".rs": ".rs", ".c": ".c", ".h": ".c", ".cc": ".c", ".cpp": ".c", ".hpp": ".c",
    ".js": ".c", ".ts": ".c", ".mjs": ".c", ".py": ".py", ".sh": ".sh", ".bash": ".sh",
    ".ps1": ".ps1", ".toml": ".toml", ".bat": ".bat", ".cmd": ".bat", ".s": ".asm",
    ".asm": ".asm", ".ld": ".c", ".adb": ".ada", ".ads": ".ada", ".gpr": ".ada",
}

#: Files that are code. Everything else -- documentation, data, test
#: fixtures, generated reports -- is reviewed by nobody, as the rule says.
CODE_SUFFIXES = set(COMMENT_STYLE)

TIERS = ("none", "xhigh", "max")

#: The reviews each tier runs, in order: (model, `/code-review` level).
RUNS = {"xhigh": (("opus", "xhigh"),), "max": (("opus", "max"), ("fable", "xhigh"))}

#: The review that may be missing from a record: Fable's, when its share of
#: the allowance is spent (`claude-global.md`).
OPTIONAL_RUNS = {("fable", "xhigh")}

#: A fenced ```json block, the form `/code-review` ends a finished review with.
JSON_BLOCK = re.compile(r"^[ \t]*```json[ \t]*\n(.*?)^[ \t]*```", re.M | re.S | re.I)

#: What a reviewer says when the review it may skip -- Fable's -- cannot run
#: because its share of the allowance is spent: its limit, or the usage credits
#: a `claude -p` session cannot accept (`claude-global.md`, "Code review"). Only
#: this lets a record go without it. Any other failure is a failure: the batch
#: is reviewed again.
LIMIT_SPENT = re.compile(r"fable limit reached|usage credits", re.I)

#: Commits committed before this day were reviewed and recorded in prose,
#: which `--check` cannot read; they are not checked.
RECORDS_START = "2026-10-12"

#: How long one reviewer may run before it is stopped, in seconds.
REVIEW_TIMEOUT = 4 * 3600

QEMU_IMAGES = ("qemu-system-x86_64.exe", "qemu-system-x86_64w.exe")


# ---------------------------------------------------------------------------
# git
# ---------------------------------------------------------------------------

def git(*args: str, cwd: str | Path | None = None, check: bool = True) -> str:
    """`git args...`'s output, in `cwd`, with no inherited `GIT_DIR`."""
    r = subprocess.run(
        ["git", "-c", "core.quotePath=false", *args], cwd=cwd, env=gitenv.clean_env(),
        capture_output=True, text=True, encoding="utf-8", errors="replace",
        stdin=subprocess.DEVNULL,
    )
    if check and r.returncode != 0:
        raise SystemExit(f"review-batch: git {' '.join(args)}: {r.stderr.strip()}")
    return r.stdout


def common_dir(cwd: str | Path | None = None) -> Path:
    """The git directory every worktree of this repository shares."""
    return Path(git("rev-parse", "--path-format=absolute", "--git-common-dir", cwd=cwd).strip())


def reports_dir(cwd: str | Path | None = None) -> Path:
    return common_dir(cwd) / "reviews"


def resolve(rev: str, cwd: str | Path | None = None, check: bool = True) -> str:
    return git("rev-parse", "--verify", "--quiet", f"{rev}^{{commit}}", cwd=cwd,
               check=check).strip()


def published(remote: str, cwd: str | Path | None = None) -> list[str]:
    """`--not <remote>/main`, or nothing if this clone has no such ref."""
    ref = f"refs/remotes/{remote}/main"
    return ["--not", ref] if resolve(ref, cwd=cwd, check=False) else []


# ---------------------------------------------------------------------------
# What a change is
# ---------------------------------------------------------------------------

def is_code(path: str) -> bool:
    """Whether `path` (repo-relative, `/`-separated) is code."""
    p = PurePosixPath(path)
    if p.suffix.lower() in CODE_SUFFIXES:
        return True
    # The hooks under scripts/ are shell scripts with no suffix.
    return p.suffix == "" and path.startswith("scripts/")


def is_comment(path: str, line: str) -> bool:
    style = COMMENT_STYLE.get(PurePosixPath(path).suffix.lower(), ".sh")
    return bool(LINE_COMMENT[style].match(line))


class Change:
    """What a commit does to one file: the lines it adds, and the unchanged
    lines of the functions it changes them in."""

    def __init__(self) -> None:
        self.added: list[str] = []
        self.context: list[str] = []


def fuzzed_crates(cwd: str | Path | None = None) -> tuple[str, ...]:
    """The directories that keep a fuzz harness (`<crate>/fuzz/`, with the
    `harness.toml` `scripts/fuzz.py` reads or cargo-fuzz's `Cargo.toml`), as
    `MAX_PATHS`-style prefixes. Every parser of untrusted input is to have one
    (`CLAUDE.md`), so having one marks a crate for `max` without a hand-kept
    entry."""
    out = set()
    for path in git("ls-files", cwd=cwd, check=False).splitlines():
        crate, sep, rest = path.partition("/fuzz/")
        if sep and rest in ("harness.toml", "Cargo.toml"):
            out.add(crate + "/")
    return tuple(sorted(out))


def judge(files: dict[str, Change], fuzzed: tuple[str, ...] = ()) -> tuple[str, list[str]]:
    """The tier a change calls for, and why: `none` when no code changes;
    `max` when code under `MAX_PATHS` or a crate in `fuzzed` (`fuzzed_crates`)
    changes, an added line of code matches `MAX_CONTENT`, or a Rust function
    containing `unsafe` is changed; else `xhigh`."""
    tier, why = "none", []
    for path, ch in files.items():
        if not is_code(path):
            continue
        if tier == "none":
            tier = "xhigh"
        prefix = next((p for p in (*MAX_PATHS, *fuzzed) if path.startswith(p)), None)
        if prefix:
            tier = "max"
            why.append(f"{path}: under {prefix}")
            continue
        hit = next(((line, m) for line in ch.added if not is_comment(path, line)
                    for m in [MAX_CONTENT.search(line)] if m), None)
        if hit:
            tier = "max"
            why.append(f"{path}: adds `{hit[1].group(0)}`: {hit[0].strip()[:100]}")
            continue
        if path.endswith(".rs") and ch.added and any(
                UNSAFE.search(line) and not is_comment(path, line) for line in ch.context):
            tier = "max"
            why.append(f"{path}: changes a function with unsafe code in it")
    return tier, why


def higher(a: str, b: str) -> str:
    return a if TIERS.index(a) >= TIERS.index(b) else b


class Commit:
    """One commit: its sha, parents, committer date, message and changes. A
    merge's changes are what its resolution added that neither side had."""

    def __init__(self, sha: str, parents: list[str], date: str, message: str):
        self.sha, self.parents, self.date, self.message = sha, parents, date, message
        self.files: dict[str, Change] = {}

    def tier(self, fuzzed: tuple[str, ...] = ()) -> tuple[str, list[str]]:
        return judge(self.files, fuzzed)

    def subject(self) -> str:
        lines = self.message.strip().splitlines()
        return lines[0][:70] if lines else ""


SEP = "\x1e"  # a record separator: git prints one before and after each header


def commits(*revs: str, cwd: str | Path | None = None) -> list[Commit]:
    """The commits `revs` names (`git log` arguments), with what each changes --
    one `git log` for all of them. Every function a commit changes comes whole
    (`-W`), so that a change inside unsafe code can be seen; a merge shows only
    what its resolution adds (`--cc`)."""
    out = git("log", "-p", "-W", "--cc", "--no-renames", "--no-color", "--no-ext-diff",
              "--src-prefix=a/", "--dst-prefix=b/",
              f"--format={SEP}%H%n%P%n%cI%n%B{SEP}", *revs, "--", cwd=cwd)
    pieces = out.split(SEP)[1:]
    result: list[Commit] = []
    for head, diff in zip(pieces[0::2], pieces[1::2] + [""] * (len(pieces) % 2)):
        sha, parents, date, message = (head.split("\n", 3) + ["", "", ""])[:4]
        c = Commit(sha.strip(), parents.split(), date.strip(), message)
        _read_diff(c, diff)
        result.append(c)
    return result


def _read_diff(c: Commit, diff: str) -> None:
    """Fill `c.files` from its patch. A merge's combined diff has a column per
    parent: a line is the resolution's own when every column says `+`."""
    width = max(1, len(c.parents))
    path, in_header, ch = None, False, None
    for line in diff.splitlines():
        if line.startswith("diff "):
            in_header, path, ch = True, None, None
            if line.startswith("diff --cc ") or line.startswith("diff --combined "):
                path = line.split(" ", 2)[2]
                ch = c.files.setdefault(path, Change())
            continue
        if in_header:
            if line.startswith("--- ") and line != "--- /dev/null" and path is None:
                # A path with a space in it ends with a tab here.
                path = line[4:].rstrip("\t").removeprefix("a/")
                ch = c.files.setdefault(path, Change())
            elif line.startswith("+++ ") and line != "+++ /dev/null":
                new = line[4:].rstrip("\t").removeprefix("b/")
                if new != path:
                    path = new
                    ch = c.files.setdefault(path, Change())
            elif line.startswith("@@"):
                in_header = False
            continue
        if ch is None or line.startswith("@@") or line.startswith("\\"):
            continue
        cols, text = line[:width], line[width:]
        if cols == "+" * width:
            ch.added.append(text)
        elif "-" not in cols:
            ch.context.append(text)


# ---------------------------------------------------------------------------
# Records
# ---------------------------------------------------------------------------

TRAILER = re.compile(r"^(Reviewed|Review-[A-Za-z-]+):[ \t]*(.*?)\s*$", re.M)


class Record:
    """A `Reviewed: <base>..<tip>` record, read from a commit message."""

    def __init__(self, sha: str, fields: dict[str, str]):
        self.sha = sha
        base, _, tip = fields.get("Reviewed", "").partition("..")
        self.base, self.tip = base.strip(), tip.strip()
        self.tier = fields.get("Review-tier", "").strip()
        self.runs: dict[tuple[str, str], str] = {}
        for run in fields.get("Review-runs", "").split(","):
            name, _, report = run.strip().partition(" ")
            model, _, level = name.partition("-")
            if model and level:
                self.runs[(model, level)] = report.strip()
        self.lane = fields.get("Review-lane", "-").strip() or "-"
        self.written_at = fields.get("Review-written-at", "unknown").strip() or "unknown"
        m = re.match(r"\s*(\d+)\s+fixed\D+(\d+)\s+dismissed", fields.get("Review-findings", ""))
        self.fixed, self.dismissed = (int(m.group(1)), int(m.group(2))) if m else (None, None)


def record_of(c: Commit) -> Record | None:
    """The review record `c`'s message carries, if any: the paragraph with a
    `Reviewed: <base>..<tip>` line -- one without `..` is prose, not a
    record."""
    for para in c.message.split("\n\n"):
        fields = {m.group(1): m.group(2) for m in TRAILER.finditer(para)}
        if ".." in fields.get("Reviewed", ""):
            return Record(c.sha, fields)
    return None


def record_problems(r: Record, reports: Path, cwd: str | Path | None) -> list[str]:
    """What is wrong with a record, or nothing."""
    if r.tier not in RUNS:
        return [f"{r.sha[:10]}: its record's Review-tier is {r.tier!r}, not xhigh or max"]
    for end in (r.base, r.tip):
        if not resolve(end, cwd=cwd, check=False):
            return [f"{r.sha[:10]}: its record names {end!r}, which is no commit here"]
    out = []
    batch = f"{resolve(r.base, cwd=cwd)}..{resolve(r.tip, cwd=cwd)}"
    for run in RUNS[r.tier]:
        report = r.runs.get(run)
        if report == "unavailable" and run in OPTIONAL_RUNS:
            continue
        if not report or report == "unavailable":
            out.append(f"{r.sha[:10]}: its record has no {run[0]} review at {run[1]}")
            continue
        # A bare file name in the reviews directory, nothing that leaves it.
        if Path(report).name != report or report.startswith("."):
            out.append(f"{r.sha[:10]}: its record names a report outside the reviews "
                       f"directory: {report}")
            continue
        if not (reports / report).is_file():
            out.append(f"{r.sha[:10]}: its record names a report this machine does not "
                       f"have: {report}")
            continue
        said = (reports / report).read_text(encoding="utf-8", errors="replace")
        # The report says which batch it reviewed and who reviewed it: a record
        # whose runs were copied from another batch's must not pass.
        head = said.split("\n", 2)
        if head[:2] != [f"review of {batch}", f"reviewer: {run[0]} at {run[1]}"]:
            out.append(f"{r.sha[:10]}: its record names a report of another review: "
                       f"{report} ({' / '.join(head[:2])})")
        elif findings_of(said) is None:
            out.append(f"{r.sha[:10]}: its record names a report that holds no finished "
                       f"review: {report}")
    return out


# ---------------------------------------------------------------------------
# --tier and the review run
# ---------------------------------------------------------------------------

def batch_tier(rng: str, remote: str = "origin",
               cwd: str | Path | None = None) -> tuple[str, list[str], list[Commit]]:
    cs = commits(rng, *published(remote, cwd), cwd=cwd)
    tier, why = "none", []
    fuzzed = fuzzed_crates(cwd)
    for c in cs:
        t, w = c.tier(fuzzed)
        tier = higher(tier, t)
        why += [f"{c.sha[:10]} {x}" for x in w]
    return tier, why, cs


def lane_letter(top: Path) -> str:
    """This worktree's lane, as `which-lane.py` says."""
    r = subprocess.run([sys.executable, str(top / "scripts" / "which-lane.py"), "--letter"],
                       cwd=top, capture_output=True, text=True, encoding="utf-8",
                       errors="replace", stdin=subprocess.DEVNULL, env=gitenv.clean_env())
    letter = r.stdout.strip()
    return letter if r.returncode == 0 and len(letter) == 1 else "-"


def qemu_running() -> bool:
    if os.name != "nt":
        return subprocess.run(["pgrep", "-f", "qemu-system"], capture_output=True).returncode == 0
    out = subprocess.run(["tasklist", "/NH"], capture_output=True, text=True,
                         encoding="utf-8", errors="replace").stdout.lower()
    return any(name in out for name in QEMU_IMAGES)


def wait_for_no_qemu(limit: int) -> None:
    """A reviewer may build to check a finding, and nothing may be built
    beside a QEMU guest (`E:/visual studio projects/CLAUDE.md`)."""
    if not qemu_running():
        return
    print("review-batch: a QEMU guest is up; waiting for it to finish", flush=True)
    start = time.monotonic()
    while qemu_running():
        if time.monotonic() - start > limit:
            raise SystemExit("review-batch: a QEMU guest is still up; run the review later")
        time.sleep(60)
    print("review-batch: no QEMU guest is up; starting", flush=True)


def review_commands(base: str, tip: str, tier: str) -> list[tuple[str, str, list[str]]]:
    """(model, level, argv) for each review `tier` runs."""
    claude = os.environ.get("CLAUDE_CODE_EXECPATH") or shutil.which("claude") or "claude"
    # --no-session-persistence: a reviewer saved as a session would turn up in
    # the operator's session lists, one project folder per scratch worktree.
    return [(model, level, [claude, "-p", "--no-session-persistence", "--model", model,
                            f"/code-review {level} commits {base}..{tip}"])
            for model, level in RUNS[tier]]


def findings_of(said: str) -> list | None:
    """The findings a reviewer's output ends with -- the last ```json block
    that holds a list, or a last line that is a bare `[]` -- or None when
    there is none, which means it did not finish a review.

    The test is positive on purpose. A reviewer that could not review still
    exits 0, with a message in place of a review: "Credit balance is too
    low", "You've hit your session limit", a reply cut off at the output
    limit. The next failure will have a message nobody has seen yet.

    A review that finished must not be thrown away for a slip in its JSON (a
    backslash in a quoted regex, a raw tab): such a block is read leniently,
    and one that still does not parse counts as a single finding that says
    so, for the triage to read in the report."""
    found = None
    for m in JSON_BLOCK.finditer(said):
        body = m.group(1).strip()
        value = _lenient_json(body)
        if isinstance(value, list):
            found = value
        elif value is None and body.startswith("[") and body.endswith("]"):
            found = [{"file": "", "summary": "the findings array does not parse as "
                      "JSON; read them in the report"}]
    if found is None:
        last = [ln.strip() for ln in said.split("[run-timeout] child exited", 1)[0]
                .splitlines() if ln.strip() and not ln.startswith("[run-timeout]")]
        if last and last[-1] == "[]":
            found = []
    return found


def _lenient_json(body: str):
    """`body` as JSON, forgiving raw control characters and backslashes that
    start no JSON escape; None if it still does not parse."""
    # Pairwise, left to right, so that a correct `\\` is kept whole and only a
    # backslash before a character no JSON escape starts with is doubled.
    repaired = re.sub(r"\\(.)", lambda m: m.group(0) if m.group(1) in '\\"/bfnrtu'
                      else "\\\\" + m.group(1), body, flags=re.S)
    for text in (body, repaired):
        try:
            return json.loads(text, strict=False)
        except ValueError:
            continue
    return None


def why_not_reviewed(code: int, out: str, err: str) -> str:
    """What to tell the user about a reviewer that did not review: from its
    exit status where `run-timeout.py` set it, else its last words on stdout
    before it exited, else its last on stderr (which otherwise carries only
    start-up warnings)."""
    if code == 124:
        return f"stopped after {REVIEW_TIMEOUT} s"
    if code == 130:
        return "interrupted"
    words = [ln.strip() for ln in out.split("[run-timeout] child exited", 1)[0].splitlines()
             if ln.strip() and not ln.startswith("[run-timeout]")]
    words = words or [ln.strip() for ln in err.splitlines() if ln.strip()]
    reason = words[-1][:200] if words else "it printed nothing"
    return f"it could not be started: {reason}" if code == 125 else reason


#: A reviewer's longest single response, in tokens, thinking included.
#: Claude Code stops one at 64000 unless told otherwise; Fable 5.1 and Opus
#: 5.5 can give 128000. A review at `xhigh` can think past the lower limit in
#: one response and then lose all of it, four times over as Claude Code
#: retries -- a Fable review on 2026-10-16 spent 54 minutes so.
REVIEW_MAX_OUTPUT_TOKENS = 128000


def run_review(argv: list[str], cwd: Path, report: Path,
               header: str) -> tuple[int, str, str]:
    """Run one reviewer in `cwd` under `run-timeout.py` (which stops its whole
    process tree on timeout), keep all it says in `report`, and answer its
    exit status, stdout and stderr."""
    runner = Path(__file__).resolve().parent / "run-timeout.py"
    env = gitenv.clean_env()
    env["CLAUDE_CODE_MAX_OUTPUT_TOKENS"] = str(REVIEW_MAX_OUTPUT_TOKENS)
    r = subprocess.run([sys.executable, str(runner), str(REVIEW_TIMEOUT), *argv], cwd=cwd,
                       env=env, capture_output=True, text=True,
                       encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL)
    report.parent.mkdir(parents=True, exist_ok=True)
    with open(report, "w", encoding="utf-8", newline="\n") as f:
        f.write(header + f"exit status: {r.returncode}\n\n" + r.stdout + r.stderr + "\n")
    return r.returncode, r.stdout, r.stderr


def remove_scratch(top: Path, scratch: Path) -> None:
    """Remove the scratch worktree and whatever was built in it. A failure is
    reported, not raised: the reviews are done by now."""
    try:
        if (scratch / "target").exists() and os.name == "nt":
            # A build directory is tens of gigabytes; `rd` is minutes faster.
            subprocess.run(["cmd", "/c", "rd", "/s", "/q", str(scratch / "target")],
                           capture_output=True, stdin=subprocess.DEVNULL)
        git("worktree", "remove", "--force", str(scratch), cwd=top, check=False)
        if scratch.exists():
            gitenv.remove_tree(scratch)
        git("worktree", "prune", cwd=top, check=False)
    except OSError as e:
        print(f"review-batch: could not remove the scratch worktree {scratch}: {e}; "
              "remove it, and any target/ in it, by hand", flush=True)


# ---------------------------------------------------------------------------
# Mutation testing: does any test notice when the batch's code is broken?
# ---------------------------------------------------------------------------

#: Seconds of mutation testing a batch gets at most, shared by its crates.
#: A mutant costs a rebuild and its package's whole test suite -- a minute
#: for a crate with fixture tests -- so a large batch is sampled, not run
#: whole: the mutants are shuffled, and the budget ends the run.
MUTANTS_BUDGET = 30 * 60

#: Packages whose tests cannot run on the host, so a mutant of theirs can
#: only fail to build or be untested: the kernel is a bare-metal binary.
NO_HOST_TESTS = {"kernel"}

#: Windows' idle priority class, which every process a mutation run starts
#: inherits: a boot test's QEMU guest, or a lane's own build, comes first.
IDLE_PRIORITY_CLASS = 0x00000040


def owning_workspaces(tree: Path, files: list[str]) -> dict[str, set[str]]:
    """Each Cargo workspace root under `tree` that `files` (repo-relative
    `.rs` paths) belong to, repo-relative ("" for the tree's own), with the
    names of the packages they are in."""
    out: dict[str, set[str]] = {}
    roots: dict[Path, str | None] = {}  # crate directory -> its workspace, asked once
    for f in files:
        d = (tree / f).parent
        while d != tree and not (d / "Cargo.toml").is_file():
            d = d.parent
        manifest = d / "Cargo.toml"
        if not manifest.is_file():
            continue
        try:
            package = tomllib.loads(manifest.read_text(encoding="utf-8"))["package"]["name"]
        except (KeyError, OSError, tomllib.TOMLDecodeError):
            continue
        if d not in roots:
            r = subprocess.run(["cargo", "locate-project", "--workspace", "--message-format",
                                "plain"], cwd=d, capture_output=True, text=True,
                               encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL,
                               env=gitenv.clean_env())
            root = Path(r.stdout.strip()).parent.resolve() if r.returncode == 0 else None
            roots[d] = None if root is None or not root.is_relative_to(tree.resolve()) else (
                "" if root == tree.resolve()
                else root.relative_to(tree.resolve()).as_posix())
        if roots[d] is not None:
            out.setdefault(roots[d], set()).add(package)
    return out


#: cargo-mutants' exit statuses that mean it tested what it found: all
#: caught, some missed, some timed out.
MUTANTS_FINISHED = {0, 2, 3}


def mutants_tally(out_dir: Path) -> tuple[int, list[str], int, int]:
    """(mutants a test ran against -- caught, missed or timed out -- the ones
    no test caught, mutants found, mutants that did not build) from a
    cargo-mutants output directory."""
    mo = out_dir / "mutants.out"
    lines = {}
    for name in ("caught", "missed", "timeout", "unviable"):
        p = mo / f"{name}.txt"
        lines[name] = [x for x in p.read_text(encoding="utf-8", errors="replace").splitlines()
                       if x.strip()] if p.is_file() else []
    # Every mutant it found, tested or not, is listed in mutants.json.
    try:
        found = len(json.loads((mo / "mutants.json").read_text(encoding="utf-8")))
    except (OSError, ValueError, TypeError):
        found = 0
    tested = len(lines["caught"]) + len(lines["missed"]) + len(lines["timeout"])
    return tested, lines["missed"], max(found, tested + len(lines["unviable"])), \
        len(lines["unviable"])


def batch_rust_files(scratch: Path, base: str, tip: str, remote: str) -> tuple[list[str], str]:
    """The Rust files the batch's own commits change, and the commit to diff
    the tip against so that only the batch's lines show: the last of
    `<remote>/main` the tip has merged, or `base` if that is later.
    Comparing `base` and `tip` alone would bring in another lane's changes
    merged from main in between."""
    revs = [f"{base}..{tip}", *published(remote, scratch)]
    names = git("log", "--no-renames", "--format=", "--name-only", "--diff-merges=dense-combined",
                *revs, "--", "*.rs", cwd=scratch)
    files = sorted({f for f in names.splitlines() if f.strip() and (scratch / f).is_file()})
    main_ref = f"refs/remotes/{remote}/main"
    since = base
    if resolve(main_ref, cwd=scratch, check=False):
        mb = git("merge-base", main_ref, tip, cwd=scratch, check=False).strip()
        if mb and is_ancestor(base, mb, scratch):
            since = mb
    return files, since


def is_ancestor(a: str, b: str, cwd: str | Path | None = None) -> bool:
    """Whether commit `a` is `b` or one of its ancestors."""
    return subprocess.run(["git", "merge-base", "--is-ancestor", a, b], cwd=cwd,
                          env=gitenv.clean_env(), capture_output=True,
                          stdin=subprocess.DEVNULL).returncode == 0


def mutation_round(top: Path, scratch: Path, base: str, tip: str, stamp: str,
                   budget: int, remote: str = "origin") -> str | None:
    """Mutate the batch's changed Rust lines in `scratch` with cargo-mutants,
    one package at a time, for at most `budget` seconds in all, at idle
    priority; keep its output and results in the reports directory and
    answer the `Review-mutants` trailer's value (None when there was nothing
    to mutate, or no cargo-mutants)."""
    try:
        have = subprocess.run(["cargo", "mutants", "--version"], capture_output=True,
                              stdin=subprocess.DEVNULL).returncode == 0
    except OSError:
        have = False
    if not have:
        print("review-batch: cargo-mutants is not installed (cargo install cargo-mutants "
              "--locked); no mutation testing this time", flush=True)
        return None
    files, since = batch_rust_files(scratch, base, tip, remote)
    groups = owning_workspaces(scratch, files)
    jobs = []
    for rel, packages in sorted(groups.items()):
        for p in sorted(packages):
            if p in NO_HOST_TESTS:
                print(f"review-batch: no mutation testing for {p}: its tests do not run on "
                      "the host", flush=True)
            else:
                jobs.append((rel, p))
    if not jobs:
        return None
    dest = reports_dir(top) / f"{stamp}-{tip[:12]}-mutants"
    deadline = time.monotonic() + budget
    runner = Path(__file__).resolve().parent / "run-timeout.py"
    tested = found = unviable = 0
    missed: list[str] = []
    failed: list[str] = []
    for rel, package in jobs:
        left = int(deadline - time.monotonic())
        if left < 60:
            failed.append(f"{package} (no time left)")
            continue
        ws = scratch / rel if rel else scratch
        diff = git("diff", "--no-renames", "--no-ext-diff", "--no-color", "--src-prefix=a/",
                   "--dst-prefix=b/", *([f"--relative={rel}/"] if rel else []), since, tip, "--",
                   *(f for f in files if not rel or f.startswith(rel + "/")), cwd=scratch)
        out = dest / package
        out.mkdir(parents=True, exist_ok=True)
        diff_file = out / "batch.diff"
        diff_file.write_text(diff, encoding="utf-8", newline="\n")
        cmd = ["cargo", "mutants", "-d", str(ws), "--in-place", "--in-diff", str(diff_file),
               "--shuffle", "--timeout", "300", "-o", str(out)]
        if os.name == "nt":
            # The workspace's own config builds for bare metal.
            cmd.append("--cargo-arg=--target=x86_64-pc-windows-gnu")
        if not rel:
            cmd += ["-p", package]
        print(f"review-batch: mutation testing {package}, at most {left // 60} minutes",
              flush=True)
        r = subprocess.run([sys.executable, str(runner), str(left), *cmd], cwd=scratch,
                           env=gitenv.clean_env(), capture_output=True, text=True,
                           encoding="utf-8", errors="replace", stdin=subprocess.DEVNULL,
                           creationflags=IDLE_PRIORITY_CLASS if os.name == "nt" else 0)
        (out / "output.txt").write_text(r.stdout + r.stderr, encoding="utf-8", newline="\n")
        t, m, f, u = mutants_tally(out)
        tested, found, unviable = tested + t, found + f, unviable + u
        missed += [f"{rel + '/' if rel else ''}{x}" for x in m]
        if r.returncode not in MUTANTS_FINISHED:
            why = "out of time" if r.returncode == 124 else f"exit {r.returncode}"
            failed.append(f"{package} ({why}; {out / 'output.txt'})")
    if not found and not failed:
        return None
    print(f"review-batch: mutants: {tested} of {found} tested against the tests "
          f"({unviable} did not build), {len(missed)} not caught by any test -- each is a "
          f"finding to triage: add the test that catches it, or say why the change cannot "
          f"matter. Results: {dest}", flush=True)
    for x in missed:
        print(f"  missed: {x}")
    for x in failed:
        print(f"  not finished: {x}")
    return (f"{len(missed)} missed of {tested} tested ({found} found"
            + (f"; not finished: {', '.join(x.split(' (')[0] for x in failed)}" if failed else "")
            + f") {dest.name}")


def effort_timeline(transcript: Path) -> list[tuple[float, str]]:
    """(time, effort) for every request a session's transcript records, in
    order: Claude Code writes each request's `perTurnEffort` there."""
    out = []
    try:
        with open(transcript, encoding="utf-8", errors="replace") as f:
            for line in f:
                if '"perTurnEffort"' not in line:
                    continue
                e = re.search(r'"perTurnEffort"\s*:\s*"(\w+)"', line)
                t = re.search(r'"timestamp"\s*:\s*"([^"]+)"', line)
                if e and t:
                    try:
                        when = dt.datetime.fromisoformat(t.group(1).replace("Z", "+00:00"))
                    except ValueError:
                        continue
                    out.append((when.timestamp(), e.group(1)))
    except OSError:
        return []
    out.sort()
    return out


def batch_written_at(cs: list[Commit], timeline: list[tuple[float, str]], fallback: str) -> str:
    """The effort level the batch's commits were made at: for each commit,
    the level of the session's last request before it. One level when every
    commit has one and they agree; `mixed` when they differ, or when the
    transcript covers only some of them (the rest were made by another
    session, at a level it cannot say); `fallback` when it covers none.
    `mixed` carries no counts, so that `--stats` groups all such batches."""
    import bisect
    times = [t for t, _ in timeline]
    seen: set[str] = set()
    uncovered = 0
    for c in cs:
        if len(c.parents) > 1:
            continue
        try:
            when = dt.datetime.fromisoformat(c.date).timestamp()
        except ValueError:
            uncovered += 1
            continue
        i = bisect.bisect_right(times, when) - 1
        # A request more than six hours before the commit is not the one
        # that made it: the session was elsewhere, or the commit is older.
        if i >= 0 and when - times[i] < 6 * 3600:
            seen.add(timeline[i][1])
        else:
            uncovered += 1
    if not seen:
        return fallback
    return next(iter(seen)) if len(seen) == 1 and not uncovered else "mixed"


def session_transcript() -> Path | None:
    """This Claude Code session's transcript, if this runs inside one."""
    sid = os.environ.get("CLAUDE_CODE_SESSION_ID")
    config = os.environ.get("CLAUDE_CONFIG_DIR") or str(Path.home() / ".claude")
    if not sid:
        return None
    found = list(Path(config).glob(f"projects/*/{sid}.jsonl"))
    return found[0] if found else None


def review(rng: str, remote: str, dry_run: bool, qemu_wait: int, written_at: str | None,
           mutants_budget: int = MUTANTS_BUDGET) -> int:
    top = Path(git("rev-parse", "--show-toplevel").strip())
    base_rev, _, tip_rev = rng.partition("..")
    if not tip_rev:
        raise SystemExit("review-batch: give the batch as <base>..<tip>")
    base, tip = resolve(base_rev), resolve(tip_rev)
    tier, why, cs = batch_tier(f"{base}..{tip}", remote)
    print(f"review-batch: {len(cs)} commits, {base[:10]}..{tip[:10]}: tier {tier}")
    for w in why[:20]:
        print(f"  {w}")
    if tier == "none":
        print("review-batch: no code changes; the rule reviews nothing here")
        return 0
    cmds = review_commands(base, tip, tier)
    if dry_run:
        for _, _, argv in cmds:
            print("would run: " + " ".join(argv))
        return 0

    wait_for_no_qemu(qemu_wait)
    stamp = dt.datetime.now().strftime("%Y%m%dT%H%M%S")
    scratch = top.parent / f"{top.name}-review-{tip[:10]}-{stamp}"
    git("worktree", "prune", cwd=top, check=False)
    git("worktree", "add", "--detach", str(scratch), tip, cwd=top)
    runs: list[str] = []
    try:
        for model, level, argv in cmds:
            name = f"{stamp}-{tip[:12]}-{model}-{level}.md"
            report = reports_dir(top) / name
            header = (f"review of {base}..{tip}\nreviewer: {model} at {level}\n"
                      f"started: {dt.datetime.now().isoformat(timespec='seconds')}\n")
            print(f"review-batch: {model} at {level} -- started", flush=True)
            code, out, err = run_review(argv, scratch, report, header)
            findings = findings_of(out) if code == 0 else None
            if findings is None and (model, level) in OPTIONAL_RUNS and LIMIT_SPENT.search(out):
                runs.append(f"{model}-{level} unavailable")
                print(f"review-batch: {model} at {level}: its allowance is spent "
                      f"({why_not_reviewed(code, out, err)}); the record says so, and the "
                      f"other review stands alone -- see {report}", flush=True)
            elif findings is None:
                print(f"review-batch: {model} at {level} FAILED (exit {code}: "
                      f"{why_not_reviewed(code, out, err)}); no record -- see {report}. "
                      "Run the batch again.", flush=True)
                return 1
            else:
                runs.append(f"{model}-{level} {name}")
                print(f"review-batch: {model} at {level} -- done, {len(findings)} "
                      f"finding(s); report {report}", flush=True)
                print(json.dumps(findings, indent=1, ensure_ascii=False), flush=True)
        if written_at is None:
            # The level the commits were made at, from this session's own
            # transcript; the session's level now only if that cannot say.
            transcript = session_transcript()
            written_at = batch_written_at(
                cs, effort_timeline(transcript) if transcript else [],
                os.environ.get("CLAUDE_EFFORT", "unknown"))
        # The record first: the reviews are done and paid for, and nothing
        # the mutation round does may lose it.
        print("\nTrailers for the follow-up commit (fill in the findings after triage):\n")
        print(f"Reviewed: {base}..{tip}")
        print(f"Review-tier: {tier}")
        print(f"Review-runs: {', '.join(runs)}")
        print(f"Review-lane: {lane_letter(top)}")
        print(f"Review-written-at: {written_at}")
        print("Review-findings: <n> fixed, <m> dismissed\n", flush=True)
        if mutants_budget > 0:
            try:
                if qemu_running():
                    print("review-batch: a QEMU guest is up; mutation testing waits for it",
                          flush=True)
                    wait_for_no_qemu(qemu_wait)
                mutants = mutation_round(top, scratch, base, tip, stamp, mutants_budget, remote)
                if mutants:
                    print(f"Add to the trailers:\nReview-mutants: {mutants}", flush=True)
            except (Exception, SystemExit) as e:  # noqa: BLE001 -- the record stands
                print(f"review-batch: no mutation testing this time ({e}); the record above "
                      "stands", flush=True)
    finally:
        remove_scratch(top, scratch)
    return 0


# ---------------------------------------------------------------------------
# --check: the pre-push gate
# ---------------------------------------------------------------------------

def check(shas: list[str], remote: str, cwd: str | Path | None = None,
          reports: Path | None = None, start: str = RECORDS_START) -> tuple[int, list[str]]:
    """Whether every commit `shas` would add to `<remote>/main` is reviewed:
    (exit status, what is wrong)."""
    cs = commits(*shas, *published(remote, cwd), cwd=cwd)
    reports = reports if reports is not None else reports_dir(cwd)
    covered: list[tuple[Record, set[str]]] = []
    exempt: set[str] = set()
    problems: list[str] = []
    fuzzed = fuzzed_crates(cwd)
    for c in cs:
        r = record_of(c)
        if not r:
            continue
        exempt.add(c.sha)
        wrong = record_problems(r, reports, cwd)
        if wrong:
            problems += wrong
            continue
        inside = set(git("rev-list", f"{r.base}..{r.tip}", cwd=cwd).split())
        covered.append((r, inside))
        # The follow-up fixes: after the reviewed tip, up to the record.
        exempt |= set(git("rev-list", "--ancestry-path", f"{r.tip}..{r.sha}",
                          cwd=cwd, check=False).split())
    for c in cs:
        if c.date[:10] < start or c.sha in exempt:
            continue
        need, why = c.tier(fuzzed)
        if need == "none":
            continue
        if not any(c.sha in inside and TIERS.index(r.tier) >= TIERS.index(need)
                   for r, inside in covered):
            reason = f" ({why[0]})" if why else ""
            problems.append(f"{c.sha[:10]} needs a review at {need}{reason}: {c.subject()}")
    return (1 if problems else 0), problems


# ---------------------------------------------------------------------------
# --stats
# ---------------------------------------------------------------------------

def code_lines(base: str, tip: str, cwd: str | Path | None = None) -> int:
    n = 0
    for line in git("diff", "--numstat", "--no-renames", base, tip, cwd=cwd,
                    check=False).splitlines():
        parts = line.split("\t")
        if len(parts) == 3 and parts[0].isdigit() and is_code(parts[2]):
            n += int(parts[0]) + int(parts[1])
    return n


def stats(rev: str, since: str, cwd: str | Path | None = None) -> list[tuple]:
    """(lane, written-at, reviews, code lines, fixed, dismissed) for the
    records on `rev` since `since`, grouped by lane and writing level."""
    groups: dict[tuple[str, str], list[int]] = {}
    out = git("log", f"--since={since}", "--grep=^Reviewed: ", f"--format={SEP}%H%n%B",
              rev, cwd=cwd)
    for piece in out.split(SEP)[1:]:
        sha, _, message = piece.partition("\n")
        r = record_of(Commit(sha, [], "", message))
        if not r or r.fixed is None:
            continue
        g = groups.setdefault((r.lane, r.written_at), [0, 0, 0, 0])
        g[0] += 1
        g[1] += code_lines(r.base, r.tip, cwd=cwd)
        g[2] += r.fixed
        g[3] += r.dismissed or 0
    return [(lane, at, *v) for (lane, at), v in sorted(groups.items())]


def print_stats(rows: list[tuple]) -> None:
    print(f"{'lane':4} {'written at':10} {'reviews':>7} {'code lines':>10} "
          f"{'fixed':>6} {'dismissed':>9} {'fixed/1000 lines':>16}")
    for lane, at, n, lines, fixed, dismissed in rows:
        rate = f"{1000 * fixed / lines:.2f}" if lines else "-"
        print(f"{lane:4} {at:10} {n:7} {lines:10} {fixed:6} {dismissed:9} {rate:>16}")


# ---------------------------------------------------------------------------
# --self-test
# ---------------------------------------------------------------------------

def self_test() -> int:
    failures: list[str] = []

    def expect(what: str, got, want) -> None:
        if got != want:
            failures.append(f"{what}: got {got!r}, want {want!r}")

    def change(added: list[str], context: list[str] | None = None) -> Change:
        ch = Change()
        ch.added, ch.context = added, context or []
        return ch

    def tier(path: str, added: list[str], context: list[str] | None = None) -> str:
        return judge({path: change(added, context)})[0]

    expect("docs only", tier("README.md", ["text"]), "none")
    expect("plain code", tier("apps/x/src/lib.rs", ["fn f() {}"]), "xhigh")
    expect("kernel path", tier("kernel/src/a.rs", ["fn f() {}"]), "max")
    expect("sudo path", tier("userspace/sudo/src/main.rs", ["fn f() {}"]), "max")
    expect("unsafe added", tier("apps/x/src/lib.rs", ["    unsafe { f() }"]), "max")
    expect("unsafe in a comment", tier("apps/x/src/lib.rs", ["// not unsafe"]), "xhigh")
    expect("block comment line", tier("apps/x/src/lib.rs", [" * unsafe here"]), "xhigh")
    expect("a deref that adds unsafe",
           tier("apps/x/src/lib.rs", ["*dst = unsafe { p.read() };"]), "max")
    expect("forbid attribute", tier("apps/x/src/lib.rs", ["#![forbid(unsafe_code)]"]), "xhigh")
    expect("unsafe attribute", tier("apps/x/src/lib.rs", ["#[unsafe(no_mangle)]"]), "max")
    expect("C define", tier("posixish/x.c", ["#define RM(p) unlink(p)"]), "max")
    expect("atomics", tier("apps/x.rs", ["x.load(Ordering::Acquire)"]), "max")
    expect("rmtree in a script", tier("scripts/t.py", ["shutil.rmtree(p)"]), "max")
    expect("rm -rf in a hook", tier("scripts/hooks/pre-push", ['rm -rf "$wt"']), "max")
    expect("Remove-Item", tier("scripts/x.ps1", ["Remove-Item -Recurse $p"]), "max")
    expect("a word inside another", tier("apps/x.rs", ["let unsafety = 1;"]), "xhigh")
    expect("a parser that left the kernel", tier("elfparse/src/lib.rs", ["fn f() {}"]), "max")

    # A review counts only if it ends with its findings array. Every failure
    # seen so far exits 0 with a message instead.
    tick = "`" * 3
    finding = '[{"file": "a.rs", "line": 3, "summary": "off by one"}]'
    expect("no findings", findings_of(f"Looks fine.\n\n{tick}json\n[]\n{tick}\n"), [])
    expect("one finding", len(findings_of(f"x\n{tick}json\n{finding}\n{tick}\n") or []), 1)
    expect("the last array wins", findings_of(
        f"{tick}json\n{finding}\n{tick}\nthen\n{tick}json\n[]\n{tick}\n"), [])
    expect("an object is not the findings", findings_of(f"{tick}json\n{{}}\n{tick}\n"), None)
    expect("an unfinished array is not the findings",
           findings_of(f"{tick}json\n[1,\n{tick}\n"), None)
    # A review that finished is not thrown away for a slip in its JSON.
    expect("the fence in capitals", findings_of(f"{tick}JSON\n[]\n{tick}\n"), [])
    slip = '[{"summary": "matches \\s*(\\d+) and \\\\d", "file": "a\tb.rs"}]'
    expect("a backslash that starts no escape, a raw tab",
           (findings_of(f"{tick}json\n{slip}\n{tick}\n") or [{}])[0].get("summary"),
           "matches \\s*(\\d+) and \\d")
    expect("an array that will not parse is one finding saying so",
           len(findings_of(f'{tick}json\n[{{"a": 1}} {{"b": 2}}]\n{tick}\n') or []), 1)
    expect("a bare [] as the last word", findings_of("Nothing found.\n[]\n"), [])
    for failed in ("Credit balance is too low",
                   "You've hit your session limit · resets 1:10pm (America/New_York)",
                   "API Error: Claude's response exceeded the 64000 output token maximum.",
                   "Fable limit reached", ""):
        expect(f"not a review: {failed[:30]!r}", findings_of(failed), None)
    expect("only Fable's own limit lets a record go without it",
           [bool(LIMIT_SPENT.search(s)) for s in (
               "Fable limit reached", "Continue on usage credits?",
               "You've hit your session limit", "API Error: Claude's response exceeded")],
           [True, True, False, False])
    expect("why it did not review", why_not_reviewed(
        0, "[run-timeout] launching\nYou've hit your session limit\n"
        "[run-timeout] child exited: PASS\n",
        "Ignoring 11 permissions.allow entries\nclaude.ai connectors are disabled\n"),
        "You've hit your session limit")
    expect("an error only on stderr", why_not_reviewed(
        1, "[run-timeout] child exited: FAIL\n", "Error: invalid model\n"),
        "Error: invalid model")
    expect("out of time", why_not_reviewed(124, "[run-timeout] TIMEOUT\n", "warning\n"),
           f"stopped after {REVIEW_TIMEOUT} s")
    expect("inside existing unsafe",
           tier("apps/x.rs", ["    p.add(i + 1)"], ["unsafe fn f(p: *const u8) {"]), "max")
    expect("unsafe nearby, nothing added", tier("apps/x.rs", [], ["unsafe fn f() {"]), "xhigh")

    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp) / "repo"
        repo.mkdir()
        reports = Path(tmp) / "reports"
        reports.mkdir()

        def run(*args: str, date: str = "2026-10-13T10:00:00") -> str:
            env = gitenv.clean_env()
            env.update(GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date,
                       GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t",
                       GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@t")
            r = subprocess.run(["git", *args], cwd=repo, env=env, capture_output=True,
                               text=True, encoding="utf-8", stdin=subprocess.DEVNULL)
            if r.returncode:
                raise SystemExit(f"self-test: git {args}: {r.stderr}")
            return r.stdout.strip()

        def write(path: str, text: str) -> None:
            f = repo / path
            f.parent.mkdir(parents=True, exist_ok=True)
            with open(f, "a", encoding="utf-8", newline="\n") as h:
                h.write(text + "\n")

        def commit(path: str, text: str, msg: str, date: str = "2026-10-13T10:00:00") -> str:
            write(path, text)
            run("add", "--", path)
            run("commit", "-q", "-m", msg, date=date)
            return run("rev-parse", "HEAD")

        def report(name: str, batch: str, reviewer: str,
                   said: str = "Review done.\n\n```json\n[]\n```\n") -> str:
            """A report of `reviewer` (`opus at max`) reviewing `batch`."""
            (reports / name).write_text(f"review of {batch}\nreviewer: {reviewer}\n"
                                        f"started: now\nexit status: 0\n\n{said}",
                                        encoding="utf-8", newline="\n")
            return name

        def record(base: str, tip: str, tier: str, runs: str, extra: str = "") -> str:
            return (f"review fixes\n\nReviewed: {base}..{tip}\nReview-tier: {tier}\n"
                    f"Review-runs: {runs}\nReview-lane: F\nReview-written-at: max\n"
                    f"Review-findings: 2 fixed, 1 dismissed{extra}")

        def refused(problems: list[str]) -> list[str]:
            return sorted(p.split()[0] for p in problems)

        run("init", "-q", "-b", "main")
        root = commit("README.md", "hello", "docs")
        run("update-ref", "refs/remotes/origin/main", root)
        old = commit("apps/a.rs", "fn old() {}", "before records", date="2026-10-01T10:00:00")
        docs = commit("notes.md", "more", "docs only")
        plain = commit("apps/b.rs", "fn b() {}", "plain")
        kern = commit("kernel/k.rs", "fn k() {}", "kernel")
        tip = commit("apps/c.rs", "fn c() { unsafe { x() } }", "unsafe")
        expect("prose record ignored",
               record_of(Commit("x", [], "", "m\n\nReviewed: 69a27c0ab by Opus")), None)

        status, problems = check([tip], "origin", cwd=repo, reports=reports)
        expect("unreviewed: which are refused", refused(problems),
               sorted(s[:10] for s in (plain, kern, tip)))
        expect("old and docs pass", any(old[:10] in p or docs[:10] in p for p in problems),
               False)

        batch = f"{root}..{tip}"
        rec_x = commit("apps/b.rs", "// fix", record(
            root, tip, "xhigh", f"opus-xhigh {report('r1.md', batch, 'opus at xhigh')}"))
        status, problems = check([rec_x], "origin", cwd=repo, reports=reports)
        expect("xhigh record: max commits still refused", refused(problems),
               sorted(s[:10] for s in (kern, tip)))

        commit("apps/c.rs", "fn fix1() {}", "first fix, no record")
        rec_m = commit("apps/b.rs", "// fix 2", record(
            root, tip, "max",
            f"opus-max {report('r2.md', batch, 'opus at max')}, fable-xhigh unavailable",
            "\nCo-Authored-By: someone <x@y>"))
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("max record covers all, fixes split over two commits", (status, problems),
               (0, []))

        (reports / "r2.md").unlink()
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("a missing report is named", any("r2.md" in p for p in problems), True)
        report("r2.md", batch, "opus at max", "Credit balance is too low\n")
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("a report of a review that never ran is refused",
               any("r2.md" in p and "no finished review" in p for p in problems), True)
        report("r2.md", f"{root}..{kern}", "opus at max")
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("a report of another batch is refused",
               any("r2.md" in p and "another review" in p for p in problems), True)
        report("r2.md", batch, "opus at xhigh")
        status, problems = check([rec_m], "origin", cwd=repo, reports=reports)
        expect("a report by another reviewer is refused",
               any("r2.md" in p and "another review" in p for p in problems), True)
        report("r2.md", batch, "opus at max")
        escape = Record("e" * 40, {"Reviewed": batch, "Review-tier": "max",
                                   "Review-runs": "opus-max ../reports/r2.md, "
                                                  "fable-xhigh unavailable"})
        expect("a report outside the reviews directory is refused",
               any("outside the reviews directory" in p
                   for p in record_problems(escape, reports, repo)), True)

        later = commit("apps/d.rs", "fn d() {}", "new work")
        status, problems = check([later], "origin", cwd=repo, reports=reports)
        expect("work after the record is refused", refused(problems), [later[:10]])

        bad = commit("apps/e.rs", "fn e() {}", record(later, later, "max", "opus-max unavailable"))
        status, problems = check([bad], "origin", cwd=repo, reports=reports)
        expect("Opus may not be unavailable", any("no opus review" in p for p in problems), True)

        # A merge whose resolution adds code: what the resolution adds is
        # judged, and what it keeps of one side is that side's, not new.
        run("checkout", "-q", "-b", "side", root)
        commit("apps/m.rs", "fn side() { std::fs::remove_file(p) }", "side")
        run("checkout", "-q", "main")
        commit("apps/m.rs", "fn main_side() {}", "main side")
        r = subprocess.run(["git", "merge", "-q", "side", "-m", "merge"], cwd=repo,
                           env=gitenv.clean_env(), capture_output=True, text=True,
                           stdin=subprocess.DEVNULL)
        with open(repo / "apps/m.rs", "w", encoding="utf-8", newline="\n") as h:
            h.write("fn side() { std::fs::remove_file(p) }\nstatic mut R: u8 = 0;\n")
        run("add", "apps/m.rs")
        run("commit", "-q", "--no-edit")
        mc = commits("HEAD^!", cwd=repo)
        t, why = mc[0].tier() if mc else ("", [])
        expect("conflicted merge: its resolution judged",
               (r.returncode != 0, len(mc), t, "static mut" in " ".join(why[:1])),
               (True, 1, "max", True))

        # Paths with a space, and a pure rename.
        commit("apps/x y/src/lib.rs", "fn s() {}", "space")
        expect("a path with a space is code", commits("HEAD^!", cwd=repo)[0].tier()[0], "xhigh")
        run("mv", "apps/b.rs", "apps/b2.rs")
        run("commit", "-q", "-m", "rename")
        expect("a rename is a change", commits("HEAD^!", cwd=repo)[0].tier()[0], "xhigh")

        rows = stats("HEAD", "2026-01-01", cwd=repo)
        lines = code_lines(root, tip, cwd=repo)
        # Three records: two of root..tip, and one of an empty range.
        expect("stats", rows, [("F", "max", 3, 2 * lines, 6, 3)])

        # A crate that keeps a fuzz harness is a parser of untrusted input.
        commit("parse/fuzz/harness.toml", "[package]", "harness")
        commit("parse/src/fuzz/mod.rs", "// a module, not a harness", "module")
        expect("crates with a harness", fuzzed_crates(repo), ("parse/",))
        expect("a fuzzed crate is reviewed at max", judge(
            {"parse/src/lib.rs": change(["fn f() {}"], [])}, fuzzed_crates(repo))[0], "max")

    # Which workspace each changed file is mutation-tested from: the tree's
    # own for its members, its own for a crate with a [workspace] of its own.
    with tempfile.TemporaryDirectory() as tmp:
        tree = Path(tmp)
        (tree / "Cargo.toml").write_text('[workspace]\nmembers = ["a"]\n', encoding="utf-8", newline="\n")
        for crate, extra in (("a", ""), ("solo", "\n[workspace]\n")):
            (tree / crate / "src").mkdir(parents=True)
            (tree / crate / "src" / "lib.rs").write_text("", encoding="utf-8", newline="\n")
            (tree / crate / "Cargo.toml").write_text(
                f'[package]\nname = "{crate}"\nversion = "0.0.0"\nedition = "2021"\n{extra}',
                encoding="utf-8", newline="\n")
        expect("owning workspaces", owning_workspaces(tree, ["a/src/lib.rs", "solo/src/lib.rs"]),
               {"": {"a"}, "solo": {"solo"}})
        mo = tree / "out" / "mutants.out"
        mo.mkdir(parents=True)
        (mo / "mutants.json").write_text(json.dumps([{}] * 9), encoding="utf-8", newline="\n")
        (mo / "caught.txt").write_text("x\ny\n", encoding="utf-8", newline="\n")
        (mo / "missed.txt").write_text("src/a.rs:1:1: replace f with g\n", encoding="utf-8", newline="\n")
        (mo / "unviable.txt").write_text("u\n", encoding="utf-8", newline="\n")
        expect("mutants tally", mutants_tally(tree / "out"),
               (3, ["src/a.rs:1:1: replace f with g"], 9, 1))

    # A batch that merged main: only its own Rust files are mutated, diffed
    # from the main it merged, not another lane's change merged in between.
    with tempfile.TemporaryDirectory() as tmp:
        repo = Path(tmp)

        def g(*args: str) -> str:
            env = gitenv.clean_env()
            env.update(GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@t", GIT_COMMITTER_NAME="t",
                       GIT_COMMITTER_EMAIL="t@t")
            r = subprocess.run(["git", *args], cwd=repo, env=env, capture_output=True,
                               text=True, encoding="utf-8", stdin=subprocess.DEVNULL)
            if r.returncode:
                raise SystemExit(f"self-test: git {args}: {r.stderr}")
            return r.stdout.strip()

        def put(name: str, text: str) -> str:
            (repo / name).write_text(text, encoding="utf-8", newline="\n")
            g("add", name)
            g("commit", "-q", "-m", name)
            return g("rev-parse", "HEAD")

        g("init", "-q", "-b", "lane")
        root = put("a.rs", "fn a() {}\n")
        g("update-ref", "refs/remotes/origin/main", root)
        put("lane.rs", "fn lane() {}\n")
        g("checkout", "-q", "-b", "mainline", root)
        other = put("m.rs", "fn other_lane() {}\n")
        g("update-ref", "refs/remotes/origin/main", other)
        g("checkout", "-q", "lane")
        g("merge", "-q", "--no-edit", "mainline")
        tip = put("lane2.rs", "fn lane2() {}\n")
        files, since = batch_rust_files(repo, root, tip, "origin")
        expect("batch's own Rust files", (files, since), (["lane.rs", "lane2.rs"], other))

    # The level a batch was written at: the session's last request before
    # each commit, from its transcript.
    with tempfile.TemporaryDirectory() as tmp:
        transcript = Path(tmp) / "s.jsonl"
        with open(transcript, "w", encoding="utf-8", newline="\n") as f:
            for when, effort in (("2026-10-11T10:00:00.000Z", "max"),
                                 ("2026-10-11T12:00:00.000Z", "max"),
                                 ("2026-10-12T09:00:00.000Z", "xhigh")):
                f.write(json.dumps({"type": "assistant", "timestamp": when,
                                    "perTurnEffort": effort}) + "\n")
            f.write('{"type": "user", "timestamp": "2026-10-12T09:30:00.000Z"}\n')
        tl = effort_timeline(transcript)
        expect("timeline", [e for _, e in tl], ["max", "max", "xhigh"])

        def made(*dates: str) -> list[Commit]:
            return [Commit(f"{i:040x}", ["p"], d, "m") for i, d in enumerate(dates)]

        expect("all at max", batch_written_at(made("2026-10-11T12:30:00+00:00"), tl, "fb"), "max")
        expect("mixed", batch_written_at(made("2026-10-11T10:05:00+00:00",
                                              "2026-10-12T09:10:00+00:00"), tl, "fb"),
               "mixed")
        expect("some made by another session", batch_written_at(
            made("2026-10-01T00:00:00+00:00", "2026-10-12T09:10:00+00:00"), tl, "fb"), "mixed")
        expect("before the transcript", batch_written_at(made("2026-10-01T00:00:00+00:00"),
                                                         tl, "fb"), "fb")
        expect("long after the last request", batch_written_at(
            made("2026-10-13T09:00:00+00:00"), tl, "fb"), "fb")

    cmds = review_commands("a" * 40, "b" * 40, "max")
    expect("max runs", [(m, lv) for m, lv, _ in cmds], [("opus", "max"), ("fable", "xhigh")])
    expect("max prompt", cmds[0][2][-1], f"/code-review max commits {'a' * 40}..{'b' * 40}")

    for f in failures:
        print(f"FAIL {f}")
    print(f"review-batch self-test: {'FAILED' if failures else 'ok'}")
    return 1 if failures else 0


# ---------------------------------------------------------------------------

def main(argv: list[str]) -> int:
    # Reviewers write arrows and dashes; a redirected stdout is cp1252 here.
    for stream in (sys.stdout, sys.stderr):
        stream.reconfigure(encoding="utf-8", errors="replace")
    if selftestflag.wants_selftest(argv):
        return self_test()
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("range", nargs="*", help="<base>..<tip>, or for --check the shas pushed")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--tier", action="store_true", help="only print the tier and why")
    mode.add_argument("--check", action="store_true", help="the pre-push gate for main")
    mode.add_argument("--stats", action="store_true", help="findings per changed line")
    ap.add_argument("--remote", default="origin",
                    help="the remote whose main is what is already published")
    ap.add_argument("--since", default=RECORDS_START, help="--stats: records from this date")
    ap.add_argument("--dry-run", action="store_true", help="print the reviews; run none")
    ap.add_argument("--qemu-wait", type=int, default=4 * 3600,
                    help="seconds to wait for a QEMU guest to finish before reviewing")
    ap.add_argument("--mutants-budget", type=int, default=MUTANTS_BUDGET,
                    help="seconds of mutation testing after the reviews; 0 for none")
    ap.add_argument("--written-at", default=None,
                    help="the effort level the batch was written at (default: read from "
                         "this session's transcript at each commit's time, else "
                         "CLAUDE_EFFORT)")
    a = ap.parse_args(argv)
    if a.stats:
        print_stats(stats(f"refs/remotes/{a.remote}/main", a.since))
        return 0
    if a.check:
        if not a.range:
            return 0
        status, problems = check(a.range, a.remote)
        if problems:
            print("pre-push: REFUSING to publish commits no review covers at their tier:",
                  file=sys.stderr)
            for p in problems:
                print(f"  {p}", file=sys.stderr)
            print("Review the batch with `python scripts/review-batch.py <base>..<tip>` "
                  "and put the trailers it prints in the follow-up commit.", file=sys.stderr)
        return status
    if len(a.range) != 1:
        ap.error("give one range, <base>..<tip>")
    if a.tier:
        tier, why, cs = batch_tier(a.range[0], a.remote)
        print(f"{tier}  ({len(cs)} commits)")
        for w in why:
            print(f"  {w}")
        return 0
    return review(a.range[0], a.remote, a.dry_run, a.qemu_wait, a.written_at,
                  a.mutants_budget)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
