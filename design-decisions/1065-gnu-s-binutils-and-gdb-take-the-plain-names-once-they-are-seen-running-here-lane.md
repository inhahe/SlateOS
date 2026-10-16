## 1065. GNU's binutils and GDB take the plain names once they are seen running here; lane B's `ar`, `objdump`, `readelf` and `gdb` retire, and `strings` stays lane B's

**Date:** 2026-10-08
**Lane:** B
**Decided by:** Claude (autonomous). Lane D put the question to lane B as
lane B's naming decision
(`requests/d-b-gnu-binutils-is-on-the-image-beside-lane-bs-tools.md`,
`requests/d-b-gnu-gdb-is-on-the-image-and-could-take-bin-gdb.md`), and
recommended this answer for every name but `strings`, where it called keeping
lane B's "reasonable".

**In short:** lane D has ported the real GNU programs that build and inspect
other programs -- the archiver `ar` (with `ranlib` and `strip`), `objdump`,
`readelf`, and the debugger GDB -- and put them on the image as `gnu-ar`,
`gnu-objdump` and so on, because lane B had already written programs of its
own under the plain names. Lane B's are rewrites that imitate GNU's; GNU's are
the programs themselves. So once a boot test has shown GNU's working on
SlateOS, GNU's take the plain names and lane B's are deleted. `strings` is the
exception: lane B's is a small text tool, already held to GNU's behaviour by a
test, and stays.

### What it is

| name | lane B's program | answer | why |
|---|---|---|---|
| `ar`, `ranlib`, `strip` | `userspace/ar`, one multi-call binary | GNU's; `userspace/ar` retires | Build tools. An autoconf build runs them by these names, straight after one another, and expects GNU's options and archive format -- the deterministic mode lane D configured included. A rewrite can only approach that. |
| `objdump`, and `nm` and `size`, which lane B's answers to | `userspace/objdump` | GNU's; the crate retires | GNU's decodes every section, relocations and DWARF included. Lane B's `nm` and `size` were never installed, and GNU's already hold those names. |
| `readelf` | `userspace/readelf` | GNU's; the crate retires | As `objdump`. |
| `gdb`, and `gdbserver`, which lane B's answers to | `userspace/gdb` | GNU's; the crate retires | The operator asked for a *port* of a capable debugger (§1050). Lane B's was written before one existed. Neither can run a program under its control until lane A's ptrace lands (`requests/d-a-a-debugger-needs-ptrace-for-native-programs.md`); when it does, GDB's Linux back end uses it unchanged. |
| `strings` | `coreutils`'s `strings` | lane B's; GNU's stays `gnu-strings` | A plain text tool, and `scripts/strings-diff.sh` holds it to GNU's output. It is part of the multi-call `coreutils` binary, not a program of its own, and moving the name would change nothing a user sees. |

**The condition.** Nothing moves until `services/ctest-binutils-runs/` passes in
a boot of `main` -- and, for `gdb`, `services/ctest-gdb-runs/`. Both wait on
lane A's generic C fixture rung (`requests/d-a-one-rung-for-every-c-fixture.md`).
Retiring a program that works for one never yet seen running here would trade
a known tool for an unknown one.

**One change, not two.** The image refuses two programs at one path, and
`scripts/rootfs-bin-manifest.txt` requires `ar`, `ranlib` and `strip`. So
neither lane can move first on its own: GNU's at `/bin/ar` beside lane B's
collides, and lane B's deleted first leaves the manifest naming a program
nothing builds. The move is one commit. Lane B pre-approves lane D making it
whole -- deleting `userspace/ar`, `userspace/objdump`, `userspace/readelf` and
`userspace/gdb`, their workspace members and every mention of them, beside
lane D's own recipe, manifest and `programs.md` lines. If lane B gets there
first, lane B makes the same commit with lane D's lines and says so in the
requests.

### Alternatives

- **Rename lane B's** (`slate-objdump` and so on). Two implementations of each
  tool, one of them an imitation nobody would choose over the original; every
  fix to lane B's would be work on a program with no users.
- **Keep every plain name lane B's.** Build scripts and people typing `ar`,
  `objdump` or `gdb` would get the imitation while the real program sat under
  a name nobody types.
- **Keep `objdump` and `readelf` for memory safety.** Both parse files that
  may be hostile, GNU's in C and lane B's in Rust -- the one real argument for
  lane B's, and the reason this is written down. Against it: these are
  inspection tools a person runs on purpose, the same ones every Linux
  distribution ships and OSS-Fuzz fuzzes continuously, and lane B's decode a
  fraction of what GNU's do. If a parser bug in GNU's ever matters here, the
  answer is to fix it upstream's way, not to keep a second program.
- **Retire `strings` too.** Defensible, and lane D's recommendation allowed
  either. Nothing would be gained: lane B's already prints what GNU's prints.

### Where

`userspace/ar`, `userspace/objdump`, `userspace/readelf`, `userspace/gdb`
(lane B); `scripts/create-ext4-rootfs.sh`, `scripts/rootfs-bin-manifest.txt`
(`ar`, `ranlib = ar`, `strip = ar`) and `programs.md` (lane D's side of the
move); `roadmap.md`, lane B's backlog, carries the task until it is done.

### How to reverse

The crates stay in history. Restoring one is a revert of the commit that
retired it, plus moving GNU's program back to its `gnu-` name.
