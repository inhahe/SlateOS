## 1058. `file` carries file 5.45's database compiled and uses it in place, and libmagic is a library of its own

**Date:** 2026-10-02
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** `file` needs its database of 22,642 rules. Upstream installs
it as an 8.5 MB compiled file (`magic.mgc`) and uses the rules straight from
it, without converting them, which is why it starts in about 2 ms. Ours first
carried the rules' *text* and parsed it at every start -- about 30 ms, fifteen
times slower, which a script that runs `file` once per name pays every time.
Now the build compiles the text exactly as `file -C` does, the program carries
the compiled bytes, and it uses them where they lie, as upstream does: 2.1 ms
a run against upstream's 1.8. The price is size: the program is 9.4 MB, the
database 8.5 MB of it -- about what upstream's install takes too, in separate
files. So that the build could do the compiling, the library half of
the port moved out of `file` into a crate of its own, `userspace/libmagic`,
which is also what lets any other program identify files without running
`file`.

| Option | For | Against |
|---|---|---|
| **A. Compile at build time, carry the `.mgc`, use it in place** (chosen) | Starts as upstream starts: 2.1 ms a run against upstream's 1.8 (an ELF file; best of 5 x 100 runs). The bytes are the ones `file -C` writes, which `scripts/file-diff.sh` holds equal to upstream's compiler, so the rules the program runs are exactly the rules upstream runs from `magic.mgc`. The database is part of the program's read-only data: paged in as it is touched and shared by every `file` running at once, as upstream's mapped file is. A rule that does not compile fails the build. | A 9.4 MB program. Using the bytes in place needs a rule (`Magic`) laid out exactly as the compiled record, which `magic.rs` asserts field by field at compile time, and one `unsafe` reinterpretation, which checks the header, the counts and the alignment first and otherwise decodes. A build script, and a crate split so it can call the library, which a build then compiles twice (for the build script, for the program). |
| D. The same database with its runs of zeros packed (8.5 MB to 0.8), decoded at start | A 1.7 MB program. This entry's first choice, in commit 9ef81fa88 (`pack_mgc`, `unpack_mgc`). | 10.7 ms a run: decoding 22,642 records into structures is most of it. |
| E. Packed, and unpacked at start into memory that is then used in place | A 1.7 MB program; not measured, but writing 8.5 MB of fresh memory at every start is about 2-3 ms more than A. | Each run's own 8.5 MB, where A's is shared. |
| B. Carry the text, parse at start | One crate; nothing generated. | ~30 ms a run. |
| C. Install `magic.mgc` in the image | Exactly upstream's layout. | A file that can go missing, or go stale against the program. Still supported: a database installed at the default path is read in preference, as upstream reads it. |

**Where it bites.** `userspace/file/build.rs` (the compile);
`userspace/file/src/main.rs`, whose `database` carries the bytes aligned for a
rule; `libmagic::magic::Magic` (`#[repr(C)]`, the layout assertions);
`libmagic::apprentice::{compile_mgc, map_builtin, in_place}`; and
`apprentice_map`, which uses the carried database when nothing is installed at
`/usr/share/misc/magic` (`Ms::builtin`). Rules are held as
`Cow<'static, [Magic]>` (`MagicMap`, `MList`): borrowed when in place, owned
when read from text or a file. A big-endian machine would decode rather than
borrow; SlateOS has none. One visible consequence, and it is upstream's:
`file -C` and `file -c` with no `-m` look for the *text* at the default path,
as they do on an ordinary install, and say "could not find any valid magic
files!"; `file -l` lists the carried database.

**Revisit when** the image's size matters more than `file`'s start: D's code
is in the history, and E is a small change from it. `file` is not yet in the
image (`scripts/rootfs-bin-manifest.txt`), so today the size costs nothing
there.
