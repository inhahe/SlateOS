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

EVERY TRACKED FILE HAS AN OWNER (design-decisions §973)
-------------------------------------------------------
The operator's answer to A-Q11: every file gets exactly one owning lane, a lane
that wants a change in another lane's file asks for it, and no file is left
with ambiguous ownership. So `ownership(path)` answers every tracked path with
one of four things, never with a shrug:

  * a **lane** -- the tables below (`OWNERSHIP`, `SCRIPT_OWNERS`), or, for a
    request, its sender: `requests/a-bc-<slug>.md` is lane A's;
  * the **operator** -- `OPERATOR_OWNS`: CLAUDE.md, the design texts, the
    operator's notes and backups. No lane edits these without being told to;
  * a **shared document** -- `SHARED_DOCUMENTS`: the files every lane writes
    under a named per-lane rule (roadmap.md rule 3), which is an owner, not an
    ambiguity;
  * **nobody** -- which `--check-all` refuses. That is the gate: a new file at
    the top of the tree, or a new script, gets its owner in the commit that
    creates it.

`owner_of` keeps its old contract -- a lane letter or None -- so its importers
are unaffected: an operator file or a shared document is no lane's tree.

A lane that adds a script under `scripts/` adds its line to its own block of
`SCRIPT_OWNERS` in the same commit. That additive line, naming the adding lane,
is the one edit to this file (lane A's) that another lane may make without a
request; anything else here goes through `requests/`.

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
    python scripts/which-lane.py --check-all [--head REV] # the gate: every tracked file owned
    python scripts/which-lane.py --self-test

Exit status: 0 when the lane was identified (or for `--owner` / `--table`),
2 when it could not be. `--check-all`: 0 when every tracked file has an owner
and every table entry names a tracked file, 1 when not, 2 when git could not
list the files.  On 2, *stop and ask the operator* rather than
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
        "rendering (gui/font), image decoding (gui/imagecodec), the video "
        "codecs (gui/video) and the Vulkan loader (gui/vulkan) -- plus the GPU "
        "userspace ports (Mesa, Vulkan drivers, Vello/HarfBuzz).",
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
    # The video codecs, one crate each: rav1d (AV1, which AVIF pictures are
    # made of) and, per the operator's F-Q2 answer (design-decisions.md
    # sec. 1332), VP9 for remote desktop's capture stream.  Added 2026-09-27
    # when lane F created the directory; it never held lane C work, so the
    # carve-out out of `gui/` took nothing lane C had.
    ("gui/video/", "F"),
    # --- §973: every file that had no lane (2026-09-27) --------------------
    # Root leaf crates go to the lane whose code depends on them most, counted
    # from the manifests -- except the hash and crypto family, which stays in
    # one lane (A, with aes/ and hmac/) because design-decisions §539 wants its
    # primitives ported, vetted and collapsed to one copy, and that is one
    # owner's job.
    ("blake2/", "A"),
    ("blockbuf/", "A"),
    ("crc32/", "A"),
    ("crc32c/", "A"),
    ("crc64/", "A"),
    ("md5/", "A"),
    ("pwkdf/", "A"),
    # The DRM plane-composition arithmetic (design-decisions §976): the
    # kernel's display backends' crate, host-tested on its own.
    ("planecompose/", "A"),
    ("sha1/", "A"),
    ("sha2/", "A"),
    ("sm3/", "A"),
    ("xxhash64/", "A"),
    ("civildate/", "B"),
    ("cronspool/", "B"),
    ("killconv/", "B"),
    ("libcall/", "B"),
    ("monoclock/", "B"),
    ("optionalfile/", "B"),
    ("procinfo/", "B"),
    ("readpass/", "B"),
    ("ttyidle/", "B"),
    ("utmpfile/", "B"),
    ("byteread/", "E"),
    ("deflate/", "E"),
    ("textfind/", "E"),
    ("textfmt/", "E"),
    ("tzrules/", "E"),
    ("yamldoc/", "E"),
    ("ziparchive/", "E"),
    # Reserved before its first file (lane E, 2026-09-27): the vetted crypto
    # design-decisions §539 calls for -- RustCrypto's XChaCha20-Poly1305 and
    # Argon2id, vendored with their upstream revision and published vectors,
    # one copy for every consumer (apps/credmanager, gui/credentials, and the
    # kernel's diskencrypt under §978, which is why it must build no_std +
    # alloc). Lane E vendors and owns it; the older hand-written primitives
    # above stay lane A's until each consumer has moved over.
    ("rustcrypto/", "E"),
    # Boot, build and repository configuration: lane A, which owns the boot
    # and the gates.
    ("esp/", "A"),
    (".cargo/", "A"),
    ("limine.conf", "A"),
    ("ovmf-code.fd", "A"),
    ("clippy.toml", "A"),
    (".gitattributes", "A"),
    (".gitignore", "A"),
    (".git-blame-ignore-revs", "A"),
    ("bare-metal-boot.md", "A"),
    ("net-userspace-migration.md", "A"),
    ("performance-targets.md", "A"),
    ("reference-implementations.md", "A"),
    ("subsystem-map.md", "A"),
    ("apply_clippy_fixes.py", "A"),
    ("find_gaps.py", "A"),
    ("serial_err.txt", "A"),
    ("serial_new.txt", "A"),
    ("serial_new_err.txt", "A"),
    ("requests/.gitkeep", "A"),
    ("coreutils-canonical-answer.md", "B"),
    ("programs.md", "B"),
    ("fix_placeholder_tests.py", "B"),
    ("loosen_stub_cli_tests.py", "B"),
    ("build-env.md", "D"),
    ("posix-blockers.md", "D"),
    ("strace_6.8-0ubuntu2_amd64.deb", "D"),
    # The three requests filed before requests were named by sender, by the
    # zone that filed them.
    ("requests/coreutils_needs_kernel_embedding.md", "B"),
    ("requests/shell_needs_kernel_embedding.md", "B"),
    ("requests/osb2_needs_compositor_syscalls.md", "C"),
    # scripts/ subdirectories; the files directly in scripts/ are in
    # SCRIPT_OWNERS below.
    ("scripts/hooks/", "A"),
    ("scripts/lib/", "A"),
    ("scripts/gatecache_site/", "A"),
    ("scripts/coreutils-spike/", "B"),
    ("scripts/dup-differential-cases/", "B"),
    ("scripts/oils-spec/", "B"),
    ("scripts/oils-spike/", "B"),
    ("scripts/bash-spike/", "D"),
    ("scripts/cmake-spike/", "D"),
    ("scripts/cpython-spike/", "D"),
    ("scripts/fixtures/", "D"),
    ("scripts/make-spike/", "D"),
    ("scripts/pkgconf-spike/", "D"),
    ("scripts/espeak-spike/", "E"),
)

