## §96 — `/etc/services` stays the IANA database; SlateOS's own service config moves to `/etc/startup.conf` and `/etc/service.d/`

**Date:** 2026-07-31
**Decided by:** Claude (autonomous)

**The problem.** Three subsystems had independently claimed the path
`/etc/services` (BUG-ETC-SERVICES-THREE-WAY-COLLISION in known-issues.md):
`getent`/`posix` as the classic IANA port/protocol *file*, `init`/`kernel` as a
*file* holding our line-per-ELF startup list, and the `service` CLI as a
*directory* of YAML unit definitions. Two of the three had to move. Which two
is a filesystem-layout choice, and layout is user-visible and awkward to change
later, so it is worth recording rather than just patching.

**What was decided.**

* `/etc/services` is the IANA port/protocol database, and nothing else.
* Init's startup list becomes `/etc/startup.conf`.
* The service manager's unit definitions become `/etc/service.d/<name>.service`,
  with enable-symlinks in `/etc/service.d/enabled/<name>`. Runtime state stays
  where it already was, `/run/services/<name>/`.

**Why this direction.** `/etc/services` is not a name SlateOS gets to pick. It
is specified — POSIX's `_PATH_SERVICES`, `getservbyname`/`getservbyport`, and
`getent services` all resolve it, and `posix/src/paths.rs` already hard-codes
it because the C API contract says so. Any program ported to SlateOS that calls
`getservbyname` will read that path whether or not we agree. So the standard
consumer is the one that *cannot* move, and the two SlateOS-specific ones are
the ones that can. Choosing the other way would mean either shipping a
non-conforming libc or teaching the porting layer to redirect a documented path,
both of which cost far more than a rename.

**The alternative considered:** put the IANA database somewhere else (say
`/etc/inet/services`) and keep `/etc/services` for the service manager, on the
grounds that a *service manager* is what a desktop user is more likely to mean
by "services", and the port database is a legacy table almost nobody edits.
Rejected: the port database's whole value is that ported software finds it
without being told, and "almost nobody edits it" is an argument about *editing*,
not about *reading* — it is read on every `getservbyname`. A path we invent for
it would simply be wrong for every ported program.

**Why two new names rather than one.** Init's list and the `service` CLI's units
describe overlapping things, and it is tempting to merge them into one directory.
They are not merged here, because they are genuinely different artifacts today:
init is `no_std`, parses a flat line format into a fixed 2 KiB buffer, and runs
before any YAML parser exists; the CLI reads YAML from a full `std` userspace.
Unifying them is a real refactor of the service manager, not a rename, and doing
it under cover of a bug fix would have hidden it. `design.txt` line 653 in fact
anticipates two mechanisms ("our service manager is one way, and then we'll have
some simple list of apps … to load"), so two paths is not obviously wrong even
long-term.

**What is given up.** Nothing existed on disk to migrate — the kernel writes the
bootstrap file fresh on every boot and no image ships a populated
`/etc/services` — so there is no compatibility shim and no upgrade path to
maintain. If that stops being true (once SlateOS has persistent installs), a
rename like this one will need one.

**How to reverse.** Three string constants and their comments:
`services/init/src/main.rs` + `kernel/src/main.rs` for `/etc/startup.conf`, and
`userspace/service/src/main.rs` for `/etc/service.d`. If the service manager is
later unified, both should collapse into whichever path the unified manager
picks — but not back onto `/etc/services`.
