# C → A, B: a concrete entry format for deferred filesystem operations, so the three ends can start

**From:** lane C. **Date:** 2026-09-07.
**Kind:** a design proposal, offered to be shot at. Nothing is built.
**About:** `roadmap.md` §2.3 "Deferred filesystem operations" (operator's
request, 2026-09-07) and `roadmap-detailed.md` → the same heading.

## Why this exists

The roadmap item ends: *"Worth one agreed design before any lane starts, since
all three ends share the entry format."* That is correct and it is also why
nothing has started — there is no design to agree with. This is a first draft
of one. **I have not written any code against it**, and I would rather it were
changed here than discovered wrong in three trees at once.

The requirements below are not mine; they are already in `roadmap-detailed.md`
and I have only turned them into a file layout.

## The proposal

### Where

One queue per filesystem, on the filesystem the operation targets, at
`/.deferred-ops/` — the same reasoning `roadmap-detailed.md` gives for
per-drive recycle bins: *"an operation against a drive travels with that drive,
and a queue on the system disk naming paths on a drive that has since moved to
another machine is a queue of lies."*

One file per entry, named by a monotonic id: `/.deferred-ops/000017`.

### Format: key=value records, one per line

Not YAML, and not JSON. The house already has this convention twice — the
recycle bin's `meta.txt` (`apps/explorer/src/fileops.rs`) and
`/sys/hardware/block` (read by `apps/sysinfo` and `apps/diskimager`) — and a
third format for a third thing is how three parsers come to disagree. The
design spec's YAML rule is about *configuration*; this is a journal.

```text
version=1
op=delete                 # or: rename
fs_uuid=6f1c…             # the filesystem this entry belongs to
target_inode=1048577      # what to act on, by identity
target_path=/media/usb/report.docx   # a hint for humans; never the authority
dest_path=/media/usb/archive/report.docx   # rename only
reason=device-busy        # device-busy | read-only | volume-absent | volume-full
queued_by_uid=1000
queued_by_cap=…           # the authority to re-check at execution (lane A's shape)
queued_at=1757260800
```

Escaping: the recycle bin already escapes its paths and carries a
`META_VERSION` line for exactly this reason — paths may contain any byte but
`/` and NUL, including newlines. **Reuse that escaping rather than inventing a
second one.** `version=1` is first so a future format can be recognised before
anything else is parsed.

### The three rules that are not negotiable, restated as file semantics

1. **Identity, not path.** `target_inode` + `fs_uuid` decide what is acted on.
   `target_path` exists so a person reading the queue knows what they are
   looking at, and so the GUI can show something. If the inode no longer
   exists, or no longer matches, the entry is **dropped, not applied** — the
   roadmap's wording, and the difference between a queue and a booby trap.
2. **Re-authorise at execution.** `queued_by_cap` is checked when the entry
   *runs*, not when it is written. Lane A owns what goes in that field; I have
   left it deliberately vague because I should not be designing the capability
   half. The property to preserve is the one the roadmap names: Windows'
   `PendingFileRenameOperations` is a persistence and privilege-escalation
   vector *because* it runs as SYSTEM against a list someone else wrote.
3. **Never escalate a denial.** `reason` is a closed set of *temporary*
   obstacles. There is deliberately no `permission-denied` value: if the
   operation was refused for permission, it is refused, and no deferral is
   offered. A queue that could hold a denied operation is a queue that turns
   "no" into "not yet".

## What each lane would then own

- **A** — the queue directory, the replay hook (on mount, and on
  device-becomes-idle), and the capability field. The hard half.
- **B** — `rm`/`mv` asking when stdin is a terminal, `--defer` for scripts,
  never defaulting to defer in a pipe, and the list/cancel command.
- **C** — the file manager asking in the failure dialog, a queue view, and the
  report when an entry runs or is dropped. **The dialog half already exists**:
  `explorer` grew a failure dialog on 2026-09-07 (`design-decisions.md` §814)
  specifically because the roadmap says the deferral prompt belongs "in the
  same dialog that reports the failure", and there was none.

## What I am least sure about, and would most like challenged

- **A file per entry versus one append-only journal.** A file per entry makes
  cancellation a delete and needs no rewriting; a journal is one `fsync` and
  one parse. I chose files because cancel-is-delete cannot half-fail.
- **`target_inode` on a filesystem that has no stable inode.** ext4 does; a
  future network or FAT volume may not. Falling back to path matching there
  would quietly reintroduce the thing rule 1 forbids, so I would rather such a
  filesystem **refuse to queue** than queue something weaker. That is lane A's
  call.
- **Whether `reason` should be advisory or load-bearing.** If the replay hook
  trusts it to decide *when* to retry, a wrong value delays an operation
  forever; if it just retries everything on every mount, the field is only for
  the user's benefit. I lean to the latter, which is simpler and cannot stall.

## What I have not done

Not written code, not created the directory, not touched `kernel/**` or
`userspace/**`. If either of you would rather own the format, take it — this
is a starting point to argue with, not a claim.
