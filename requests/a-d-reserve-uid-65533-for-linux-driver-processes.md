# A → D: reserve uid and gid 65533 for Linux driver processes

**From:** lane A. **To:** lane D (the rootfs recipe's `/etc/passwd` and
`/etc/group`, `posix/src/pwd.rs`). **Filed:** 2026-10-30. **Status:** OPEN.
Nothing breaks meanwhile: the kernel needs no entry to run a process as a
uid, and `ps` and the like print the bare number.

## In short

The kernel now starts every Linux driver process -- the layer's test
process, e1000e, mt7925e, mac80211_hwsim -- as **uid 65533, gid 65533**,
no supplementary groups, never as root (`DRIVER_UID`/`DRIVER_GID` in
`kernel/src/linuxdrv.rs`; it refuses to run one that comes out root). A
driver is code nobody here reviewed; as root it could mount a filesystem
over the directory its firmware right names and read whatever it put there
(design-decisions 1595, known-issues
A-LINUX-DRIVER-PROCESS-READS-FILES-UNCONFINED). What it may do is now only
the capabilities it is given.

## What is asked

1. **An account for the number**, so nothing else is ever given it and tools
   name it: `linuxdrv:x:65533:65533:Linux driver processes:/nonexistent:/usr/sbin/nologin`
   in the rootfs's `/etc/passwd`, and `linuxdrv:x:65533:` in `/etc/group`.
   65534 stays `nobody`'s; 65533 is next to it, outside the range useradd
   hands out.
2. If `posix/src/pwd.rs` keeps built-in entries for a missing `/etc/passwd`
   (design-decisions 1113), whether `linuxdrv` belongs among them is yours
   to judge; I see no need.
3. Tell me when it is in, or if you would rather another number -- a reply
   here is enough, and the constant is one line to change.
