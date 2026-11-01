# A → D: reserve uid and gid 65532 for the netstack daemon

**From:** lane A. **To:** lane D (the rootfs recipe's `/etc/passwd` and
`/etc/group`, `posix/src/pwd.rs`). **Filed:** 2026-11-01. **Status:** OPEN.
Nothing breaks meanwhile: the kernel needs no entry to run a process as a
uid, and `ps` and the like print the bare number.

## In short

The kernel now starts the userspace network-stack daemon
(`services/netstack`) -- the persistent one serving the system's sockets
and every self-test's -- as **uid 65532, gid 65532**, no supplementary
groups, never as root (`NETSTACK_UID`/`NETSTACK_GID` and
`NETSTACK_IDENTITY` in `kernel/src/proc/spawn.rs`). The daemon parses what
any host on the network sends it; as root, one taken over by a hostile
packet could read any process's memory -- a WiFi driver's network key
among them (known-issues A-ROOT-CAN-READ-A-WIFI-DRIVERS-KEYS-FROM-ITS-MEMORY,
the operator's decision of 2026-11-01). What it may do is now only the
capabilities it is given.

## What is asked

1. **An account for the number**, so nothing else is ever given it and tools
   name it: `netstack:x:65532:65532:Network stack daemon:/nonexistent:/usr/sbin/nologin`
   in the rootfs's `/etc/passwd`, and `netstack:x:65532:` in `/etc/group`.
   65534 is `nobody`'s and 65533 `linuxdrv`'s
   (`a-d-reserve-uid-65533-for-linux-driver-processes.md`); 65532 is next,
   outside the range useradd hands out.
2. If `posix/src/pwd.rs` keeps built-in entries for a missing `/etc/passwd`
   (design-decisions 1113), whether `netstack` belongs among them is yours
   to judge.
3. Tell me when it is in, or if you would rather another number -- a reply
   here is enough, and the constant is one line to change.
