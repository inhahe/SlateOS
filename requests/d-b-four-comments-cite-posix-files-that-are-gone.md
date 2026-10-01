# D → B: four comments of yours point at posix files that are gone

**Status:** DONE 2026-10-01 (lane B) -- `fsattr.rs` cites
`kernel/src/fs/acl.rs` alone, `interp.rs` the libc's `shebang.rs` and
`spawn.rs`, and `utmpfile` the compile-time assert in `utmpx.rs` (all three
of its sites). `wipefs`'s parenthesis had already gone with its
util-linux port (ac9375cbd, 2026-09-27). · **Filed:** 2026-09-27 by lane D · **Priority:** low --
comments only; no code reads any of these files.

## In short

On 2026-09-26 and 27 lane D deleted the C library's header-transcription
modules that nothing in the library used -- 2,119 of them
(design-decisions.md §1118). Four comments in lane B's tree cite one as a
source, and the citation now leads nowhere. Each has a better one:

| Where | Cites | What to cite instead |
|---|---|---|
| `userspace/coreutils/src/fsattr.rs:812` | `posix/src/linux_acl.rs` (with `kernel/src/fs/acl.rs`) | `kernel/src/fs/acl.rs` alone -- the posix file held the xattr's constants and nothing read them |
| `userspace/oils/src/interp.rs:1298` | "`posix`'s `linux_binfmt` knows `SCRIPT_MAG`" | the C library's own `#!` handling: `posix/src/shebang.rs`, which `execve` (`posix/src/spawn.rs`) runs -- that is what makes a `#!` line the OS's business on SlateOS; `linux_binfmt.rs` only held the constant |
| `utmpfile/src/lib.rs:14, 61, 483` | `posix/src/linux_utmp_types.rs`'s `UTMPX_RECORD_SIZE = 384` | `posix/src/utmpx.rs`: `const _: () = assert!(RECORD == 384);`, glibc's x86-64 `struct utmp`, checked at compile time |
| `userspace/wipefs/src/main.rs:584` | "`posix/src/linux_blkpg.rs` carries the numbers and nothing reads them" | the sentence stands without the parenthesis: the numbers were a transcription, and nothing read them there either |

`utmpfile/` belongs to no lane (roadmap.md, "Owned by no lane"); it is
listed here because every crate that uses it is lane B's (`who`, `last`,
`finger`, coreutils).

Any deleted file is `git show 025311c98^:posix/src/<name>.rs` (the `*_types`
ones) or the parent of the later deletions.
