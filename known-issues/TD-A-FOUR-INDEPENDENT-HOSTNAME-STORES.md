### TD-A-FOUR-INDEPENDENT-HOSTNAME-STORES

**Date:** 2026-08-23 · **Lane:** A · **Where:** `kernel/src/fs/sysfs.rs:77`,
`kernel/src/fs/netsettings.rs:662`, `kernel/src/fs/fileshare.rs:254`,
`kernel/src/fs/nameservice.rs:160`

**In short:** the kernel stores "the name of this machine" in four separate
places that do not talk to each other. Change it in one and the other three
keep the old value, so different parts of the system will disagree about what
the computer is called.

Found while fixing `FIXED-A-VMGUEST-REPORTED-AN-EMPTY-HOSTNAME-TO-EVERY-HYPERVISOR`
above, which was a *fifth* copy — that one is now resolved by reading from
`fs::sysfs`, but the other four remain:

| Module | Storage | Reached via |
|---|---|---|
| `fs::sysfs` | `static HOSTNAME: Mutex<String>` | `/sys/kernel/hostname`, `get_hostname()` |
| `fs::netsettings` | its own state | `set_hostname()` / `hostname()` |
| `fs::fileshare` | its own state | `set_hostname()` / `hostname()` |
| `fs::nameservice` | its own state | `set_hostname()` |

(`ipc::namespace` and `container` also carry hostnames, but those are
correct: a UTS namespace is *supposed* to hold a per-process override. They
are not part of this issue.)

**Why it matters concretely.** A user renames the machine in Settings. Which
of the four gets written decides whether the change shows up in `/sys`, in
the SMB browse list, in DNS registration, in what the hypervisor is told, or
in some subset of those. Every combination is a bug and none of them is
reported.

**Proper fix.** `fs::sysfs::HOSTNAME` is the right home — it is the one with
a filesystem path, a validator (non-empty, ≤ 253 bytes per the DNS limit) and
a defined fallback. Make `fs::sysfs::set_hostname` public, delete the other
three stores, and have those modules read through on demand as `vmguest` now
does. Each has a `set_hostname` that must become either a write-through or a
removal, decided per module: `nameservice`'s looks like a write-through,
`fileshare`'s and `netsettings`'s look like they should just go.

**Not done now** because it crosses four modules that other work is touching
and is not on the path of any current boot failure. It is pure consolidation
with no behavioural question in it, so it can be done whenever the boot suite
is green.
