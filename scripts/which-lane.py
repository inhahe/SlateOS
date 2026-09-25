#!/usr/bin/env python3
"""Print which of the six parallel-agent lanes this session is, and what it owns.

Six Claude sessions work this repo at once -- two per Claude account -- and
each owns a disjoint set of paths (see roadmap.md -> "Six-Agent Parallel
Execution").  An agent must be able to answer "which one am I?" without asking
the operator, and every script that acts on "my lane" must get the same answer
this one gives.  This file is also the single ownership table: other scripts
import `owner_of` / `OWNERSHIP` / `LANES` from it rather than keeping copies,
because every copy that existed under the three-lane split drifted.

WHAT IDENTIFIES A LANE
----------------------
The operator starts each session under an agent name, "Lane A" .. "Lane F"
(orchestrator2's `--agent-name Lane-D`, or `/rename Lane D` in an open
session; `ListAgents` shows it on its first line: "This session is Lane-D
[...]").  That name is the identity.  Spell it with a hyphen when it is given
with `--agent-name`: orchestrator2's agent registry refuses a space, and a
session it refuses is outside the registry -- it receives no halts.  Nothing
carries the name into a subprocess automatically -- orchestrator2 does not
export it -- so the lane is accepted from four places:

  1. `--agent-name NAME` or `--lane X` on the command line: the agent passes the
     name `ListAgents` showed it;
  2. `SLATEOS_LANE` in the environment (a letter, or a name like `Lane D`);
  3. `ORCH2_AGENT_NAME` in the environment, if anything has put it there.
     orchestrator2 does not: the variable is its fallback for `--agent-name` on
     the *launch*, which consumes it, so an agent's own processes do not see it
     (a session opened in a hub that was already running never did);
  4. the worktree this copy of the script lives in: `os-lane-d/scripts/` answers
     D, provided that worktree really has `lane-d` checked out.

Every source that is present must agree.  Two that disagree make the answer
UNKNOWN (exit 2) rather than letting the "stronger" one win: a session named
Lane D running a script out of `os-lane-a` is about to write into lane A's
worktree, and that is the one failure this whole arrangement exists to prevent.
Likewise a worktree whose directory says `os-lane-d` but whose HEAD is
`lane-a` -- someone ran `git checkout` inside it -- is refused, not guessed at.

WHAT NO LONGER IDENTIFIES A LANE
--------------------------------
`CLAUDE_CONFIG_DIR`.  Under three lanes each Claude account ran exactly one
session, so the account's config directory named the lane, and this script
used to read nothing else.  With two sessions per account it names two lanes;
a detector that kept reading it would give both sessions the same answer, and
both would believe it.  It is printed for information only.

Usage::

    python scripts/which-lane.py                         # briefing for this session
    python scripts/which-lane.py --agent-name "Lane-D"   # ... given the ListAgents name
    python scripts/which-lane.py --letter                # just the letter, for scripting
    python scripts/which-lane.py --owner gui/window/src/lib.rs posix/src/unistd.rs
    python scripts/which-lane.py --table                 # the whole ownership table
    python scripts/which-lane.py --self-test

Exit status: 0 when the lane was identified (or for `--owner` / `--table`),
2 when it could not be.  On 2, *stop and ask the operator* rather than
guessing -- writing outside your lane is the one failure mode that silently
destroys another agent's work.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from pathlib import Path, PurePosixPath
from typing import Callable, Mapping

PROJECT_ROOT = Path(__file__).resolve().parent.parent

#: The integration checkout: `main`, merges only, nobody's lane.
INTEGRATION_WORKTREE = "os"


class Lane:
    """One lane: who it is, where it works, and what it is for.

    A plain class, deliberately not a `@dataclass`.  Other scripts load this
    file by path (`importlib.util.spec_from_file_location`, because the name has
    a hyphen) without registering it in `sys.modules`, and under
    `from __future__ import annotations` the dataclass machinery looks the
    module up there and crashes -- which took down every importer the first
    time this was written.  The self-test loads the module that way to keep it
    from coming back.
    """

    __slots__ = ("letter", "name", "summary", "split_note")

    def __init__(self, letter: str, name: str, summary: str,
                 split_note: str) -> None:
        self.letter = letter
        self.name = name
        #: One sentence on the lane's domain, for the briefing.
        self.summary = summary
        #: What changed for this lane in the six-lane split, printed until
        #: nobody needs reminding.  Empty for a lane with nothing to hand over.
        self.split_note = split_note

    def __repr__(self) -> str:
        return f"Lane({self.letter!r}, {self.name!r})"

    @property
    def branch(self) -> str:
        return f"lane-{self.letter.lower()}"

    @property
    def worktree_name(self) -> str:
        return f"os-lane-{self.letter.lower()}"

    @property
    def agent_name(self) -> str:
        return f"Lane {self.letter}"


LANES: dict[str, Lane] = {
    "A": Lane(
        "A",
        "Kernel, Core & Networking",
        "boot, memory, scheduler, IPC, syscalls, capabilities, processes, the "
        "in-kernel drivers and filesystems, the kernel graphics surface "
        "(DRM/KMS, fb), all of networking -- the in-kernel stack, the userspace "
        "net crates, WiFi, services/netstack -- the benchmarks and the boot test.",
        "gained networking from lane C (net/**, netipc/**, netproto/**, "
        "netring/**, net80211/**, aes/**, hmac/**) and services/netstack/** "
        "from lane B's old tree.",
    ),
    "B": Lane(
        "B",
        "Userland",
        "every program under userspace/ -- coreutils, Oils/OSH, the CLI tools, "
        "the package manager (userspace/pkg) -- and init, the service manager "
        "(init/**).",
        "handed posix/**, services/**, toolchain/stubs/**, "
        "toolchain/build-sysroot.ps1 and scripts/create-ext4-rootfs.sh to the "
        "new lane D.",
    ),
    "C": Lane(
        "C",
        "Desktop & Toolkit",
        "the desktop shell and window management (gui/desktop), the widget "
        "toolkit, appearance and themes, and the desktop services under gui/ "
        "(clipboard, notifications, credentials, associations, settings files, "
        "thumbnails, ...) -- all of gui/ except lane F's crates.",
        "handed apps/** to the new lane E; gui/compositor, gui/window, "
        "gui/remote, gui/font, gui/imagecodec and gui/vulkan to the new lane F; "
        "the networking crates to lane A.",
    ),
    "D": Lane(
        "D",
        "POSIX, libc & Toolchain",
        "the POSIX/libc layer (posix/**), the sysroot (toolchain/stubs, "
        "build-sysroot.ps1), the bare-metal service binaries and C test "
        "fixtures (services/**, except netstack), the rootfs image recipe, and "
        "the large ports that stand on libc (gcc/make/cmake, CPython, fastpy "
        "self-hosting, the Rust toolchain, WINE).",
        "new lane, taken from lane B's old tree.",
    ),
    "E": Lane(
        "E",
        "Applications",
        "every application under apps/.",
        "new lane, taken from lane C's old tree.",
    ),
    "F": Lane(
        "F",
        "Graphics Stack",
        "the compositor and its display protocol (gui/compositor, gui/remote), "
        "the window library every application links (gui/window), text "
        "rendering (gui/font), image decoding (gui/imagecodec) and the Vulkan "
        "loader (gui/vulkan) -- plus the GPU userspace ports (Mesa, Vulkan "
        "drivers, Vello/HarfBuzz).",
        "new lane, taken from lane C's old tree.",
    ),
}

LETTERS: tuple[str, ...] = tuple(LANES)

#: (path prefix, lane letter).  A prefix ending in `/` owns that directory and
#: everything under it; any other prefix names exactly one file.  **The
#: longest matching prefix wins**, which is how a carve-out is written:
#: `gui/compositor/` -> F outranks `gui/` -> C, and `services/netstack/` -> A
#: outranks `services/` -> D.  roadmap.md's ownership table is the prose form
#: of this tuple; if they disagree, fix whichever is wrong in the same commit.
OWNERSHIP: tuple[tuple[str, str], ...] = (
    # --- A: kernel, core & networking -------------------------------------
    ("kernel/", "A"),
    ("bench/", "A"),
    ("net/", "A"),
    ("netipc/", "A"),
    ("netproto/", "A"),
    ("netring/", "A"),
    ("net80211/", "A"),
    ("aes/", "A"),
    ("hmac/", "A"),
    ("services/netstack/", "A"),
    ("toolchain/x86_64-slateos.json", "A"),
    ("scripts/boot-test.sh", "A"),
    ("scripts/run-timeout.py", "A"),
    ("scripts/wedge-soak.sh", "A"),
    # --- B: userland ------------------------------------------------------
    ("userspace/", "B"),
    ("init/", "B"),
    # --- C: desktop & toolkit -- all of gui/ except F's crates below --------
    ("gui/", "C"),
    # --- D: POSIX, libc & toolchain ---------------------------------------
    ("posix/", "D"),
    ("services/", "D"),
    ("toolchain/stubs/", "D"),
    ("toolchain/build-sysroot.ps1", "D"),
    ("scripts/create-ext4-rootfs.sh", "D"),
    # --- E: applications --------------------------------------------------
    ("apps/", "E"),
    # `randrange` is a root leaf crate.  It is listed because it is the one
    # such crate that already had an owner on record -- scripts/pre-boot.py
    # held lane C to it -- and 15 of its 25 users are under apps/, which is
    # the half of lane C that became lane E.
    ("randrange/", "E"),
    # --- F: graphics stack ------------------------------------------------
    ("gui/compositor/", "F"),
    ("gui/window/", "F"),
    ("gui/remote/", "F"),
    ("gui/font/", "F"),
    ("gui/imagecodec/", "F"),
    ("gui/vulkan/", "F"),
)

#: What the table deliberately leaves to nobody, for the briefing.  These are
#: not free-for-alls: the shared documents and the workspace Cargo.toml have
#: their own rules (roadmap.md rules 3 and 4), and scripts/** plus the root leaf
#: crates are the open question A-Q11 ("who owns files that are not one lane's
#: tree?").  Until that is answered, change them through a request to the lanes
#: that use them, or keep the change additive and say so in the commit.
UNASSIGNED_NOTE = (
    "shared documents and the workspace Cargo.toml (roadmap.md rules 3-4); "
    "scripts/** other than the named files; the root leaf crates (sha2, "
    "procinfo, tzrules, deflate, ...) -- see open-questions.md A-Q11"
)


def _normalise(path: str | os.PathLike[str]) -> str:
    """A repo-relative, forward-slash form of `path`, or "" if it is not ours.

    An absolute path is accepted when it points into *any* checkout of this
    project -- `E:/visual studio projects/os-lane-a/kernel/...` answers the same
    as `kernel/...` -- because an agent working from the integration tree names
    files in its own worktree by absolute path, and ownership is a property of
    the repository path, not of which checkout it was spelled from.
    """
    raw = os.fspath(path).replace("\\", "/").strip()
    if not raw:
        return ""
    p = Path(raw)
    if p.is_absolute() or re.match(r"^[A-Za-z]:/", raw):
        try:
            rel = p.resolve().relative_to(PROJECT_ROOT.parent.resolve())
        except (OSError, ValueError):
            return ""
        parts = rel.parts[1:]  # drop the checkout's own directory name
        return "/".join(parts)
    rel_posix = str(PurePosixPath(raw))
    while rel_posix.startswith("./"):
        rel_posix = rel_posix[2:]
    return "" if rel_posix in (".", "") else rel_posix


def owner_of(path: str | os.PathLike[str]) -> str | None:
    """The letter of the lane that owns `path`, or None if no lane does.

    Longest prefix wins (see `OWNERSHIP`).  A directory may be named with or
    without its trailing slash.  None means "no lane's tree" -- a shared
    document, a script nobody owns, a root leaf crate -- never "anyone's".
    """
    rel = _normalise(path)
    if not rel:
        return None
    best: tuple[int, str] | None = None
    for prefix, lane in OWNERSHIP:
        if prefix.endswith("/"):
            hit = rel.startswith(prefix) or rel == prefix[:-1]
        else:
            hit = rel == prefix
        if hit and (best is None or len(prefix) > best[0]):
            best = (len(prefix), lane)
    return best[1] if best else None


def owned_by(letter: str) -> list[str]:
    """Human-readable ownership for one lane, carve-outs spelled out."""
    out = []
    for prefix, lane in OWNERSHIP:
        if lane != letter:
            continue
        shown = prefix + "**" if prefix.endswith("/") else prefix
        carved = [
            p for p, other in OWNERSHIP
            if other != letter and p != prefix and prefix.endswith("/")
            and p.startswith(prefix)
        ]
        if carved:
            shown += " (except " + ", ".join(c + "**" if c.endswith("/") else c
                                             for c in carved) + ")"
        out.append(shown)
    return out


_NAME_RE = re.compile(r"^\s*lane[\s_.-]*([a-z])\s*$", re.IGNORECASE)


def parse_lane_name(text: str | None) -> str | None:
    """`Lane D`, `Lane-D`, `lane_d`, `LaneD` -> "D".  Anything else -> None.

    Spelling-tolerant on purpose.  The operator names the lanes "Lane A" ..
    "Lane F", but orchestrator2's registry refuses a space in an agent name
    (identities match `[A-Za-z0-9][A-Za-z0-9._-]*`), so the name a session
    actually carries may be `Lane-A`.  Both mean lane A.
    """
    if not text:
        return None
    m = _NAME_RE.match(text)
    if not m:
        return None
    letter = m.group(1).upper()
    return letter if letter in LANES else None


def parse_lane_letter(text: str | None) -> str | None:
    """A bare letter (`d`, `D`) or anything `parse_lane_name` accepts."""
    if not text:
        return None
    t = text.strip()
    if len(t) == 1 and t.upper() in LANES:
        return t.upper()
    return parse_lane_name(t)


def current_branch(root: Path) -> str | None:
    """The branch checked out in the worktree at `root`, read from disk.

    Read from `.git` rather than by running git, for two reasons: it needs no
    subprocess, and it cannot be misdirected by an inherited `GIT_DIR` -- git
    exports one into every hook, and pre-push calls scripts that import this
    module (see scripts/gitenv.py for what that has cost before).  Returns None
    for a detached HEAD or anything unreadable.
    """
    dotgit = root / ".git"
    try:
        if dotgit.is_file():
            first = dotgit.read_text(encoding="utf-8").strip()
            if not first.startswith("gitdir:"):
                return None
            gitdir = Path(first[len("gitdir:"):].strip())
            if not gitdir.is_absolute():
                gitdir = (root / gitdir).resolve()
        elif dotgit.is_dir():
            gitdir = dotgit
        else:
            return None
        head = (gitdir / "HEAD").read_text(encoding="utf-8").strip()
    except OSError:
        return None
    prefix = "ref: refs/heads/"
    return head[len(prefix):] if head.startswith(prefix) else None


def lane_of_worktree(
    root: Path,
    branch_of: Callable[[Path], str | None] = current_branch,
) -> tuple[str | None, str]:
    """`(letter or None, what the worktree says)` for the checkout at `root`.

    The directory name alone is a label anyone can change; the branch is what
    decides where the next commit lands.  So a lane is only claimed when the
    two agree, and a disagreement is reported as a conflict (the letter "!").
    """
    name = root.name
    m = re.fullmatch(r"os-lane-([a-z])", name)
    if not m or m.group(1).upper() not in LANES:
        if name == INTEGRATION_WORKTREE:
            return None, f"the integration worktree `{name}` (main; nobody's lane)"
        return None, f"worktree `{name}` is not a lane worktree"
    letter = m.group(1).upper()
    branch = branch_of(root)
    want = LANES[letter].branch
    if branch is None:
        return letter, f"worktree {name} (branch unreadable; directory name only)"
    if branch != want:
        return "!", (f"worktree {name} has `{branch}` checked out, not `{want}` "
                     f"-- somebody switched branches inside it")
    return letter, f"worktree {name} on {branch}"


def detect_lane(
    agent_name: str | None = None,
    lane: str | None = None,
    *,
    environ: Mapping[str, str] | None = None,
    root: Path | None = None,
    branch_of: Callable[[Path], str | None] = current_branch,
) -> tuple[str | None, str]:
    """Return `(lane letter or None, how it was decided)`.

    The second element is prose for a human: the evidence when a lane was
    found, the reason when it was not.  Importers that only want the letter
    take `detect_lane()[0]`; the tuple shape is the one the three-lane version
    returned, so they did not have to change.
    """
    env = os.environ if environ is None else environ
    root = PROJECT_ROOT if root is None else root

    claims: list[tuple[str, str]] = []  # (letter, source)
    notes: list[str] = []

    if lane:
        got = parse_lane_letter(lane)
        if got is None:
            return None, f"--lane {lane!r} is not one of {', '.join(LETTERS)}"
        claims.append((got, f"--lane {lane}"))
    if agent_name:
        got = parse_lane_name(agent_name)
        if got is None:
            return None, (f"agent name {agent_name!r} is not a lane name "
                          f"(expected 'Lane A' .. 'Lane F', or 'Lane-A')")
        claims.append((got, f"agent name {agent_name!r}"))

    raw = env.get("SLATEOS_LANE", "").strip()
    if raw:
        got = parse_lane_letter(raw)
        if got is None:
            return None, f"SLATEOS_LANE={raw!r} is not one of {', '.join(LETTERS)}"
        claims.append((got, f"SLATEOS_LANE={raw}"))

    raw = env.get("ORCH2_AGENT_NAME", "").strip()
    if raw:
        got = parse_lane_name(raw)
        if got is None:
            # Some other agent's name (a session in another project, or an
            # auto-assigned one like `os-00`).  Not evidence either way.
            notes.append(f"ORCH2_AGENT_NAME={raw!r} is not a lane name; ignored")
        else:
            claims.append((got, f"ORCH2_AGENT_NAME={raw}"))

    wt_letter, wt_how = lane_of_worktree(root, branch_of)
    if wt_letter == "!":
        return None, wt_how
    if wt_letter is not None:
        claims.append((wt_letter, wt_how))
    else:
        notes.append(wt_how)

    letters = sorted({c[0] for c in claims})
    if len(letters) > 1:
        detail = "; ".join(f"{src} says {letter}" for letter, src in claims)
        return None, f"the sources disagree: {detail}"
    if not letters:
        why = "; ".join(notes) if notes else "no source named a lane"
        return None, f"nothing identifies this session's lane ({why})"
    how = "; ".join(src for _letter, src in claims)
    if notes:
        how += " (" + "; ".join(notes) + ")"
    return letters[0], how


# ---------------------------------------------------------------------------
# Output
# ---------------------------------------------------------------------------

def _unknown(how: str) -> str:
    worktrees = ", ".join(
        f"{lane.agent_name} -> {PROJECT_ROOT.parent / lane.worktree_name}"
        for lane in LANES.values()
    )
    return (
        "lane: UNKNOWN\n"
        f"why:  {how}\n"
        f"CLAUDE_CONFIG_DIR: {os.environ.get('CLAUDE_CONFIG_DIR') or '<unset>'} "
        "(the account -- two lanes share each one, so it cannot say which)\n"
        "\n"
        "Your lane is your agent name. `ListAgents` prints it on its first line\n"
        "('This session is Lane-D ...'). Re-run with it:\n"
        "\n"
        '    python scripts/which-lane.py --agent-name "Lane-D"\n'
        "\n"
        "or run the copy of this script inside your own worktree, which answers\n"
        f"for that worktree: {worktrees}.\n"
        "\n"
        "If your agent name is not 'Lane <letter>', or the sources above\n"
        "disagree, STOP: do not guess a lane and do not edit anything shared.\n"
        "Ask the operator which lane this session is."
    )


def briefing(letter: str, how: str) -> str:
    lane = LANES[letter]
    wt = PROJECT_ROOT.parent / lane.worktree_name
    state = "" if wt.is_dir() else "   <-- MISSING: see roadmap.md Step 0.5"
    here = ""
    if PROJECT_ROOT.name != lane.worktree_name:
        here = (f"\n\nNOTE: this copy of the script is in `{PROJECT_ROOT.name}`, not "
                f"your worktree.\n      Work in {wt} -- never edit files here.")
    lines = [
        f"lane:              {letter}",
        f"name:              {lane.name}",
        f"agent name:        {lane.agent_name}",
        f"worktree:          {wt}{state}",
        f"branch:            {lane.branch}",
        f"identified by:     {how}",
        f"CLAUDE_CONFIG_DIR: {os.environ.get('CLAUDE_CONFIG_DIR') or '<unset>'}"
        "  (informational; two lanes share each account)",
        f"owns (write):      {', '.join(owned_by(letter))}",
        "not yours:         everything another lane owns -- ask with "
        "`--owner PATH`",
        f"no lane owns:      {UNASSIGNED_NOTE}",
        "",
        f"Your domain: {lane.summary}",
    ]
    if lane.split_note:
        lines.append(f"Six-lane split (2026-09-22): {lane.split_note}")
    lines += [
        "",
        f"Work only items tagged `[{letter}]` in roadmap.md. Need a change in "
        "another lane's tree?",
        f"File requests/{letter.lower()}-<to>-<slug>.md (e.g. "
        f"requests/{letter.lower()}-a-<slug>.md) instead of editing it.",
        'Full rules: roadmap.md -> "Six-Agent Parallel Execution".',
    ]
    return "\n".join(lines) + here


def table() -> str:
    rows = []
    for letter, lane in LANES.items():
        rows.append(f"{letter}  {lane.agent_name:<7} {lane.name:<27} "
                    f"{', '.join(owned_by(letter))}")
    rows.append(f"-  (none)  {'':<27} {UNASSIGNED_NOTE}")
    return "\n".join(rows)


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

def _self_test() -> int:
    """Fixtures for every rule above, each asserted in both directions."""
    failures: list[str] = []

    def check(label: str, got: object, want: object) -> None:
        if got == want:
            print(f"  ok    {label}")
        else:
            print(f"  FAIL  {label}: got {got!r}, want {want!r}")
            failures.append(label)

    # --- importable the way every importer imports it -----------------------
    # By path, and NOT registered in sys.modules -- check-lane-signals.py,
    # prune-build-trees.py, open-requests.py and pre-boot.py all do exactly
    # this.  A construct that needs the module in sys.modules (a @dataclass
    # under postponed annotations, for one) passes every other case here and
    # breaks all four of them.
    import importlib.util
    spec = importlib.util.spec_from_file_location("which_lane_probe", __file__)
    try:
        if spec is None or spec.loader is None:
            raise ImportError("no loader")
        probe = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(probe)
        loaded: object = all(callable(getattr(probe, fn, None))
                             for fn in ("detect_lane", "owner_of"))
    except Exception as exc:  # noqa: BLE001 -- the failure IS the finding
        loaded = f"{type(exc).__name__}: {exc}"
    check("loads by path without a sys.modules entry, as importers do",
          loaded, True)

    # --- names ------------------------------------------------------------
    for text, want in [
        ("Lane A", "A"), ("Lane-A", "A"), ("lane_f", "F"), ("LaneC", "C"),
        ("  lane d ", "D"), ("LANE-E", "E"), ("Lane.B", "B"),
        ("Lane G", None), ("os-00", None), ("A", None), ("", None),
        ("Lane AB", None), ("orchestrator2-ee", None),
    ]:
        check(f"parse_lane_name({text!r})", parse_lane_name(text), want)
    for text, want in [("d", "D"), ("F", "F"), ("Lane-B", "B"), ("g", None),
                       ("AB", None)]:
        check(f"parse_lane_letter({text!r})", parse_lane_letter(text), want)

    # --- ownership --------------------------------------------------------
    for path, want in [
        ("kernel/src/main.rs", "A"),
        ("kernel\\src\\main.rs", "A"),
        ("./kernel/src/main.rs", "A"),
        ("net80211/src/lib.rs", "A"),
        ("netipc", "A"),
        ("services/netstack/src/main.rs", "A"),
        ("services/netstack", "A"),
        ("scripts/boot-test.sh", "A"),
        ("scripts/boot-test.sh.orig", None),
        ("userspace/oils/README.md", "B"),
        ("init/src/main.rs", "B"),
        ("gui/toolkit/src/lib.rs", "C"),
        ("gui/desktop", "C"),
        ("gui/Cargo.toml", "C"),
        ("posix/src/unistd.rs", "D"),
        ("services/init/src/main.rs", "D"),
        ("services/ctest-pty/main.c", "D"),
        ("toolchain/stubs/src/lib.rs", "D"),
        ("toolchain/build-sysroot.ps1", "D"),
        ("toolchain/x86_64-slateos.json", "A"),
        ("apps/chess/src/main.rs", "E"),
        ("randrange/src/lib.rs", "E"),
        ("gui/compositor/src/lib.rs", "F"),
        ("gui/compositor", "F"),
        ("gui/window/src/lib.rs", "F"),
        ("gui/remote/src/lib.rs", "F"),
        ("gui/font/src/lib.rs", "F"),
        ("gui/imagecodec/src/png.rs", "F"),
        ("gui/vulkan/src/lib.rs", "F"),
        ("gui/compositorx/src/lib.rs", "C"),
        ("scripts/check-eol.py", None),
        ("Cargo.toml", None),
        ("roadmap.md", None),
        ("sha2/src/lib.rs", None),
        ("", None),
    ]:
        check(f"owner_of({path!r})", owner_of(path), want)

    sibling = PROJECT_ROOT.parent / "os-lane-a" / "gui" / "window" / "src" / "lib.rs"
    check("owner_of(absolute path in a sibling checkout)", owner_of(sibling), "F")
    outside = Path(PROJECT_ROOT.anchor) / "definitely-not-this-project" / "kernel"
    check("owner_of(absolute path outside the project)", owner_of(outside), None)

    prefixes = [p for p, _ in OWNERSHIP]
    check("no prefix is listed twice", len(prefixes), len(set(prefixes)))
    check("every lane owns something",
          sorted({lane for _, lane in OWNERSHIP}), sorted(LETTERS))
    check("every owner is a known lane",
          all(lane in LANES for _, lane in OWNERSHIP), True)

    # --- detection --------------------------------------------------------
    parent = PROJECT_ROOT.parent

    def branch_is(name):
        return lambda _root: name

    def detect(**kw):
        kw.setdefault("environ", {})
        return detect_lane(**kw)[0]

    check("worktree os-lane-d on lane-d -> D",
          detect(root=parent / "os-lane-d", branch_of=branch_is("lane-d")), "D")
    check("worktree os-lane-d on lane-a -> refused",
          detect(root=parent / "os-lane-d", branch_of=branch_is("lane-a")), None)
    check("worktree os-lane-d, branch unreadable -> D by name",
          detect(root=parent / "os-lane-d", branch_of=branch_is(None)), "D")
    check("the integration tree alone -> unknown",
          detect(root=parent / "os", branch_of=branch_is("main")), None)
    check("agent name in the integration tree -> that lane",
          detect(agent_name="Lane-D", root=parent / "os",
                 branch_of=branch_is("main")), "D")
    check("agent name contradicting the worktree -> refused",
          detect(agent_name="Lane-D", root=parent / "os-lane-a",
                 branch_of=branch_is("lane-a")), None)
    check("agent name agreeing with the worktree -> that lane",
          detect(agent_name="Lane E", root=parent / "os-lane-e",
                 branch_of=branch_is("lane-e")), "E")
    check("--lane letter -> that lane",
          detect(lane="f", root=parent / "os", branch_of=branch_is("main")), "F")
    check("a non-lane agent name is refused outright",
          detect(agent_name="os-00", root=parent / "os-lane-a",
                 branch_of=branch_is("lane-a")), None)
    check("SLATEOS_LANE -> that lane",
          detect(environ={"SLATEOS_LANE": "B"}, root=parent / "os",
                 branch_of=branch_is("main")), "B")
    check("SLATEOS_LANE contradicting ORCH2_AGENT_NAME -> refused",
          detect(environ={"SLATEOS_LANE": "B", "ORCH2_AGENT_NAME": "Lane-C"},
                 root=parent / "os", branch_of=branch_is("main")), None)
    check("ORCH2_AGENT_NAME -> that lane",
          detect(environ={"ORCH2_AGENT_NAME": "Lane F"}, root=parent / "os",
                 branch_of=branch_is("main")), "F")
    check("a non-lane ORCH2_AGENT_NAME is ignored, not refused",
          detect(environ={"ORCH2_AGENT_NAME": "os-00"}, root=parent / "os-lane-c",
                 branch_of=branch_is("lane-c")), "C")
    check("CLAUDE_CONFIG_DIR alone no longer identifies a lane",
          detect(environ={"CLAUDE_CONFIG_DIR": r"C:\Users\x\.claude-account-b"},
                 root=parent / "os", branch_of=branch_is("main")), None)
    check("a scratch worktree is nobody's lane",
          detect(root=parent / "os-six-lanes", branch_of=branch_is("six-lanes")),
          None)
    check("os-lane-g is not a lane",
          detect(root=parent / "os-lane-g", branch_of=branch_is("lane-g")), None)

    print()
    if failures:
        print(f"which-lane: {len(failures)} FAILURE(S)")
        return 1
    print("which-lane: self-test passed")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Print which parallel-agent lane this session is."
    )
    parser.add_argument("--agent-name", metavar="NAME",
                        help="this session's agent name, as ListAgents shows it "
                             "(e.g. 'Lane-D')")
    parser.add_argument("--lane", metavar="X",
                        help="assert the lane letter directly (A-F)")
    parser.add_argument("--letter", action="store_true",
                        help="print only the lane letter, for scripting")
    parser.add_argument("--owner", nargs="+", metavar="PATH",
                        help="print the lane that owns each PATH and exit")
    parser.add_argument("--table", action="store_true",
                        help="print the whole ownership table and exit")
    parser.add_argument("--self-test", "--selftest", "--self_test",
                        dest="selftest", action="store_true",
                        help="run this script's own fixtures and exit")
    args = parser.parse_args(argv)

    if args.selftest:
        return _self_test()
    if args.table:
        print(table())
        return 0
    if args.owner:
        for path in args.owner:
            print(f"{owner_of(path) or '-'}\t{path}")
        return 0

    letter, how = detect_lane(args.agent_name, args.lane)
    if letter is None:
        print(_unknown(how), file=sys.stderr)
        return 2
    if args.letter:
        print(letter)
        return 0
    print(briefing(letter, how))
    return 0


if __name__ == "__main__":
    sys.exit(main())