#: Every file directly under `scripts/`, by owning lane (design-decisions
#: §973). A script belongs to the lane whose code it judges or serves; a test
#: suite goes with the script it tests; a data file with the script that reads
#: it; the gate machinery, and the gates that judge every lane's files, are lane
#: A's. Lanes C and D named their own on 2026-09-27. A lane adding a script adds
#: its line to its own block in the same commit -- the one edit to this file
#: another lane may make without a request.
SCRIPT_OWNERS: dict[str, tuple[str, ...]] = {
    "A": (
        "abi-reach.py",
        "absent-operand-ledger.txt",
        "audit-rustfmt-drift.py",
        "bench-history.py",
        "boot-history.py",
        "bootstrap-worktree.sh",
        "build-image.ps1",
        "build-iso.sh",
        "build-usb-image.py",
        "bytestr-oracle.rs",
        "canary-load-test.sh",
        "canary-load.py",
        "canary-spread-survey.py",
        "check-absent-operand-default.py",
        "check-accidental-headings.py",
        "check-ansic-quoting-vs-bash.py",
        "check-bin-collisions.py",
        "check-boot-skips.py",
        "check-boot-test-reexec.sh",
        "check-cfg-unix.py",
        "check-collapsed-messages.py",
        "check-config-turn-guards.py",
        "check-control-bytes.py",
        "check-crate-names.py",
        "check-dead-code-allows.py",
        "check-destructive-writes.py",
        "check-docs.py",
        "check-eol.py",
        "check-evdev-elf-asm.py",
        "check-excluded-crate-tests.py",
        "check-foreign-worktree-paths.py",
        "check-gate-call-sites.py",
        "check-gate-invocation-parity.py",
        "check-gated-selftests.py",
        "check-gates-are-wired.py",
        "check-gates-can-refuse.py",
        "check-kshell-pipeline-vs-bash.py",
        "check-kshell-rungs-vs-bash.py",
        "check-lane-signals.py",
        "check-linux-only-capabilities.py",
        "check-live-counter-reads.py",
        "check-mutation-needles.py",
        "check-option-refusal.py",
        "check-program-catalogue.py",
        "check-query-status.py",
        "check-ran-if.py",
        "check-recursive-locks.py",
        "check-refusals-refuse.py",
        "check-release-staleness.py",
        "check-requests-not-deleted.py",
        "check-ring3-entry-regs.py",
        "check-roadmap-done.py",
        "check-scratch-config.py",
        "check-self-tests-wired.py",
        "check-selftest-flag-spellings.py",
        "check-selftest-format-wording.py",
        "check-selftest-reach.py",
        "check-selftest-reinit.py",
        "check-selftest-rung-numbers.py",
        "check-selftest-skips.py",
        "check-selftest-wording.py",
        "check-shell-callables.py",
        "check-shell-message-names.py",
        "check-shell-noun-article.py",
        "check-shellquote-vs-bash.py",
        "check-stale-blockers.py",
        "check-test-order-independence.py",
        "check-test-root-writes.py",
        "check-text-mode-writes.py",
        "check-unreachable-mutators.py",
        "check-untested-crates.py",
        "check-usage-names-reach-the-command.py",
        "check-usage-status.py",
        "check-user-access-sites.py",
        "check-variant-lists.py",
        "check-vfs-permission-gate.py",
        "check-vfs-under-lock.py",
        "check-workspace-lints.py",
        "clippy-diff.py",
        "clippy-sites.py",
        "control-bytes-baseline.txt",
        "create-disk.py",
        "display_rename.py",
        "doc_entries.py",
        "docs-baseline.json",
        "docs-carry-forward.py",
        "docs-migrate.py",
        "docs_layout.py",
        "docsearch.py",
        "echo-escapes-oracle.rs",
        "flake-hunt.sh",
        "gate-cache.py",
        "gate-cost-report.py",
        "gatecache_tee.py",
        "gatecache_trace.py",
        "gen-script-index.py",
        "gitenv.py",
        "gittree.py",
        "grade-positional.py",
        "guest.py",
        "hang-repro-loop.sh",
        "hostload.py",
        "install-hooks.sh",
        "kasan-build.sh",
        "kasan-check-preshadow.py",
        "ki_split.py",
        "layout-sweep.py",
        "live-counter-ledger.txt",
        "merge-readiness.py",
        "migrate-worktrees-to-drive.ps1",
        "msysbash.py",
        "mutate-gate.py",
        "never-read-probe.py",
        "open-requests.py",
        "option-refusal-ledger.txt",
        "orphan-modules-baseline.txt",
        "positional-model-limits.py",
        "pre-boot.py",
        "proctree.py",
        "prune-build-cache.py",
        "prune-build-trees.py",
        "qemu-probe.py",
        "raced-globals-baseline.txt",
        "raced-globals.py",
        "reclaim-space.py",
        "resolve-rip.sh",
        "roadmap-done-baseline.txt",
        "run-checker.sh",
        "run-qemu.ps1",
        "rust_scopes.py",
        "rustemit.py",
        "rustlex.py",
        "rustrungs.py",
        "rustscan.py",
        "safewrite.py",
        "scan-orphan-modules.py",
        "scan-unwrap.py",
        "selftest-boot-gate-identity.py",
        "selftestflag.py",
        "shellcheck-all.sh",
        "snapshot-todo2.sh",
        "soak-nmi-check.sh",
        "split-frames.py",
        "split-hunks.py",
        "src_digest.py",
        "srcload.py",
        "stack-frames.py",
        "stage-hunks.py",
        "straddle-check.py",
        "suite_pool.py",
        "symbolize.py",
        "test-bench-history.py",
        "test-boot-history-commit.py",
        "test-boot-history.py",
        "test-boot-lock.sh",
        "test-boot-test.py",
        "test-bootstrap-worktree.py",
        "test-build-usb-image.py",
        "test-canary-load.py",
        "test-check-boot-skips.py",
        "test-check-gated-selftests.py",
        "test-check-release-staleness.py",
        "test-check-requests-not-deleted.py",
        "test-check-self-tests-wired.py",
        "test-docs-tools.py",
        "test-gate-cache.py",
        "test-gate-cost-report.py",
        "test-gittree.py",
        "test-grade-positional.py",
        "test-guest.py",
        "test-hostload.py",
        "test-layout-sweep.py",
        "test-msysbash.py",
        "test-open-requests.py",
        "test-pre-push-fmt-gate.py",
        "test-pre-push-identity-gate.py",
        "test-pre-push-python-choice.py",
        "test-pre-push-run-checker.py",
        "test-pre-push-suites-scope.py",
        "test-pre-push-touches.py",
        "test-pre-push-tree-is-push.py",
        "test-proctree.py",
        "test-prune-build-cache.py",
        "test-prune-build-trees.py",
        "test-reclaim-space.py",
        "test-rustemit.py",
        "test-selftests-are-repo-safe.py",
        "test-src-digest.py",
        "test-srcload.py",
        "test-straddle-check.py",
        "test-worktree.sh",
        "untested-crates-baseline.txt",
        "variant-lists-partial.txt",
        "wdog-nmi-soak.sh",
        "wdog-reset-experiment.sh",
        "which-lane.py",
        "who-holds-dir.py",
        "workspace-lints-baseline.txt",
        "workspace-test.py",
        "write-usb-stick.ps1",
    ),
    "B": (
        "all-diff.sh",
        "arch-diff.sh",
        "argv-ignored-baseline.txt",
        "argv-utf8-baseline.txt",
        "argv-utf8.py",
        "audit-cli-fabrication.py",
        "awk-diff.sh",
        "basenc-diff.sh",
        "bashprobe.py",
        "bc-diff.sh",
        "blkdiscard-diff.sh",
        "blkid-cli-diff.sh",
        "blkid-diff.sh",
        "blkid-probe.c",
        "blockdev-diff.sh",
        "blockdev-shim.c",
        "build-userspace.ps1",
        "c-maybe-probe.py",
        "cal-diff.sh",
        "calc-diff.sh",
        "cat-diff.sh",
        "charwidth-gen.py",
        "check-argv-ignored.py",
        "check-cp-diff-sees-nul.py",
        "check-diff-preamble-order.py",
        "check-doc-links.py",
        "check-drive-root-litter.py",
        "check-help-vs-parser.py",
        "check-option-stops.py",
        "check-read-defaults.py",
        "chgrp-diff.sh",
        "chown-diff.sh",
        "cksum-diff.sh",
        "cli-fabrication-baseline.txt",
        "cmp-diff.sh",
        "column-diff.sh",
        "comm-diff.sh",
        "comm-probe.py",
        "compare-short-options.py",
        "coreutils-check.sh",
        "cp-diff.sh",
        "csplit-diff.sh",
        "cut-diff.sh",
        "date-diff.sh",
        "dd-diff.sh",
        "df-diff.sh",
        "diff-diff.sh",
        "diff-wsl.sh",
        "digest-diff.sh",
        "dircolors-diff.sh",
        "du-diff.sh",
        "dup-bins-survey.py",
        "dup-differential.py",
        "echo-diff.sh",
        "ed-diff.sh",
        "env-diff.sh",
        "expand-diff.sh",
        "expr-diff.sh",
        "extfloat-cases.py",
        "extfloat-diff.sh",
        "extfloat-div-probe.c",
        "extfloat-probe.c",
        "factor-diff.sh",
        "fastpy-slateos-bundle.py",
        "file-diff.sh",
        "file-gen-cdf.py",
        "file-gen-elf.py",
        "file-gen-z.py",
        "file-magic-vendor.py",
        "find-diff.sh",
        "findmnt-diff.sh",
        "flock-diff.sh",
        "fmt-diff.sh",
        "fnmatch-probe.c",
        "fold-diff.sh",
        "free-diff.sh",
        "frozen-flag-answered.txt",
        "gen-oils-bind-tables.py",
        "getconf-diff.sh",
        "getconf-gen.py",
        "getopt-ambiguity-check.py",
        "getopt-diff.sh",
        "grep-diff.sh",
        "head-diff.sh",
        "host-errmsg-baseline.txt",
        "host-errmsg.py",
        "hostid-diff.sh",
        "hostname-diff.sh",
        "id-diff.sh",
        "interleave-diff.sh",
        "join-diff.sh",
        "join-probe.py",
        "lockfile-diff.sh",
        "logger-diff.sh",
        "logname-diff.sh",
        "ls-diff.sh",
        "ls-quote-probe.py",
        "lsblk-diff.sh",
        "lscpu-diff.sh",
        "lsirq-diff.sh",
        "lsmem-diff.sh",
        "lsns-diff-world.sh",
        "lsns-diff.sh",
        "mknod-diff.sh",
        "mktemp-diff.sh",
        "more-diff.sh",
        "mountpoint-diff.sh",
        "multicall-aliases-baseline.txt",
        "multicall-aliases.py",
        "multicall-shadowed-baseline.txt",
        "mv-diff.sh",
        "nice-diff.sh",
        "nl-diff.sh",
        "nohup-diff.sh",
        "nproc-diff.sh",
        "numfmt-diff.sh",
        "od-diff.sh",
        "option-gap-baseline.txt",
        "option-gap-ref.sh",
        "option-gap.sh",
        "option-stops-baseline.txt",
        "osh-bash-diff.py",
        "osh-diff.sh",
        "parse-datetime-diff.sh",
        "paste-diff.sh",
        "paste-probe.py",
        "patch-diff.sh",
        "pathchk-diff.sh",
        "pinky-diff.sh",
        "pr-diff.sh",
        "printable-audit.py",
        "printenv-diff.sh",
        "printf-cases.py",
        "printf-diff.sh",
        "printf-probe.sh",
        "prlimit-diff.sh",
        "probe-cp-diff-nul.sh",
        "probe-date-d-grammar.sh",
        "probe-date-f.sh",
        "probe-date-r-quoting.sh",
        "probe-diff-name-quoting.sh",
        "probe-diff-side-by-side.sh",
        "probe-env-empty-name.sh",
        "probe-env-split-empty.sh",
        "probe-env-split-escapes.sh",
        "probe-env-split-expansion-env.sh",
        "probe-env-split-options.sh",
        "probe-env-split-string.sh",
        "program-catalogue.py",
        "ps-diff.sh",
        "ptx-diff.sh",
        "pwd-diff.sh",
        "quote-names-baseline.txt",
        "quote-names-why.py",
        "quote-names-wire.py",
        "quote-names.py",
        "quote-probe.py",
        "quote-sweep.py",
        "read-defaults-baseline.txt",
        "rm-diff.sh",
        "scols-probe.c",
        "sed-diff.sh",
        "seq-cases.py",
        "seq-diff.sh",
        "seq-probe.sh",
        "sh-diff.sh",
        "sharutils-ref.sh",
        "shred-diff.sh",
        "shuf-diff.sh",
        "smartcols-cases.py",
        "smartcols-diff.sh",
        "sort-diff.sh",
        "split-diff.sh",
        "stat-diff.sh",
        "stderr-exit-zero-sweep.py",
        "stdin-hang-sweep.sh",
        "strftime-diff.sh",
        "strftime-probe.c",
        "strings-diff.sh",
        "sum-diff.sh",
        "swapon-diff.sh",
        "sync-diff.sh",
        "syslog-client-check.sh",
        "tac-diff.sh",
        "tail-diff.sh",
        "tar-diff.sh",
        "tee-diff.sh",
        "test-check-cp-diff-sees-nul.py",
        "test-checkers-honour-head.py",
        "test-diff-bound.sh",
        "test-diff-forward.sh",
        "test-diff.sh",
        "test-fastpy-slateos-bundle.py",
        "test-pre-push-doclinks-gate.py",
        "test-pre-push-gates.py",
        "test-pre-push-unixhalf-gate.py",
        "test-program-catalogue.py",
        "time-diff.sh",
        "touch-diff.sh",
        "tr-diff.sh",
        "truncate-diff.sh",
        "tsort-diff.sh",
        "tsort-probe.py",
        "tty-diff.sh",
        "tz-diff.sh",
        "uname-diff.sh",
        "unexpand-diff.sh",
        "uniq-diff.sh",
        "unknown-option-sweep.py",
        "unlink-diff.sh",
        "uptime-diff.sh",
        "users-diff.sh",
        "util-linux-extra.sh",
        "util-linux-source.sh",
        "uu-diff.sh",
        "wc-diff.sh",
        "whoami-diff.sh",
        "wipefs-diff.sh",
        "write-error-diff.sh",
        "xargs-diff.sh",
        "yes-diff.sh",
    ),
    "C": (
        "check-contrast-explorer.js",
        "check-fields-written-never-read.py",
        "check-overlay0-ink.py",
        "check-tested-but-uncalled.py",
        "check-text-ink.py",
        "check-window-wiring.py",
        "contrast-explorer.html",
        "fields-written-never-read-baseline.txt",
        "gather-notices.py",
        "lane-claims.py",
        "lanec_scan.py",
        "reintro-input-settings.py",
        "reintro-keylayout.py",
        "reintro-list-hit-tests.py",
        "reintro-modal-geometry.py",
        "reintro-mouse-page.py",
        "reintro-palette.py",
        "reintro-reload-input.py",
        "reintro-row-hit-tests.py",
        "reintro-scroll-panes.py",
        "reintro-textview.py",
        "reintro-toolkit-focus.py",
        "test-gather-notices.py",
        "test-lane-claims.py",
        "test-reintro-palette.py",
    ),
    "D": (
        "check-duplicate-exports.py",
        "check-env-identity.py",
        "check-libc-abi.py",
        "check-libc-declared.py",
        "check-libc-overlay.py",
        "check-libc-prototypes.py",
        "check-libc-shape.py",
        "check-libc-target-warnings.py",
        "check-manifest-producers.py",
        "check-one-libc-per-process.py",
        "check-pinned-target-build.py",
        "convert-fastpy-embeds.py",
        "ctest-fixtures.py",
        "duplicate-exports-baseline.txt",
        "env-identity-baseline.txt",
        "extract-tcc-strace.sh",
        "find-reachable-fixtures.py",
        "gen-chmod-fixture.sh",
        "gen-human-fixture.sh",
        "p37-check.sh",
        "p38-check.sh",
        "probe-tcc-hosted.sh",
        "rootfs-bin-manifest.txt",
        "setup-toolchain.sh",
        "test-ctest-fixtures.py",
        "test-rootfs-staging.sh",
    ),
    "E": (
        "check-diskcleanup-test-roots.py",
        "check-frame-needles.py",
        "check-key-release-wiring.py",
        "check-tick-wiring.py",
        "check-unused-exports.py",
        "count_centrings.py",
        "find-claimed-acts.py",
        "find-drawn-only-settings.py",
        "find-echoed-settings.py",
        "find-options-only-emptied.py",
        "find-overstated-records.py",
        "find-silent-incapacity.py",
        "find-stale-admissions.py",
        "find-stale-dead-code-allows.py",
        "find-stranded-serialisers.py",
        "find-swallowed-ticks.py",
        "find-unpinned-picker-routing.py",
        "frozen-flag-survey.py",
        "key-survey-answered.txt",
        "key-survey-baseline.txt",
        "key-survey.py",
        "lossy-decode-baseline.txt",
        "lossy-decode.py",
        "mutation_harness.py",
        "reintro-benchmark.py",
        "reintro-credmanager.py",
        "reintro-lockscreen.py",
        "reintro-spreadsheet.py",
        "reintro-sysinfo.py",
        "sabotage.py",
        "scan-unwired.py",
        "stillreports.py",
        "test-mutation_harness.py",
        "verify_mutations.py",
    ),
    "F": (
        "check-generated-tables.py",
        "q45_apply.py",
        "q45_survey.py",
        "reintro-evdev.py",
    ),
}

