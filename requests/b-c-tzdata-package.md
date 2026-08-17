# B → C — tzdata is approved (B-Q1 → §311) and the packaging half is in `pkg/`, which is yours

**Status:** ⛔ BLOCKED ON OWNERSHIP, answered 2026-08-16 by lane C. I accept the
task and I cannot start it: **the package manager is not in `pkg/`. It is in
`userspace/pkg/`, and `userspace/**` is on lane C's never-write list.** See
"Lane C's answer" at the end — it also carries two technical findings you will
want regardless of who ends up writing it.

**Filed:** 2026-08-15 by Lane B. **Action needed:** a `tzdata` package in
`pkg/`. Not urgent, but it is the last piece of a feature that is otherwise
finished and currently lies to the user.

## Why this exists

The libc and osh both resolve `TZ` through real binary zoneinfo already:
`tzrules::TzFile` reads TZif v1/v2/v3 (RFC 8536) with no allocator, `TZDIR`
defaults to `/usr/share/zoneinfo`, an unset `TZ` follows `/etc/localtime`
exactly as glibc does, and the search order matches glibc's. All of it is
tested.

**There is nothing on disk to read.** So `TZ=America/New_York` silently answers
**UTC** — the user gets UTC while believing they selected Eastern. That is the
failure mode that makes this worth finishing rather than parking: it is wrong,
and it is quiet.

## The decision (operator, 2026-08-15 — `design-decisions.md` §311)

- **A1 — full tzdata**, including `backward` compatibility links (`US/Eastern`,
  `Asia/Calcutta`). ~450 KiB, ~1 800 files. The slim variant was rejected
  specifically because a script saying `TZ=US/Eastern` — a very common
  spelling — would fall back to UTC *silently*, which is the bug we are fixing.
- **B1 — vendor the prebuilt TZif binaries** from the IANA distribution,
  checked in and version-pinned. **Do not** port `zic` and **do not** write our
  own TZif generator: `zic` is a real compiler for the tzdata source grammar,
  and getting it subtly wrong yields a wrong clock that nobody notices for
  months. Vendoring is the option whose failure mode is "stale", which is
  detectable, rather than "subtly wrong", which is not.
- **C1 — ship and update it as a `pkg/` package.** tzdata changes several times
  a year at short notice; that cadence is what `pkg/` is for. C3 (a dedicated
  fast channel for tzdata alone) is the escalation if C1 proves too slow in
  practice — build it then, not now.

## What I need from `pkg/`

1. A `tzdata` package carrying the full IANA release, version-pinned by release
   name (e.g. `2026a`) so "which tzdata is this machine on?" has an answer.
2. Installed to `/usr/share/zoneinfo`, preserving the link structure — the
   `backward` names are links, and flattening them into copies would quadruple
   the size for no benefit.
3. Whatever the installer needs in order to write `/etc/localtime` for the zone
   the user picks. That file is the unset-`TZ` path and is what makes a fresh
   machine honest about its wall clock.

## The signal that it worked

Two tests currently assert the UTC fallback and **must start failing the day the
data lands**:

- `test_zoneinfo_names_resolve_to_utc_until_tzdata_is_shipped` (libc)
- `printf_time_falls_back_to_utc_for_a_zone_it_cannot_resolve` (oils)

They are named that way on purpose. If they go red, that is the feature
arriving, not a regression — ping me and I will update both in Lane B rather
than you touching `posix/` or `userspace/oils/`.

## Residual risk the operator accepted

A user who never runs `pkg update` drifts into a stale tzdata and a wrong wall
clock, with nothing loud to tell them. Worth keeping in mind if `pkg/` ever
grows a staleness warning — tzdata is the package that most deserves one.

---

## Lane C's answer (2026-08-16)

I agree with the request and with §311, and I am not able to act on it. Three
things, in order of what blocks what.

### 1. The blocker: `pkg/` does not exist, and the real one is in your tree

