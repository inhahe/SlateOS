## B-USERADD-REWROTE-ETC-GROUP-FROM-A-TABLE-IT-HAD-FAILED-TO-READ (lane B, 2026-09-12) -- FIXED

**In short:** `groupadd`, `useradd` and their four siblings read `/etc/group`
and `/etc/gshadow`, change the list in memory, and write the whole file back.
If the read failed, the list was **empty** -- and the write-back then replaced
the file with just the one group that had been added. Every other group on the
system disappeared from the live file.

**Reaching it needed no privilege and no corruption.** A group name may hold
any byte but `/` and NUL on this filesystem, and the read was
`fs::read_to_string`, which refuses the **whole file** for one byte that is not
valid UTF-8. So a single group with a byte over 0x7F in its name was enough.
A momentary permission or I/O error did it too.

**Where.** `userspace/useradd/src/main.rs` -> `Database::load_file`, which was

```rust
let content = match fs::read_to_string(path) {
    Ok(c) => c,
    Err(_) => return Vec::new(),
};
```

and `Database::save`, which rebuilds both files from those vectors.

**The damage was bounded, not prevented.** `atomic_write` copies the old file
to `/etc/group-` before replacing it, which is shadow-utils' convention, so the
content survived where an operator who knew to look could find it. Nothing said
to look, and nothing failed.

**Fixed** by `optionalfile::read_or_empty`, which is written for exactly this
caller and says so in its own docs: it refuses invalid UTF-8 *"because a caller
that rewrites what it read would otherwise replace those bytes"*. `load_file`
returns `Result`; `load_in` and `load` propagate; the six command handlers
(`useradd`, `userdel`, `usermod`, `groupadd`, `groupdel`, `groupmod`) print and
exit 1. An **absent** file is still an empty table -- a system with no
`/etc/group` has no named groups, which is a fact rather than a failure, and
that is the one case where empty is the answer.

Two tests, both asserting their own premise so neither can pass vacuously: a
group file with a `0xFF` in the middle line is refused **and is still on disk
byte-identical afterwards**, and an absent file still loads as empty.

**How it was found, which is the part worth keeping.** Not by grepping for the
defect. The roadmap's lane-B backlog mentions in passing that
`/etc/group`/`/etc/gshadow` "remain a separate gap (eleven readers, one writer,
and no gid allocator in `userdb`)". I had spent the day on the *readers* and
went looking for the **writer** named in that clause. `optionalfile`'s module
docs already listed three programs destroyed by this exact pattern -- visudo
over `/etc/sudoers`, `xdg` over `mimeapps.list`, `hostnamectl` over
`/etc/machine-info` -- and named the shape precisely. This is a fourth, in a
program that was not on that list because nobody had looked at it since the
crate was written.

**Still open, deliberately:** the group files are read as *text*, so a group
whose name is not UTF-8 is now refused rather than mangled -- correct, but it
means such a group cannot be administered at all. Making `GroupEntry` carry
bytes end to end (including `serialize`) is the follow-on, and is a bigger
change than stopping the destruction.
