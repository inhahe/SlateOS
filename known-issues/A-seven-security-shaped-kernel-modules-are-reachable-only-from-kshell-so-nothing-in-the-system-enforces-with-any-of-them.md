### [A] Seven security-shaped kernel modules are reachable only from `kshell`, so nothing in the system enforces with any of them -- 2026-09-21
**Status:** OPEN -- **observation, not a bug report.** No single module is broken; the layer is unconnected. Extends the path-keyed entry above, which turns out to be a symptom of this.

**In short:** the kernel contains a set of modules that decide whether
something is allowed -- seals, security policy, per-path capabilities, file
locks, disk-encryption unlock, authentication. All are implemented and
tested, and several report statistics into `/proc`. Not one of them is
consulted by any code that actually does the thing it would be guarding.
The only way to reach them is to type a command into the kernel shell.

**Measured, one grep per row: callers outside the module's own file.**

| module | entry point | reached from |
|---|---|---|
| `fs/sealing.rs` | `check_seals` | `kshell` only |
| `fs/secpolicy.rs` | `check_access` | `kshell` only |
| `fs/capsettings.rs` | `check_access(uid, path)` | `kshell` only |
| `fs/diskencrypt.rs` | `unlock_volume(id, _passphrase)` | `kshell` only |
| `fs/authbroker.rs` | `authenticate(principal, method)` | `kshell` only |
| `fs/reclock.rs` | record locks | a self-test, and `release_all` on process exit. **Nothing acquires** |
| `fs/vfs.rs` | `flock_resolved` | reachable, but nothing in-tree takes an advisory lock |

**Note the signature in row 4.** `unlock_volume(id: u32, _passphrase: &str)`
-- the parameter is underscore-prefixed, so the passphrase is not merely
simulated, it is structurally unused. The comment says
*"Simulated passphrase check (in real implementation, derive key and
verify)"*. That is honest, and it is one `pub fn` away from a caller who
would reasonably assume it checks.

**Why this is one entry and not seven.** Every one of these modules was
filed -- by me, this week -- as low severity *because nothing relies on it
yet*. Written seven times, that reads as seven small risks. Written once, it
reads as what it is: **a security layer that is built and not wired in**,
whose individual defects (path-keying, simulated checks, counters that can
only read zero) are all held harmless by the same single fact. The first
commit that connects any one of them removes that protection for that
module only, and it will be a commit about wiring, so its diff will not
mention the defect it activates.

**The `/proc` consequence, already recorded for two of these (dd-942).** A
reader who sees `denied: 0` concludes *nothing was denied*. The truth is
*nothing asked*. That reading is available today for `sealing` and
`secpolicy`; the others export statistics of the same shape.

**What I am NOT claiming.** That any of this is a bug. A layer built ahead
of its callers is a legitimate way to build an OS, and the comments are
candid about what is simulated. The claim is narrower and worth making:
**the project does not currently have a place where that staging is
written down**, so each module reads as finished when looked at alone, and
I have now twice re-derived "oh, nothing calls this" from scratch while
assessing severity.

**Suggested next step, for the operator rather than for me:** decide whether
this layer is staged-for-later or believed-to-be-live. Those need different
things -- a tracking list in the first case, wiring work in the second --
and it is not a call I should make by reading greps.