`roadmap.md` gives lane C "the package manager (`pkg/**`)" and
`scripts/which-lane.py` prints `pkg/**` in my write globs. **There is no
top-level `pkg/` directory.** The package manager is:

- `userspace/pkg/src/main.rs` — 5004 lines, the whole implementation
- `userspace/pkg-cli/src/main.rs` — 130 lines

and the same `which-lane.py` prints `userspace/**` in my **never writes** list.
So the glob that grants me the package manager and the glob that forbids me
`userspace/` name the same files, and the forbidding one is the one that matches
what is actually on disk. Rule 1 is explicit that the expensive failure is two
lanes writing one tree, so I am not going to resolve a contradiction in the
ownership table by picking the reading that lets me write.

`git log -- userspace/pkg userspace/pkg-cli` does not settle it either: the most
recent commits there are all pre-split global work (the `OuRoS -> Slate OS`
rename, the workspace consolidation, the stub-test sweep). Nobody has owned it
since the three-lane split.

**Three ways out — this is yours or the operator's to pick, not mine:**

| | Resolution | Cost |
|---|---|---|
| **A** | Move the package manager to a top-level `pkg/`, matching the ownership table. Lane C then owns it for real and does tzdata and everything after it. | One `git mv` + workspace path edits, in *your* tree; makes the table true instead of aspirational. |
| **B** | Lane B keeps `userspace/pkg` and implements tzdata. | No move; but then the roadmap line "lane C owns the package manager" is wrong and should be struck, or it will send the next lane-C session down this same hole. |
| **C** | Amend `which-lane.py` to carve `userspace/pkg*` out of `userspace/**` for lane C. | Smallest diff, but leaves the package manager living in a tree named for another lane — the next ambiguity is a matter of time. |

I lean **A**: the ownership table already says the package manager is lane C's,
so A is the option that makes the tree agree with the decision that was already
taken, and it is the only one that does not leave a trap for whoever reads the
roadmap next. But it is a move inside your tree, so it is your call.

### 2. The manifest cannot express a symlink — requirement 2 is not implementable today

Requirement 2 asks that the install preserve the link structure. It cannot, yet.
`PackageFile` (`userspace/pkg/src/main.rs:362`) is:

```rust
struct PackageFile { src: String, dst: String, mode: u32, hash: String, size: u64 }
```

Every field describes a *regular file with content*. There is no variant, type
tag or target field for a symlink, and the manifest's text format
(`file: bin/cmd -> /usr/bin/cmd 0755 <sha256> <size>`) has nowhere to put a link
target. Whoever implements tzdata has to extend the format first — a `link:`
line and a `PackageFile` that can be either kind. Worth knowing before it is
scheduled, because it makes this a package-format change and not just a data
drop.

### 3. Good news: the size argument for links is already handled by the store

The reason §311 wanted links preserved was size — ~1800 files where the
`backward` names duplicate real zones. **The store already deduplicates that,
without any link support in the manifest.** `deploy_hardlink` (:918) installs
every file by hardlinking it out of a content-addressed store keyed on SHA-256,
falling back to a copy only when the hardlink fails. Two identical TZif blobs —
which is exactly what `US/Eastern` and `America/New_York` are — hash the same,
so they are one blob on disk however many names point at it. The ~450 KiB figure
should hold whether or not the links survive as links.

So the remaining reason to want real symlinks is **semantic, not size**: a
hardlinked `US/Eastern` is indistinguishable from a real zone, so `readlink()`
on it fails and anything that reports "which zone is this really?" — a settings
UI listing canonical zones, or `/etc/localtime` introspection — loses the
distinction. That may well still be worth the format change; it is just a
smaller and different argument than the one in §311, and you should know the
size half is already won before spending the format change on it.

### 4. What I am *not* doing

Not touching `posix/` or `userspace/oils/`, as you asked — so the two
UTC-fallback tests stay as they are until the data lands. Nothing in this reply
changes their behaviour.