#: Files only the operator edits: the instructions, the original design texts,
#: the operator's own notes, reference images and backups. A lane does not edit
#: these unless the operator says to (CLAUDE.md: "Do not edit this file during
#: normal development").
OPERATOR_OWNS: tuple[str, ...] = (
    ".claude/",
    "backups/",
    "CLAUDE.md",
    "claude.md.bak",
    "design.txt",
    "design desicions.txt",
    "other design decisions.txt",
    "design-review.txt",
    "differences from windows.txt",
    "ipc.txt",
    "scheduler.txt",
    "memory management.txt",
    "os.md",
    "api.txt",
    "answer.md",
    "operator-answers-2026-06-13.md",
    "convo1.txt",
    "claude_use.txt",
    "dual_use.txt",
    "effort_level.txt",
    "names.txt",
    "resetdate.txt",
    "todo.old.txt",
    "todo.old.2.txt",
    "todo3.txt",
    "roadmap.single-agent.md",
    "Aero Desktop (offline).html",
    "aero-window-frame.png",
    "file explorer.png",
    "slate os.png",
    "go.bat",
    "push.bat",
)

#: Files every lane writes, each under a named per-lane rule. A rule that says
#: who writes which part is an owner, not an ambiguity. The rules themselves
#: are in roadmap.md -> "Six-Agent Parallel Execution" rules 3 and 4.
SHARED_DOCUMENTS: dict[str, str] = {
    "roadmap.md": "rule 3: your lane's own items and sections",
    "roadmap-detailed.md": "rule 3: status flags in place, nothing deleted",
    "roadmap-done.md": "rule 3: your lane's finished items, moved there by check-docs.py --fix-roadmap",
    "known-issues.md": "a signpost since the 2026-10-02 cutover: the entries are known-issues/",
    "known-issues-resolved.md": "a signpost: the entries are known-issues-resolved/",
    "design-decisions.md": "a signpost: the entries are design-decisions/",
    "open-questions.md": "a signpost: the entries are open-questions/",
    "deferred-questions.md": "a signpost: the entries are deferred-questions/",
    "awaiting-operator.md": "rule 3: items with your lane's letter",
    "todo.txt": "rule 3: your own `## Lane <X>` headings",
    "manual-testing.txt": "rule 3: items for your own lane's features",
    "README.md": "rule 3: the paragraphs about your own subsystems",
    "Cargo.toml": "rule 4: the workspace manifest's member and dependency lists",
    "Cargo.lock": "rule 4: regenerated by cargo, never hand-edited",
    "scripts/INDEX.md": "generated by scripts/gen-script-index.py: every lane adding a script regenerates it, nobody hand-edits it",
}

