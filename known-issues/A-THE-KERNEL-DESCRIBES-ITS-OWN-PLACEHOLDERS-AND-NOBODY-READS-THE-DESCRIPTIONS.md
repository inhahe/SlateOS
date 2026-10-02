## A-THE-KERNEL-DESCRIBES-ITS-OWN-PLACEHOLDERS-AND-NOBODY-READS-THE-DESCRIPTIONS (lane A, 2026-09-15) — **Status: OPEN**, one fixed, population triaged

**The search, which is lane C's and is the useful part.** Grep for code that *describes itself* as a placeholder:

```
in the real os | for now, we | simulated state | placeholder for
in a real system | in a real kernel | for simulation | is simulated | (simulated)
```

It works because **this class is almost always documented**. Whoever wrote it knew, wrote it down, and the note then aged into furniture. `dmevent`'s seeded devices were labelled; so was `fwupdate::apply_update`; so was lane C's `ChildProcess`. **41 hits in `kernel/**`.**

**Triaged by 945's ordering** -- first the claims that a protective action is already under way, then the claims that demand an action, then the ones that merely describe. That ordering matters here: sorting by subsystem stakes would have buried a credential store under a partition manager.

| site | claims | verified |
|---|---|---|
| `credentials.rs:211` | `unlock()` "requires user authentication" | **read; fixed** |
| `diskencrypt.rs:362` | generates a recovery key | comment only |
| `fscache.rs:204` | flushed a device's cache | comment only |
| `memdiag.rs:219` | ran a memory test | comment only |
| `startuprepair.rs:238` | ran all standard checks | comment only |
| `osreset.rs:573,599` | scanned and repaired system files | comment only |
| `partmgr.rs:537` | formatted a partition | comment only |
| `powerwake.rs:232` | sent a Wake-on-LAN packet | comment only |
| `logrotate.rs:275` | rotated the logs | comment only |
| `kmod.rs:1` | loads kernel modules | comment only |

**"comment only" means exactly that**, and is the honest label: those rows are what the *comment* says, not what the code does. Only the first was read. A comment claiming simulation can be as stale as any other comment -- 944's whole subject -- and some of these may since have grown real implementations. Each needs reading before it is believed in either direction.

**The one that was read.** `credentials::unlock()` was documented as requiring user authentication and as verifying a master password "in a real system". It takes no argument, so it never could, and `kshell`'s `cred unlock` asks for nothing either. `retrieve` genuinely refuses while the flag is false, so this is a real access control with a free unlock.

Survivable today only because **nothing under `kernel/src/syscall` reaches this module** -- checked by reading, after a first measurement said four files did. That count was `grep` for the word, which matches `SET_CREDENTIALS`, the capability bit. A name counted as a caller.

So the live defect is documentation, and it is **944 inverted**: a false sentence holding a gate *open* rather than a stale one holding it shut. The danger is in the future tense -- the next person to expose this as a syscall would read "requires user authentication" and believe the boundary already existed. The doc now says it authenticates nothing, that the flag is load-bearing anyway, and that adding the check is a feature rather than a line, because no master secret is stored anywhere to compare against.

**Why the rest are open rather than fixed.** Ten sites, each needing its own reading and its own decision between lane C's three outcomes: covered by an existing disclaimer, real-but-unreachable (record the reason), or not inert but wrong (correct it). Batching that would produce ten guesses rather than ten findings. Working down the table in order.
