## 1100. Six lanes, two per account: where the tree is cut, and how a session knows which lane it is

**Date:** 2026-09-22
**Lane:** D
**Decided by:** Claude (operator-approved scope) — the operator decided six
lanes, two sessions per Claude account, named "Lane A" … "Lane F"; Claude drew
the split and made every call below it, and the operator may overrule any of
them.

**In short:** the project now runs six agents at once instead of three, two on
each Claude account. Each agent owns part of the source tree, so that no two of
them ever edit the same file. The operator chose the number and the names. This
entry records how the tree was divided six ways, how an agent now works out
which lane it is — the old way stopped working — and the handful of mechanical
changes that had to follow. It is numbered as lane D's first entry only because
it was written on `main` before any lane-D session existed, which made D's band
the one place no other lane could be writing at the same moment.

### The split

| Lane | Name | Owns |
|---|---|---|
| A | Kernel, Core & Networking | `kernel/**`, `bench/**`, the boot test — and now `net/**`, `netipc/**`, `netproto/**`, `netring/**`, `net80211/**`, `aes/**`, `hmac/**`, `services/netstack/**` |
| B | Userland | `userspace/**`, `init/**` |
| C | Desktop & Toolkit | `gui/**` except F's crates |
| D | POSIX, libc & Toolchain | `posix/**`, `services/**` except `netstack`, `toolchain/stubs/**`, `build-sysroot.ps1`, `create-ext4-rootfs.sh` |
| E | Applications | `apps/**`, `randrange/**` |
| F | Graphics Stack | `gui/compositor`, `gui/window`, `gui/remote`, `gui/font`, `gui/imagecodec`, `gui/vulkan` |

Each old lane had one part with a deep backlog and one part coupled tightly to a
*different* lane, and the cuts follow those seams:

- **Networking went to the kernel lane.** The kernel links `netipc` and
  `netring`; `services/netstack` is a thin daemon over the same crates; and the
  WiFi work had been a request-and-reply loop between lanes A and C for weeks.
  Every open networking item needed lane A already. Lane A was also the lane
  with spare capacity — its own roadmap table said on 2026-09-14 and again on
  2026-09-17 that it was down to operator-gated items.