#: The per-entry documents (one file per entry since the 2026-10-02 cutover,
#: `scripts/docs-migrate.py`): every lane writes its own entries in them under
#: roadmap.md rule 3, so a directory here is a shared document as the single
#: files before it were.
SHARED_DOCUMENT_DIRS: dict[str, str] = {
    "known-issues/": "rule 3: one file per open issue, your lane's own",
    "known-issues-resolved/": "rule 3: one file per closed issue, your lane's own",
    "design-decisions/": "rule 3: one file per decision, in your lane's band",
    "open-questions/": "rule 3: one file per open question with your lane's letter",
    "open-questions-resolved/": "rule 3: one record file per lane, your lane's own",
    "deferred-questions/": "rule 3: one file per deferred question, your lane's own",
}

#: The baseline `docs-carry-forward.py` writes for the lane that carried its
#: entries across the cutover is that lane's: `scripts/docs-baseline-carried-<x>.json`.
_CARRIED_BASELINE = re.compile(r"^scripts/docs-baseline-carried-([a-f])\.json$")

#: A request's owner is its sender: `requests/<from>-<to...>-<slug>.md`.
_REQUEST_NAME = re.compile(r"^requests/([a-f])-[a-f]+-[^/]+$")

#: What `ownership()` answers.  `kind` is "lane", "operator", "shared" or
#: "none"; `lane` is the letter for a lane; `rule` says why.
class Ownership:
    __slots__ = ("kind", "lane", "rule")

    def __init__(self, kind: str, lane: str | None, rule: str) -> None:
        self.kind = kind
        self.lane = lane
        self.rule = rule

    def __repr__(self) -> str:
        return f"Ownership({self.kind!r}, {self.lane!r}, {self.rule!r})"

    def label(self) -> str:
        """One word for a report: the lane letter, `operator`, `shared` or `-`."""
        if self.kind == "lane":
            return self.lane or "-"
        return {"operator": "operator", "shared": "shared"}.get(self.kind, "-")


