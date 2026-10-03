### [A] Eight security-named `fs/` modules claim to enforce something and nothing calls them; `sealing` is the worst of them -- 2026-09-17

**Status:** OPEN

**In short:** the kernel has a feature that lets a program mark a file
permanently unchangeable. You can set the mark, `/proc` will list the file as
marked, and **nothing stops anyone writing to it.** The same shape covers
seven other security-named modules. None of it is exploitable today because
nothing uses any of it -- the danger is that the next thing to use it will
believe the guarantee.

A bounded subset of the 340-module pattern above, taken because a false
security claim is materially worse than a display that does not dim, not
because the pattern is different.

| module | crypto/hw refs | callers outside `/proc`, `kshell`, own self-test |
|---|---|---|
| `faceunlock` | 0 | 0 |
| `authbroker` | 0 | 0 |
| `secpolicy` | 0 | 0 |
| `secmod` | 0 | 0 |
| `diskencrypt` | 0 | 0 |
| `filevault` | 0 | 0 |
| `immutable` | 0 | 0 |
| `sealing` | 0 | 0 |
| `integrity` | 5 | 2 |

`integrity` is the control: same naming, same directory, and it does use
crypto and has real callers. So the eight are not an artefact of how the
question was asked.

**The claims are not uniform, and the distinction matters.** Some are honest
already:

| module says | reading |
|---|---|
| `diskencrypt`: "Disk encryption **management** ... Manages encryption *status*, key slots ... **Provides the settings panel interface**" | honest. It manages status and says so. |
| `authbroker`: "**Implements** a Plan 9 Factotum-inspired authentication broker. Programs never touch passwords or keys directly" | false. Nothing brokers anything. |
| `filevault`: "**Provides** per-folder encryption with password-based key derivation" | false. Zero crypto references in the file. |
| `sealing`: "provides a mechanism to place **irrevocable restrictions** on file operations" | false, and the worst of them. |

**Why `sealing` is the worst.** Enumerated rather than asserted: every
`sealing::` reference outside its own file is `procfs.rs` (`stats`,
`list_sealed`) or `kshell.rs` (`add_seals`, `get_seals`). The write and
truncate paths -- `fs/vfs.rs`, `fs/handle.rs` -- contain no reference to
seals at all. So a seal can be set, `/proc/sealing` will list the file as
sealed, and a write succeeds. An unenforced *irrevocable* restriction is
worse than no restriction, because the word invites reliance.

**And its `/proc` output makes the gap unreadable.** `sealing::stats()`
reports a `denied` count that reads 0 on any ordinary boot, so "0 denied"
looks like *nobody has tried* rather than *nothing in the write path asks*
-- dd-942 in the one place a reader would look to find out.

*(Corrected 2026-09-21: this said the count "can only ever be 0 because
nothing checks a seal to deny anything", and that is false. `DENIED_OPS`
IS incremented, in `check_seals` at lines 216 and 269 -- but `check_seals`
has exactly one caller outside its module, `kshell`, a command a human
types. So the counter is live and reachable, just not from anything that
writes a file. `secpolicy` carried the same overstatement of mine and is
corrected too. Found by trying to MEASURE the class rather than trusting my
own prose: a scan for atomic counters in `fs/` that are loaded but never
incremented returned zero candidates, which contradicted what I had
published twice. The scan was right.)* Same shape as `binfmt`'s `stats()` returning zeros for an uninitialised
table, and as the lock-context corpus reading `clean` over a population of
its own fixtures.

**What I am not doing.** Not implementing enforcement: seal checking belongs
in the VFS write path and is a real feature with a capability story
attached. Not sweeping the docs either -- but the security four
(`authbroker`, `filevault`, `sealing`, `faceunlock`) are a defensible
targeted correction rather than a sweep, because changing `provides` to
`records` makes a doc match its code and is not the shared-convention
rewrite dd-951 warns about. Recorded first so the finding exists
independently of whether the wording gets fixed.

#### Triage complete 2026-09-18: 5 docs corrected, 3 were already honest

All nine modules in the table above have now been read rather than
inferred from their names -- which matters, because inferring purpose from
a name and a signature list is exactly how I called `binfmt` a registry
when its line 1 says *statistics*.

| module | line 1 says | verdict | action |
|---|---|---|---|
| `sealing` | "place **irrevocable restrictions**" | false | doc states the gap |
| `authbroker` | "**Implements** a Plan 9 Factotum-inspired ... broker" | false | doc states the gap |
| `filevault` | "**Provides** per-folder encryption" | false | doc states the gap |
| `faceunlock` | "**Provides** facial recognition ... verification" | false, and `verify()` always succeeds | doc leads with ALWAYS SUCCEEDS |
| `secpolicy` | "mandatory access control policy **engine**" | false | doc states the gap (this update) |
| `immutable` | "**Provides** `chattr`-style file flags" | duplicate of a working mechanism | separate entry; not a missing feature |
| `secmod` | "Security Module **Statistics** ... **monitoring** ... **Tracks**" | **honest** | none |
| `diskencrypt` | "encryption **management** ... encryption **status** ... **settings panel interface**" | **honest** | none |
| `integrity` | (the control) | **honest**, and really enforces | none |

**`secpolicy` was the one addition.** Its `check_access` has exactly one
caller outside the module -- `kshell.rs:101079`, a command a human types --
so `set_mode(Enforcing)` changes what `/proc` reports and nothing else. The
word *mandatory* is the dangerous part: it is the claim that stops the next
person adding their own check. Its architecture block also advertises
`secpolicy::check(...)`, which does not exist under that name.

**Three were left alone because they are accurate**, and that is the half of
this triage worth keeping. `secmod` and `diskencrypt` use *statistics*,
*monitoring*, *tracks*, *management*, *status*, *settings panel interface* --
all true of stores that store. Editing them would have replaced correct text
with a warning about a problem they do not have, and a sweep that cannot
tell those two from the other five would have done exactly that. The
distinction is not detectable from the directory, the naming, or the caller
count: `secmod` and `authbroker` have identical caller profiles (0 outside
`/proc`, `kshell` and self-test) and opposite verdicts. **Only line 1
separates them**, which is the rule that came out of the `binfmt` error and
is the reason this pass read nine module docs instead of grepping for nine
caller counts.