- **Lane B split at the libc boundary.** `posix/`, the sysroot, the C test
  fixtures and the rootfs recipe are one pipeline with one owner, and the large
  ports that stand on libc (gcc, CPython, fastpy self-hosting, the Rust
  toolchain, WINE — B-Q18's list) belong with it. The userspace programs stayed
  with lane B, which is where B's recent commits and nearly all of its open
  questions were; so B-Q8 … B-Q21 still mean what they say.
- **Lane C split three ways.** `apps/` (142 crates) was as busy as all of
  `userspace/` — 572 commits in the 14 days before the split against 580 — and
  became lane E. `gui/` divides at the display protocol: the compositor, the
  protocol (`gui/remote`) and the window library every application links
  (`gui/window`) stay together in lane F, so a protocol change never needs a
  handshake, and F also takes text rendering, image decoding, the Vulkan loader
  and the GPU ports that were C's deepest backlog. The desktop shell, toolkit,
  appearance and desktop services stayed lane C.

**Alternatives considered for the split:**

- *Halve each old lane (A→A+D, B→B+E, C→C+F).* Symmetric and easy to remember.
  Rejected: lane A was already short of unblocked work, so halving it makes two
  idle lanes; and the kernel has no seam that avoids shared files (the syscall
  tables, `main.rs`, the capability code), so kernel-core against kernel-drivers
  would have been a constant stream of requests.
- *Networking as a lane of its own.* Rejected: about 40 files and 8 commits in
  the 14 days before the split, and every open item needs the kernel side — a
  lane that would mostly have filed requests to lane A.
- *A lane for `scripts/`.* It is the busiest directory by far (989 commits in
  those 14 days), but lanes write gates for their own code, and a central owner
  would turn every new gate into a request. Who owns `scripts/` is also
  `open-questions.md` A-Q11, still with the operator; this split does not
  answer it and does not assign `scripts/`.
- *Split `apps/` or `userspace/` alphabetically.* Balanced by construction, but
  the halves share nothing except a letter, so every sweep and every
  cross-application change would span two lanes.

### How a session knows its lane

Under three lanes each Claude account ran one session, and
`scripts/which-lane.py` read nothing but `CLAUDE_CONFIG_DIR`. With two sessions
per account that names two lanes, so it now identifies nothing and is only
printed. The lane comes from the **agent name** (`--agent-name`, or
`SLATEOS_LANE` / `ORCH2_AGENT_NAME` in the environment) and from the
**worktree** the script lives in (`os-lane-<x>`, with `lane-<x>` actually
checked out, read from `.git` rather than by running git). Every source present
must agree.

- *Why not the worktree alone:* the operator's launcher starts all six sessions
  in `os`, so for an agent's first command the worktree often says nothing.
- *Why not the name alone:* orchestrator2 does not export the name to child
  processes, so a script cannot see it unless it is passed or the launcher sets
  it; the worktree covers every script an agent runs inside its own tree.
- *Why refuse a disagreement instead of preferring one source:* a session named
  Lane D running a script from `os-lane-a` is operating in lane A's tree, and
  the one thing this arrangement exists to prevent is exactly that.

The same file became the **single ownership table** (longest matching prefix
wins, which is how `gui/compositor/` → F carves out of `gui/` → C), and the
scripts that kept their own copies now import it. Two of those copies had
already drifted from it — `merge-readiness.py` gave lane A all of `toolchain/`
and `pre-boot.py` gave lane C a `pkg/` that has never existed — which is the
usual fate of a mirror.

### What else had to change

1. **Deferred questions were renamed `D-Q<n>` → `DQ<n>`.** `D-` is lane D's
   question prefix now, and the gates that parse question ids would have read
   D-Q2 (install clang and enable CFI) as a lane-D question. DQ1 and DQ2 carry
   their old names in their headings, and **lane D's own numbering starts at
   D-Q3**, so D-Q1 and D-Q2 are never reissued — the rule the section numbers in
   this file already follow. Old citations in `requests/` and in the entries of
   this file were left as written; the live documents use the new names.
2. **Empty bands take their anchor from the band row** (`check-design-decisions-
   bands.py`): see the header of this file.
3. **Every gate that parses a lane letter accepts A–F**, and
   `scripts/open-requests.py` now parses requests addressed to several lanes
   (`a-bc-…`) — twenty such files had been skipped by the three-lane parser.
4. **The publish step follows §538.** The operator decided on 2026-08-21 that
   lanes publish with a fast-forward `git push origin <sha>:main` from their
   own worktree, but `CLAUDE.md` still told agents to merge inside `os`, and
   agents may not edit `CLAUDE.md`. This rewrite was the operator's instruction
   to edit it, and six lanes publishing make fifteen pairs that can race in the
   shared checkout where three made three, so both documents now say §538.
5. **Retagging.** Open roadmap items were retagged to their new owner; finished
   items keep the letter of the lane that finished them, and prose inside an
   item that names a lane is history.
6. **Three worktrees** — `os-lane-d`, `os-lane-e`, `os-lane-f`, on branches
   `lane-d`, `lane-e`, `lane-f` — were created from `main` at this change.
7. **`CLAUDE.md`'s phantom `pkg/**` is gone**, which settles C-Q27 without a
   decision: lane C had asked leave to delete it from a file only the operator
   may edit, and the operator's instruction to rewrite the lane section covered
   it. The new table was written from `scripts/which-lane.py`'s, which was
   already right.

### Left open, deliberately

- **A-Q11** (who may edit `scripts/` and the small top-level library crates)
  and **A-Q18** (every lane appends at the end of `known-issues.md`, which
  conflicted eleven times in one day under three lanes) both get worse with six
  lanes, and both are the operator's to answer.
- orchestrator2 refuses a space in an agent name, so "Lane A" registers as
  `Lane-A` unless orchestrator2 changes; the scripts accept both spellings.
- `E:` had 312 GB free for six lanes' build caches. `roadmap.md` Step 0.6 says
  so, but the constraint is physical, not procedural.

**Where it lives:** `scripts/which-lane.py` (`OWNERSHIP`, `LANES`,
`detect_lane`, `owner_of`); `roadmap.md` → "Six-Agent Parallel Execution";
`CLAUDE.md` → "Six Sessions"; the band rows at the top of this file;
`backups/three-lanes/`.

**How to reverse:** `backups/three-lanes/README.md` describes going back to
three lanes, including what must not be copied back wholesale. Changing the
split without going back is one commit: edit `OWNERSHIP` in
`scripts/which-lane.py` and the table in `roadmap.md` together, and retag the
open items that move. The gates need nothing, because they accept A–F.
