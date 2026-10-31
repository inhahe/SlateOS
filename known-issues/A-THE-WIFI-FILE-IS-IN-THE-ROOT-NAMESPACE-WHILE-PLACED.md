## A-THE-WIFI-FILE-IS-IN-THE-ROOT-NAMESPACE-WHILE-PLACED (lane A, 2026-10-31) — OPEN

**Status:** OPEN

**In short:** while a WiFi driver process joins the network the boot
medium names, the kernel puts the network file, passphrase included, at
`/.boot-wifi` in the mount namespace every process shares. A tree
capability limits the driver to that directory, but a process holding the
general `File` capability (the right to every file) could read the
passphrase for as long as it is placed: up to `WIFI_JOIN_S` (200 s), though
the driver reads it in its first moments. Today no such process runs then,
because the boot's join, in the Linuxdrv self-test, comes before every
long-lived process starts. A join made beside other processes would expose
it.

**Where.** `kernel/src/linuxdrv.rs`: `place_boot_wifi` mounts the sealed
memfs with `Vfs::mount_with_options` in the caller's (root) namespace;
`drop_boot_wifi` detaches it after the join (design-decisions 1596).

**The proper fix.** Place the file in a mount namespace of the joining
driver's own: `fs::mntns::create_from` a copy of the root's table, mount the
sealed memfs into that copy only, spawn the driver suspended
(`spawn::spawn_process_suspended`) and `mntns::set_process` it into the
namespace before it runs. No path from any other namespace then names the
file, whatever capability the asker holds, and the namespace goes, the
memfs with it (wiped as it drops), when the driver ends. What it needs
first: namespace-taking forms of what the placing calls in the caller's
namespace today -- the mount, and `trees::register`'s pin
(`Vfs::resolve_path`, `Vfs::pin_dir`, `Vfs::filesystem_read_only`). The
drop already detaches the filesystem by its identity from every
namespace (`Vfs::detach_filesystem`), and needs nothing new.

Trigger: a WiFi join outside the boot's self-tests -- a driver that serves
WiFi for the system's life and joins at boot or rejoins a lost link
(known-issues A-WIFI-REKEYS-AND-LOST-LINKS-ARE-NOT-ANSWERED), since other
processes run then.
