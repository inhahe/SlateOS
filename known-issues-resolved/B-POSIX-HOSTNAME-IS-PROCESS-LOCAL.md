## B-POSIX-HOSTNAME-IS-PROCESS-LOCAL (lane B, 2026-08-22) — FIXED: read side 2026-09-10, write side the same day (`b542b361b`)

**In short:** `gethostname()` and `sethostname()` — the two C functions any
program uses to ask or set what this machine is called — do not actually talk
to the system. Each program that calls them gets its own private copy of the
answer, starting at `localhost`, and setting it changes only that program's
copy. Two programs running side by side can hold different opinions about the
machine's name, and neither is the real one. This was found while fixing the
`hostname` command (entry above), which is now file-based and no longer
affected; but every *other* caller still is.

**Where it lives:** `posix/src/unistd.rs` — `gethostname()` (line 1091),
`sethostname()` (line 1735), and the `process_global!` block above them (line
989). `process_global!` is defined at `posix/src/perprocess.rs:82` and expands
to `static mut STORAGE` on the target, `thread_local!` on the host. It is a
per-process variable by construction; there is no syscall behind it.

**How to reproduce:** any program that calls `sethostname("x", 1)` and then
`execve`s or exits, followed by any program calling `gethostname()` — the
second sees `localhost`. Also: `uname()`'s `nodename` field is filled from the
same storage (`copy_hostname`, line 1020), so `uname -n` has the same defect
wherever it is served from the C function rather than from the files.

**Why this is not simply "the utility's fault":** the functions are honest
about their *arguments* — they match glibc's truncation rules, return
`ENAMETOOLONG`, check `CAP_SYS_ADMIN` before anything else exactly as Linux's
`sys_sethostname` does, and are well tested. Every observable detail is right
except the one that matters: where the value lives. That is what made the
defect survive — the code around it looks carefully done, because it is.

**What the proper fix looks like.** Two options, and the choice needs care:

1. **Back them with the files** — `gethostname()` reads
   `/proc/sys/kernel/hostname` then `/etc/hostname`; `sethostname()` writes
   both. Correct immediately and consistent with every other consumer, but it
   puts filesystem I/O behind a function that callers reasonably assume is
   cheap and non-blocking, and `posix` is `no_std` on the target so the read
   has to go through its own VFS path rather than `std::fs`.
2. **Back them with a kernel call** — the kernel already owns a host name at
   `/sys/kernel/hostname` (`kernel/src/fs/sysfs.rs:76`, written by `kshell`).
   A `uname`-style syscall would be the Linux-faithful shape and would keep the
   functions cheap. This needs a kernel-side addition, which is **lane A**, so
   it would go through `requests/`.

Option 2 is the right end state and option 1 is a correct interim. Not decided
yet — deliberately, because it is worth doing once. Until then, **anything in
the tree that needs the real host name should read the two files**, which is
what `dhcpcd`, `getty`, `osh`, `sysctl`, `logger`, `hostnamectl`, `snapper2`,
`sudo` and now `hostname` all do.

**If never fixed:** the C functions stay quietly wrong. Nothing in the tree
depends on them today (the file-based route is universal), so nothing is
currently broken by it — but they are the obvious thing for a ported C program
to call, and it would get `localhost` with no indication anything was amiss.

---


### Fixed on the read side — 2026-09-10

`gethostname`, `getdomainname` and `uname`'s `nodename` now read
`/proc/sys/kernel/hostname`, then `/etc/hostname`, then the stored buffer.
That is the same pair in the same order the rest of the tree already uses:
`osh` fills `$HOSTNAME` from exactly it, `dhcpcd` writes both when a lease
supplies a name, and `sysctl` maps `kernel.hostname` onto the first. Matching
the existing order was the point -- a libc that agreed with the kernel but not
with the shell would have replaced one disagreement with another.

**`sethostname` returned `ENOSYS` from that change; `setdomainname` did not,
for one commit longer.** The commit message said both, and only one was true --
`setdomainname` kept writing its process-local buffer and returning `0`, and
`getdomainname` kept reading it back, so the domain-name pair retained the
entire original defect while the record said it was fixed. Corrected the same
day, together with the `sethostname` doc comment, which still described the
storage behaviour the same commit had removed.

Both return `ENOSYS` now instead of `0`. They
wrote a process-local buffer that `gethostname` then read back, so a program
could set the hostname, read it, get its own value and conclude it had worked.
That is worse than `setgroups`' honest refusal in the one way that matters:

| | `setgroups` | `sethostname` (before) |
|---|---|---|
| what it did | nothing | nothing observable |
| what it returned | `-1`, `ENOSYS` | **`0`** |
| what the caller learned | the truth | that it had worked |

The `CAP_SYS_ADMIN` check still runs **first**, deliberately: an unprivileged
caller should learn it is unprivileged, which is permanent, rather than that
the call is unimplemented, which is not.

### Why the write side is still open

**There is no native syscall number for the hostname at all.** The kernel holds
it in `fs::nameservice`, reachable only from the Linux-ABI table. Established
by lane A's audit of handlers reachable from that table and from no native
number (notice of 2026-09-10T07:09:56Z) -- `fs::nameservice` was the single
module that survived their triage of fourteen -- and confirmed by grepping
`kernel/src/syscall/number.rs`, whose only `DOMAIN` matches are the unrelated
`SYS_DMA_DOMAIN_*`.

Requested as `requests/b-a-no-native-syscall-reports-the-hostname.md`. When the
number exists, `sethostname` becomes a syscall and the `ENOSYS` goes away.

### Four tests were asserting the bug

`test_sethostname_roundtrip` set the hostname, read it back, and asserted they
agreed. They did agree -- both ends were the same buffer -- so the round trip
was the *evidence that it worked*, and it was the defect. Likewise
`uname_nodename_tracks_sethostname`, and two `phase167` tests asserting the
write landed. All four now assert the refusal.

`sethostname` no longer being a way to change the hostname does take away the
only means of *varying* it, which `gethostid` needs -- it hashes the name.
That is what `set_stored_hostname_for_test` is for: a test seam is the honest
place for it, and a public function that half-works is not.

### The write side, closed the same day — noted 2026-10-01

The section above was overtaken within hours and never updated, which this
note repairs. Lane A added the native pair -- `SYS_HOSTNAME_SET` (1072) and
`SYS_DOMAINNAME_SET` (1073), `04ef99f35` -- and `b542b361b` wired `sethostname`
and `setdomainname` to them, so both now change the system's names, under
`Rights::SET_HOSTNAME`, refusing a name over 64 bytes with `EINVAL`.
`gethostname` and `getdomainname` read `/proc/sys/kernel/hostname` and
`.../domainname` and nothing else. `services/ctest-hostname` holds the
round trip on the real kernel. The C functions are now what a ported program
should call, and `hostname` does (2026-10-01): its file-based reading, which
this entry recommended for as long as the functions were wrong, is gone.
