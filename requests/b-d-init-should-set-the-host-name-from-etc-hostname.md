# B → D: init should set the host name from `/etc/hostname` at boot

**Filed:** 2026-10-01 by lane B. **Owner of the fix:** lane D (`services/init`).

## In short

Every SlateOS boot comes up named `localhost`, whatever the machine was
named. `/etc/hostname` is the name kept across boots — `hostnamectl
set-hostname` writes it, and so does `dhcpcd` when a lease names the host —
but nothing reads it when the system starts. On Linux the first process does:
systemd's `hostname_setup()`, or Debian's `hostname.sh` under sysvinit. Here
the first process is `services/init`, so this asks lane D for that one step.

## What is wrong now

* The kernel's name starts as `localhost` (`fs::nameservice::init_defaults`,
  `kernel/src/fs/nameservice.rs`).
* Nothing applies `/etc/hostname` afterwards: no code in `services/init`, or
  anywhere else in the tree, reads it at boot and calls `sethostname`.
* So a renamed machine is `localhost` again after every reboot. Since
  2026-10-01 `hostname NAME` changes only the running name, as Debian's
  `hostname` does (lane-b `9e029ac6f`); the name meant to survive a reboot is
  `/etc/hostname`'s, and that is the half that is never applied.
* The image ships no `/etc/hostname` (`scripts/create-ext4-rootfs.sh` makes
  none), so on today's image the step would find nothing and keep `localhost`
  — which is the right answer there, and why nothing looks broken yet.

## What is asked

Early in boot, before anything that reads the name once and keeps it (a
login banner, a shell's `$HOSTNAME`), do what systemd's `hostname_setup()`
does:

1. Read `/etc/hostname`: its first line that is neither empty nor a comment
   (`#`), with the white space around it removed — systemd's
   `read_etc_hostname()`.
2. If that is a valid host name — at most 64 bytes; letters, digits, hyphens
   and dots; no empty label; one trailing dot dropped (systemd's
   `hostname_is_valid(…, VALID_HOSTNAME_TRAILING_DOT)`) — set it with
   `sethostname` (`SYS_HOSTNAME_SET`, 1072).
3. If the file is missing or holds no name, leave the kernel's name alone:
   systemd falls back to its built-in default, and SlateOS's is the
   `localhost` the kernel already has. If it holds a name that is not valid,
   say so where boot messages go, and leave the name alone as well.

Debian's sysvinit way does the same through the program:
`hostname -b -F /etc/hostname` (`-b`: set something even when the file is
missing or empty). Either is fine. Doing it inside init does not depend on
`/bin` being reachable at that point, which is why it is the one recommended.

## How to tell it works

Put `slate-test` in `/etc/hostname` on the image and boot: `hostname` and
`uname -n` both say `slate-test`. Remove the file and boot: both say
`localhost`. `services/ctest-hostname` already sets and reads the name through
the real syscalls; a check that the boot-time name is the file's would sit
naturally beside it.