#: What the briefing says about the files no lane owns.
UNASSIGNED_NOTE = (
    "the operator's files (CLAUDE.md, the design texts, notes, backups) and the "
    "shared documents, each under its roadmap.md rule 3/4 -- see "
    "`--owner PATH`; nothing else (design-decisions 973)"
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


def _build_tables() -> tuple[dict[str, str], list[tuple[str, str]]]:
    """Exact-file and directory-prefix lookups from OWNERSHIP + SCRIPT_OWNERS.

    An exact entry is always the longest possible match for its own path, so
    looking it up first and falling back to the longest directory prefix is
    the same rule as "longest prefix wins", at dictionary speed -- which the
    gate needs, since it asks about every tracked file.
    """
    exact: dict[str, str] = {}
    prefixes: list[tuple[str, str]] = []
    for prefix, lane in OWNERSHIP:
        if prefix.endswith("/"):
            prefixes.append((prefix, lane))
        else:
            exact[prefix] = lane
    for lane, names in SCRIPT_OWNERS.items():
        for name in names:
            exact["scripts/" + name] = lane
    prefixes.sort(key=lambda pl: len(pl[0]), reverse=True)
    return exact, prefixes


_EXACT, _PREFIXES = _build_tables()


def _lane_of_rel(rel: str) -> str | None:
    """The lane owning a normalised repo-relative path, or None."""
    if not rel:
        return None
    hit = _EXACT.get(rel)
    if hit is not None:
        return hit
    m = _REQUEST_NAME.match(rel)
    if m:
        return m.group(1).upper()
    for prefix, lane in _PREFIXES:
        if rel.startswith(prefix) or rel == prefix[:-1]:
            return lane
    return None


def owner_of(path: str | os.PathLike[str]) -> str | None:
    """The letter of the lane that owns `path`, or None if no lane does.

    Longest prefix wins (see `OWNERSHIP`); a file named in `SCRIPT_OWNERS` or a
    request named by its sender is an exact answer.  A directory may be named
    with or without its trailing slash.  None means "no lane's tree" -- an
    operator file or a shared document, or a path nothing covers, which
    `ownership` tells apart -- never "anyone's".
    """
    return _lane_of_rel(_normalise(path))


def ownership(path: str | os.PathLike[str]) -> Ownership:
    """Who owns `path`: a lane, the operator, a shared-document rule, or nobody."""
    rel = _normalise(path)
    lane = _lane_of_rel(rel)
    if lane is not None:
        return Ownership("lane", lane, "the ownership table")
    if rel in SHARED_DOCUMENTS:
        return Ownership("shared", None, SHARED_DOCUMENTS[rel])
    for prefix, rule in SHARED_DOCUMENT_DIRS.items():
        if rel.startswith(prefix):
            return Ownership("shared", None, rule)
    carried = _CARRIED_BASELINE.match(rel)
    if carried:
        return Ownership("lane", carried.group(1).upper(), "the lane that carried its entries")
    for entry in OPERATOR_OWNS:
        if rel == entry or (entry.endswith("/") and
                            (rel.startswith(entry) or rel == entry[:-1])):
            return Ownership("operator", None, "only the operator edits it")
    return Ownership("none", None, "nothing in the table covers it")


def unowned(paths: list[str]) -> list[str]:
    """The paths among `paths` that nothing owns."""
    return [p for p in paths if ownership(p).kind == "none"]


def stale_entries(paths: list[str]) -> list[str]:
    """Exact-file table entries that name no tracked file: a script deleted,
    or renamed, whose line stayed behind.

    Only exact entries.  A directory entry with nothing under it is a
    *reservation*, not a stale line -- `gui/video/` was claimed for lane F on
    2026-09-27, before the crate's first file existed, which is what lets that
    first commit land with the right owner instead of falling to `gui/`'s.
    """
    tracked = set(paths)
    exact = [e for e in list(_EXACT) + list(SHARED_DOCUMENTS) + list(OPERATOR_OWNS)
             if not e.endswith("/")]
    return sorted(e for e in exact if e not in tracked)


def duplicate_scripts(
    table: Mapping[str, tuple[str, ...]] | None = None,
) -> list[str]:
    """Script names listed under more than one lane in `SCRIPT_OWNERS` (or in
    `table`, which is how the self-test feeds it a fixture)."""
    seen: dict[str, str] = {}
    dups = []
    for lane, names in (SCRIPT_OWNERS if table is None else table).items():
        for name in names:
            if name in seen:
                dups.append(f"{name} ({seen[name]} and {lane})")
            seen[name] = lane
    return dups


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
    scripts = SCRIPT_OWNERS.get(letter, ())
    if scripts:
        out.append(f"{len(scripts)} files in scripts/ (SCRIPT_OWNERS)")
    out.append(f"requests/{letter.lower()}-*.md (the requests it sent)")
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
    rows.append(f"-  operator {'':<26} {', '.join(OPERATOR_OWNS)}")
    rows.append(f"-  shared   {'':<26} "
                + ", ".join(f"{doc} ({rule})" for doc, rule in SHARED_DOCUMENTS.items()))
    return "\n".join(rows)


def _tracked_files(head: str | None) -> list[str] | None:
    """Every tracked path: the index's, or revision `head`'s.  None on a git
    failure -- which the gate reports as no verdict, never as a pass."""
    import subprocess
    if head is None:
        cmd = ["git", "-C", str(PROJECT_ROOT), "ls-files", "-z"]
    else:
        cmd = ["git", "-C", str(PROJECT_ROOT), "ls-tree", "-r", "-z",
               "--name-only", head]
    try:
        out = subprocess.run(cmd, capture_output=True, check=True).stdout
    except (OSError, subprocess.CalledProcessError):
        return None
    return [p for p in out.decode("utf-8", "surrogateescape").split("\0") if p]


def check_all(head: str | None) -> int:
    """The §973 gate: every tracked file has an owner, and every table entry
    names something tracked.  0 clean, 1 findings, 2 no verdict."""
    paths = _tracked_files(head)
    if paths is None:
        print("which-lane: cannot list the tracked files "
              f"({'the index' if head is None else head}) -- no verdict.",
              file=sys.stderr)
        return 2
    # A floor: a listing this short means git answered for the wrong tree
    # (or none), and "0 unowned" over it would be a verdict about nothing.
    if len(paths) < 1000:
        print(f"which-lane: only {len(paths)} tracked file(s) listed -- fewer "
              "than this tree has ever had, so no verdict.", file=sys.stderr)
        return 2
    lost = unowned(paths)
    stale = stale_entries(paths)
    dups = duplicate_scripts()
    for p in lost:
        print(f"unowned: {p}")
    for e in stale:
        print(f"stale:   {e} (a table entry naming nothing tracked)")
    for d in dups:
        print(f"twice:   {d}")
    if lost or stale or dups:
        print(f"\nwhich-lane: {len(lost)} unowned file(s), {len(stale)} stale "
              f"entr{'y' if len(stale) == 1 else 'ies'}, {len(dups)} script(s) "
              "owned twice.\n"
              "Every tracked file has exactly one owner (design-decisions 973).\n"
              "Give a new file its owner in the commit that creates it:\n"
              "  * a script directly in scripts/: add its name to your lane's "
              "block of SCRIPT_OWNERS\n"
              "    in scripts/which-lane.py (the one edit to that file that "
              "needs no request);\n"
              "  * a request: name it requests/<your letter>-<to>-<slug>.md;\n"
              "  * anything else new at the top of the tree: ask lane A, "
              "which owns the table.\n"
              "A stale entry is a line whose file is gone: delete the line.",
              file=sys.stderr)
        return 1
    print(f"ok -- all {len(paths)} tracked file(s) have an owner "
          f"({'the index' if head is None else head}).")
    return 0


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
        ("gui/video/rav1d/src/lib.rs", "F"),
        ("gui/video", "F"),
        ("gui/compositorx/src/lib.rs", "C"),
        ("gui/videos/src/lib.rs", "C"),
        # §973: scripts, root crates and requests have lanes now.
        ("scripts/check-eol.py", "A"),
        ("scripts/hooks/pre-push", "A"),
        ("scripts/run-checker.sh", "A"),
        ("scripts/cat-diff.sh", "B"),
        ("scripts/check-window-wiring.py", "C"),
        ("scripts/check-libc-abi.py", "D"),
        ("scripts/bash-spike/run.sh", "D"),
        ("scripts/key-survey.py", "E"),
        ("scripts/espeak-spike/run.sh", "E"),
        ("sha2/src/lib.rs", "A"),
        ("procinfo/src/lib.rs", "B"),
        ("deflate/src/lib.rs", "E"),
        ("requests/a-bc-some-slug.md", "A"),
        ("requests/c-abf-the-operator-answered.md", "C"),
        ("requests/f-a-x.md", "F"),
        ("requests/g-a-x.md", None),
        ("requests/A-b-uppercase-is-not-a-sender.md", None),
        ("requests/a-b-sub/dir.md", None),
        ("requests/osb2_needs_compositor_syscalls.md", "C"),
        ("requests/.gitkeep", "A"),
        # Operator files and shared documents are still no lane's tree.
        ("CLAUDE.md", None),
        ("Cargo.toml", None),
        ("roadmap.md", None),
        ("", None),
    ]:
        check(f"owner_of({path!r})", owner_of(path), want)

    for path, want in [
        ("kernel/src/main.rs", "lane"),
        ("CLAUDE.md", "operator"),
        ("backups/three-lanes/README.md", "operator"),
        ("backups", "operator"),
        ("roadmap.md", "shared"),
        ("Cargo.lock", "shared"),
        ("known-issues.md", "shared"),
        # One file per entry since the 2026-10-02 cutover: the directories
        # are shared documents as the single files were.
        ("known-issues/A-SOME-ISSUE.md", "shared"),
        ("known-issues-resolved/TD-B-SOME-ISSUE.md", "shared"),
        ("design-decisions/1529-some-decision.md", "shared"),
        ("open-questions/C-Q1.md", "shared"),
        ("open-questions-resolved/lane-d.md", "shared"),
        ("deferred-questions/DQ1.md", "shared"),
        ("roadmap-done.md", "shared"),
        # ...but not a file merely named like one of them.
        ("known-issues-elsewhere.md", "none"),
        ("scripts/docs-baseline-carried-c.json", "lane"),
        ("scripts/docs-baseline-carried-z.json", "none"),
        ("a-brand-new-top-level-file.txt", "none"),
        ("scripts/a-brand-new-script.py", "none"),
    ]:
        check(f"ownership({path!r}).kind", ownership(path).kind, want)
    check("a carried baseline is the carrying lane's",
          ownership("scripts/docs-baseline-carried-c.json").label(), "C")
    check("ownership label of a lane path", ownership("apps/chess").label(), "E")
    check("ownership label of an operator file", ownership("design.txt").label(),
          "operator")

    # --- the gate's three findings, over a synthetic listing -------------------
    # Everything the tables name, as a tracked-file list: an exact entry as
    # itself, a directory entry as one file under it.
    def listing_of_tables() -> list[str]:
        out = []
        for entry in list(_EXACT) + [p for p, _ in _PREFIXES] + \
                list(SHARED_DOCUMENTS) + list(OPERATOR_OWNS):
            out.append(entry + "x" if entry.endswith("/") else entry)
        return out

    full = listing_of_tables()
    check("a listing of exactly the tables has nothing unowned",
          unowned(full), [])
    check("...and nothing stale", stale_entries(full), [])
    check("a new top-level file is unowned",
          unowned(full + ["new-thing.txt"]), ["new-thing.txt"])
    check("a new file under an owned directory is not",
          unowned(full + ["kernel/src/new.rs"]), [])
    dropped = "scripts/cat-diff.sh"
    check("a script deleted but still in the table is stale",
          stale_entries([p for p in full if p != dropped]), [dropped])
    check("a directory entry with nothing under it is a reservation, not stale",
          stale_entries([p for p in full if not p.startswith("scripts/lib/")]),
          [])
    check("a reserved directory still owns the first file created in it",
          owner_of("gui/video/src/lib.rs"), "F")
    check("the real table owns no script twice", duplicate_scripts(), [])
    check("a script under two lanes is reported",
          duplicate_scripts({"A": ("x.py", "y.py"), "B": ("x.py",)}),
          ["x.py (A and B)"])
    check("SCRIPT_OWNERS names only files directly in scripts/",
          [n for names in SCRIPT_OWNERS.values() for n in names if "/" in n], [])
    check("every SCRIPT_OWNERS key is a lane",
          sorted(k for k in SCRIPT_OWNERS if k not in LANES), [])
    check("no shared document or operator file also has a lane",
          [p for p in list(SHARED_DOCUMENTS) + list(OPERATOR_OWNS)
           if _lane_of_rel(p.rstrip("/")) is not None], [])

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
    parser.add_argument("--check-all", action="store_true",
                        help="the gate: refuse a tracked file with no owner, a "
                             "table entry naming nothing, or a script owned "
                             "twice (design-decisions 973)")
    parser.add_argument("--head", metavar="REV",
                        help="with --check-all: judge REV's files, not the index")
    parser.add_argument("--self-test", "--selftest", "--self_test",
                        dest="selftest", action="store_true",
                        help="run this script's own fixtures and exit")
    args = parser.parse_args(argv)

    if args.selftest:
        return _self_test()
    if args.head and not args.check_all:
        parser.error("--head only means something with --check-all")
    if args.check_all:
        return check_all(args.head)
    if args.table:
        print(table())
        return 0
    if args.owner:
        for path in args.owner:
            own = ownership(path)
            note = "" if own.kind == "lane" else f"\t({own.rule})"
            print(f"{own.label()}\t{path}{note}")
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
