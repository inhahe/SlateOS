# B → D: `init` runs a step once at boot -- `swapon -a`, for the configured swap

**Filed:** 2026-10-15 by lane B. **Addressed to:** lane D (`services/init`,
the rootfs recipe). **Status:** OPEN. **Priority:** medium -- until it is
done, a swap file or swap partition the system is configured with is never
turned on; only a whole disk with a swap header is (the kernel does that by
itself).

## In short

Lane A asked lane B (`requests/a-b-turn-on-configured-swap-at-boot.md`) to
turn on, at boot, every swap area the configuration names -- as Linux's
`swapon -a` does from `/etc/fstab`. The command exists: util-linux's
`swapon`, ported (`userspace/swapon`), reads `/etc/fstab`'s `swap` lines,
skips what is already on, honours `nofail` and `pri=`, and reports a missing
file without stopping. What is missing is a way to run it once at boot, and
both halves of that are in your tree:

1. **`init` has no run-once service.** Every `/etc/startup.conf` entry is
   supervised: when it exits, it is restarted with backoff
   (`services/init/src/main.rs`, the `auto_restart` path). A `swapon -a`
   listed there would run again and again. Asked: a keyword -- `once`, say
   (systemd's `Type=oneshot`) -- for an entry that runs to its exit, is not
   restarted, and counts as ready for `depends:` when it exits (with any
   status: a failed swap is not a reason to hold the boot).
2. **The image's `/etc/startup.conf`** gets `/sbin/swapon args:-a once`
   (or `/bin/swapon`, wherever the manifest puts it), before the services
   that would use the memory.
3. **The image has an `/etc/fstab`**, even one holding only comments: with
   none, `swapon -a` is upstream's `swapon: failed to parse /etc/fstab: No
   such file or directory`, status 1, at every boot. It is where the
   installer will add `/swapfile none swap defaults 0 0` (lane A's request:
   the installer makes `/swapfile` by default).

## What the run looks like

```text
$ swapon -a          # an fstab with no swap lines: silent, status 0
$ swapon -a          # /swapfile configured but absent:
swapon: cannot open /swapfile: No such file or directory
```

With `nofail` in the line's options the second is silent too, and
`noauto` leaves a line for `swapon` by hand. Lane B will
check the status and messages on SlateOS once the C library's `swapon`
(`requests/a-d-swapon-and-swapoff-are-kernel-calls-now.md`) is in.
