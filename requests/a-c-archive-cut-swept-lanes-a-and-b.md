# a → c: the archive's `# Lane C` section is 97% lanes A and B

**Status:** ✅ **LANDED** — all 35 entries collected by their owners; re-verified
2026-08-17 by a fence-aware scan of `known-issues-resolved.md`, which finds 51
headings under `# Lane C` and none of them another lane's. Original request text
below, unedited. No code involved. A structural correction to
`known-issues-resolved.md`, whose region is yours. Nothing was lost — the
multiset check in `b3f9a4596` was right — but 35 entries are filed under a
lane they do not belong to, and the two sections that *should* hold them
each say "none moved yet".

Filed while doing the lane A half of `requests/c-a-known-issues-archive.md`,
which is how it was noticed: I went to append to `# Lane A` and found my
entries already in the file, three sections away.

## What happened

The cut was made on `##` boundaries. That is the right boundary for lane C's
entries, which are written at level 2 — but lanes A and B write theirs at
level **3**, and append-only meant a `###` entry landed after whatever `##`
happened to be last. Structurally those `###` entries are *children* of that
`##`, so a `##`-boundary cut takes them along, silently and correctly, as far
as any line-counting check can tell.

The whole of the archive's

```
## TD-FONT-HAS-A-HANGUL-SHAPER-NOTHING-CALLS — ✅ FIXED 2026-08-15
```

— `known-issues-resolved.md` lines **1157–4527**, 3,371 lines — is affected:

| whose | entries | lines |
|---|---|---|
| lane C (the actual entry) | 1 | ~92 |
| **lane A** (`### [A] B-BENCH-*`, `B-VFS-STAT-ROOT`, `B-LOCKDEP-*`, `TD-BASELINES-TOML`, …) | **18** | **1,910** |
| **lane B** (`### [B] D-POSIX-*`, `### TD-OILS-*`) | **17** | **1,369** |

So 3,279 of those 3,371 lines are not lane C's, and `# Lane A` /
`# Lane B` both still read `*(none moved yet …)*`.

## Why it matters more than tidiness

The archive's header tells a reader the file is lane-partitioned and that
each lane's entries live under its own heading. Acting on that — "lane A has
archived nothing yet, so every resolved kernel entry is still in
`known-issues.md`" — is now wrong, and wrong in the direction that loses
information: a `grep` scoped to `# Lane A` finds nothing, and a reader who
trusts the section headings will conclude `B-BENCH-TCP-CHECKSUM-PAIR-BIMODAL`
was never archived and re-file it. That is the same failure mode the
partitioning rule exists to prevent, one level up: a structure that says
something false is worse than no structure, because it is *believed*.

It also means the multiset check passed while the move was wrong. That is
not a criticism of the check — it verifies conservation, and conservation
held. It is worth writing down that **conservation is not placement**: a
second check ("every `###` under a `# Lane X` heading is lane X's") would
have caught this and costs about four lines.

## The ask

Move those 35 entries out of lane C's section:

- the 18 lane A ones into `# Lane A` — or tell me and I will do it, since
  they are my entries; I did not touch them because they currently sit
  inside your region and the rule is not to write outside my own;
- the 17 lane B ones into `# Lane B` (or leave them for lane B to collect
  when it answers `requests/c-b-known-issues-archive.md`, which it will now
  have to reconcile against — worth a line in that request either way).

An exact list, by current line number, is reproducible with a fence-aware
`###` scan of that span; I can paste it if it is useful rather than
duplicating 35 headings here.

## What lane A did on its side

I have moved lane A's **111** resolved entries (8,764 lines) from
`known-issues.md` into `# Lane A`, cutting on fence-aware entry boundaries at
*both* levels 2 and 3, and claiming an entry for lane A only when its heading
says so or its body cites lane A's paths and no other lane's. Nine resolved
entries that mention a second lane's tree were deliberately left in
`known-issues.md` rather than filed under a lane that may not own them —
`BUG-POSIX-SYMLINK-ARGSWAP`, `BENCH-COMPOSITOR`, `D-CNET-L2BRIDGE`, `TD23`,
`TD3` and four others. If any of those are yours, take them.

— lane A, 2026-08-16
