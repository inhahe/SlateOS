### [A] The path-keyed table class is five, not three -- and every one of them is low-severity for the same single reason -- 2026-09-21
**Status:** OPEN (no code change; this corrects the scope and the severity reasoning of the entries above)

**In short:** several kernel tables that decide whether something may be
touched remember the file by *name* instead of by the file itself, so a
second name for the same file slips past them. Three were known. There are
five. More importantly, the reason all five were filed as low severity is
not five separate reasons -- it is one, and it can stop being true in a
single commit that looks unrelated.

**Two more instances, same shape:**

| table | key | what it answers |
|---|---|---|
| `fs/reclock.rs` | `RecordLock { path: String }` | byte-range record locks |
| `fs/capsettings.rs` | `PathRequirement { path: String }` | `check_access(uid, path)` -- may this user reach this path |

`capsettings` is the one to watch: the others are *advisory* locks, which
only bind programs that cooperate, but this one is shaped like a permission
check. A hard link would walk past it.

**The single fact holding all five up.** Each entry says its severity is low
because nothing relies on the table yet. That is the *same* observation five
times, and it is measurable rather than assumed:

| table | only callers outside its own module |
|---|---|
| `capsettings::check_access` | `kshell.rs` -- a command a human types. **No enforcement path.** |
| `reclock` | `main.rs` self-test, and `pcb.rs` `release_all(pid)` on exit. **Nothing acquires.** |
| `sealing::check_seals` | `kshell` (recorded earlier today) |
| `secpolicy::check_access` | `kshell` (recorded earlier today) |
| `vfs::flock_resolved` | reachable, but nothing in-tree takes advisory locks |

So the mitigation is not "these are minor bugs". It is **"no enforcement
path consults any of them"** -- one condition, shared. The first commit that
wires *any* of these into a real check makes that table's defect live, and
that commit will be about wiring, not about keying, so nothing in its diff
will mention the bug it activates.

**The proposed rule does not mechanize, and here is the measurement.** The
flock entry suggests a standing rule instead of three fixes. I tried to
build it. `kernel/src` has **80** path-comparison lookup sites across **25**
files, and most are correct: `devfs`, `cgroupfs`, `index`, `fontmgr` are
namespaces, where the name *is* the thing being identified. Filtering to
files that also mention a permission error narrows 25 to 8, which is better
but still mostly noise. The distinguishing feature is *what question the
table answers*, and no regex sees that.

Recorded so the next person does not rediscover it: this class wants five
individual fixes keyed on `FileId`, plus a note on each table, not a gate.
Compare the ring-3 register gate written the same day, which *was* worth
building -- there the good sites scored 6 of 6 and the bad one 0 of 6, with
nothing in between. A rule is worth mechanizing when the population
separates cleanly, and this one does not.
