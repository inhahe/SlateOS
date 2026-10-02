### [B] TD-B-LS-INVENTS-A-POSITION-FOR-THE-DOT-ENTRIES — 2026-08-22 — OPEN (tech debt)
**Status:** OPEN — re-verified 2026-09-24: `RealTree::read_dir` still puts the dots at the front of the stream.

**What it is.** `ls -a` has to list `.` and `..`, and `std::fs::read_dir`
discards them. `RealTree::read_dir` in `userspace/coreutils/src/bin/ls.rs`
puts them back at the front of the stream. That position is a guess, and on
ext4 it is the wrong one.

**How to see it.** Under `-U`, `-f` or `--sort=none` — the three listings whose
order is the directory's own and not a sort — we disagree with GNU. Measured on
WSL's ext4, in a directory of twelve entries built by `touch`:

```text
$ ls -f t          (GNU 9.5, and a raw readdir(3) loop, byte for byte)
y.tar.gz  ..  x  a  dir  f9  z.txt  f2  f10  bb  .hidden  .

$ ls -f t          (ours)
.  ..  y.tar.gz  x  a  dir  f9  z.txt  f2  f10  bb  .hidden
```

ext4's hashed directory index put `.` last in that directory; another
directory puts it somewhere else again. Under every *sorted* listing the dots
sort to the front regardless, so the default `ls -a` is unaffected — this is
visible only with the three order-preserving flags.

**Why it is not simply fixed.** There is nothing to recover the position from.
`std::fs::read_dir` filters the dot entries out inside the iterator, so by the
time we see the stream the information is gone. The fix has to read the
directory below std:

- a raw `getdents64` on Linux and on SlateOS, `cfg`-forked per target, with the
  `unsafe` that implies; or
- a `libc` dependency — **rejected**, and worth writing down why: `libc` has no
  `x86_64-slateos` support, so it would have to be a
  `[target.'cfg(target_os = "linux")'.dependencies]` entry. That fixes the
  measurement harness and leaves the shipping binary exactly as wrong, which is
  strictly worse than the current state: the harness would go green while the
  bug stayed.

**What the proper fix looks like.** Give `Tree::read_dir` a real directory read
that yields the dot entries in their own positions. The natural home is not
`ls` — `find`, `du` and the shell's globber all walk directories — so it wants
to be a small crate (`userspace/dirread`, say) with one `getdents64` per
supported target behind a safe iterator, in the same shape as `pwdb` and
`localtime`. Until then `ls -aU` on SlateOS reports SlateOS's own readdir order
with the dots prepended, which is self-consistent and merely not GNU's.

**Trigger:** do it when a second utility needs the dot entries, or when
`ls -f`'s order starts mattering to something (a test, a script in the image).
