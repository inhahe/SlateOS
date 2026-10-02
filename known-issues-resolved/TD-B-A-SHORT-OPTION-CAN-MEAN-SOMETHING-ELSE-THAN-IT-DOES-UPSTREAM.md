## TD-B-A-SHORT-OPTION-CAN-MEAN-SOMETHING-ELSE-THAN-IT-DOES-UPSTREAM -- FIXED 2026-09-15

A defect class, not a single bug: a short option bound to the WRONG long
option. The program accepts the flag, understands it as something else, and
answers confidently. No existing gate can see it.

**Found once, by accident.** `blkid -n` was bound to `--no-encoding` here and
is `--match-types` in util-linux, so `blkid -n vfat,ext3 /dev/sda1` set a
no-op flag, consumed `vfat,ext3` as a DEVICE PATH, and reported an ext2
filesystem the caller had asked to exclude. Fixed 2026-09-15; the measurement
and the repair are in that commit.

**Why nothing catches it.** `check-help-vs-parser.py` compares our help text
against our parser, and both were internally consistent -- the help said
`-n, --no-encoding` and the parser agreed. `check-fields-written-never-read.py`
saw only that `no_encoding` was never read, which reads as a missing feature.
Every tool we own is self-consistent about a mapping that is wrong relative to
the program it replaces. The only oracle is the REFERENCE's own flag table.

**Method that works**, and it is cheap:

    wsl -d Ubuntu -- bash -s <<'EOF'
    <tool> --help | grep -E "^ +-[a-zA-Z],"
    EOF
    grep -oE '"-[A-Za-z]" \| "--[a-z-]+"' userspace/<tool>/src/main.rs | sort -u

then compare the two pairings by eye. It needs the real tool, so it cannot be
a pre-push gate on a machine without one -- which is why this is a tracked
sweep rather than a check.

**Checked so far -- 3 tools, 1 defect.** Cleared rows are recorded because a
cleared list is worth more than a shorter one: it says where NOT to look
again.

| tool | pairings compared | result |
|---|---|---|
| `blkid` | `-c -d -n` | **`-n` WRONG** -- fixed 2026-09-15 |
| `unshare` | all 16 (`-m -u -i -n -p -U -C -T -f -r -S -G -R -w -h -V`) | all match |
| `nsenter` | all 10 (`-a -t -F -G -S -V -W -r -w -h`) | all match |

**SWEPT 2026-09-15 by `scripts/compare-short-options.py`: 84 crates compared
against their references, 71 clear, 24 collisions in 13 tools.** 65 crates
have no reference available on this machine and were not compared.

    chpasswd   -s   --sha256        here, --sha-rounds      upstream
    dmesg      -T   --human-time    here, --ctime           upstream
    dmesg      -c   --clear         here, --read-clear      upstream
    dmesg      -f   --follow        here, --facility        upstream
    dmesg      -s   --search        here, --buffer-size     upstream
    eject      -f   --force         here, --floppy          upstream
    findmnt    -d   --fs-devno      here, --direction       upstream
    findmnt    -t   --type          here, --types           upstream
    flock      -E   --conflict-exit here, --conflict-exit-code upstream
    getty      -h   --help          here, --flow-control    upstream
    getty      -o   --long-hostname here, --login-options   upstream
    hardlink   -X   --exclude       here, --respect-xattrs  upstream
    hardlink   -o   --respect-owner here, --ignore-owner    upstream
    hardlink   -p   --respect-perm  here, --ignore-mode     upstream
    hardlink   -t   --respect-time  here, --ignore-time     upstream
    hardlink   -x   --respect-xattr here, --exclude         upstream
    last       -t   --time          here, --until           upstream
    locale     -k   --keyword       here, --keyword-name    upstream
    logrotate  -d   --dry-run       here, --debug           upstream
    mkfs       -V   --version       here, --verbose         upstream
    pstree     -g   --numeric-uid   here, --show-pgids      upstream
    pstree     -h   --help          here, --highlight-all   upstream
    pstree     -t   --threads       here, --thread-names    upstream
    sysctl     -n   --values-only   here, --values          upstream

**Each row is a claim to check, and the first sweep proves why.** It reported
26, and its two most severe --

    mount   -f   --force   here, --fake          upstream
    mount   -l   --lazy    here, --show-labels   upstream

-- were WRONG. `mount -f` upstream is "dry run; skip the mount(2) syscall",
which reads as a safety flag that performs the action instead of simulating
it, and it was the first thing I went to fix. Those arms are in the `umount`
branch: `userspace/mount` is two programs, and umount(8) really does define
`-f, --force` and `-l, --lazy`. Acting on the report wholesale would have
turned two correct bindings into incorrect ones. The checker now compares
against every personality a crate answers to and its self-test carries that
case in both directions.

**Ranked by what being wrong costs**, which is not the order above:

1. `hardlink -x`/`-X` are SWAPPED with each other. Upstream `-x <regex>`
   excludes files and `-X` respects xattrs; here `-X` excludes and `-x`
   respects. So `hardlink -x '\.git' dir` consumes the regex as a positional
   and links files the caller meant to exclude -- on a tool whose whole job
   is to merge files into one inode, and which is not reversible by re-running
   with the flag spelled right.
2. `hardlink -p`/`-o`/`-t` are INVERTED: `--ignore-mode` upstream versus
   `--respect-perm` here, and likewise for owner and time. Ours is the
   conservative direction (fewer links), so a script written upstream gets
   safer behaviour than it asked for rather than more dangerous -- worth
   fixing, not urgent.
3. `dmesg -c` is `--read-clear` upstream (print, THEN clear) and `--clear`
   here. If ours clears without printing, `dmesg -c` discards the buffer the
   caller was trying to read. Needs checking before it is believed.
4. `getty -h` and `pstree -h` print help where upstream they are
   `--flow-control` and `--highlight-all`. Surprising, but visibly so.
5. The rest change output or units and fail loudly enough to notice.

**Unchecked:** the 60 crates with no reference on this machine, and 12
individual bindings whose personality is not installed (a `semanage` arm
inside `userspace/selinux`, an `xdg-open` arm inside `userspace/xdg`). Those
are skipped rather than guessed at, and the report prints the count.
