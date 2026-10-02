## B-DOZENS-OF-COMMANDS-EXIST-IN-SOURCE-AND-CAN-NEVER-BE-RUN (lane B, 2026-08-22)

**In short:** We have written somewhere around **50 to 70 command-line tools**
— `e2fsck`, `mke2fs`, `strip`, `ranlib`, `xxd`, `killall`, `shred`, `visudo`,
`iostat`, `lpr` and many more — that **no build produces an executable for**.
They are real, complete implementations, several thousand lines in places. They
are unreachable because of how they were written: each one lives inside
*another* program, which is supposed to change behaviour when it is run under
that name, and **nothing ever runs it under that name**. Type `e2fsck` on the
finished system and you get "command not found", even though we have an
`e2fsck`. This is a lot of finished work that currently ships as dead code.

**The mechanism.** A "multi-call" program looks at the name it was started as
and behaves accordingly — one file on disk, many command names pointing at it
by symbolic link. BusyBox on Linux is the famous example. That works only if
somebody *creates the links*. We wrote the dispatch and never wrote the links:

- there is no `userspace/<alias>` crate for these names, so cargo never builds
  an executable called `e2fsck`;
- `userspace/coreutils` has no bin of that name either;
- `scripts/create-ext4-rootfs.sh` creates no such alias. It *can* — it copies
  `dash` to `/bin/sh` at line 669 and does the same again at line 1168, so the
  machinery exists and is simply never pointed at any of these.

So the personality is selected by a branch that no invocation can reach.

**Verified by reading the source, not just by pattern:**

| Program | Names it answers to that nothing produces |
|---|---|
| `userspace/e2fsprogs` | `mke2fs`, `mkfs.ext2/3/4`, `e2fsck`, `fsck.ext2/3/4`, `dumpe2fs`, `debugfs`, `e2image`, `e2label`, `filefrag`, `resize2fs`, `tune2fs` |
| `userspace/systemctl` | `systemd-analyze`, `systemd-cat`, `systemd-cgls`, `systemd-cgtop`, `systemd-escape`, `systemd-notify`, `systemd-path`, `systemd-tmpfiles` |
| `userspace/sudo` | `visudo`, `sudoedit`, `sudoreplay` |
| `userspace/perf` | `perf-record`, `perf-report`, `perf-stat`, `perf-top` |
| `userspace/lp` | `lpr`, `lpq`, `lprm`, `lpstat`, `cancel` |
| `userspace/sysstat` | `mpstat`, `pidstat`, `cifsiostat`, `tapestat` |
| `userspace/ar` | `ranlib`, `strip` |
| `userspace/pv` | `shred`, `truncate` |
| `userspace/last` | `lastb`, `lastlog` |
| `userspace/locale` | `getconf`, `localedef` |
| `userspace/mesg` | `talk`, `write` |
| `userspace/w` | `finger`, `pinky` |
| `userspace/hexdump` | `xxd` |
| `userspace/kill` | `killall` |
| `userspace/cal` | `ncal` |
| `userspace/blkid` | `findfs` |
| `userspace/getty` | `mingetty` |
| `userspace/crond` | `anacron` |
| `userspace/xdg` | `mimeopen`, `xdg-mime` |
| `userspace/ninja` | `samu` |

`e2fsprogs` is the one to look at first: `userspace/e2fsprogs/src/main.rs:98-103`
dispatches `mke2fs` and `e2fsck` (with their `mkfs.ext4`/`fsck.ext4` spellings),
the file is ~3400 lines, and **`design.txt` makes ext4 the filesystem**. The
tools that create and check our only filesystem are among the unreachable ones.

**Why the count is a range.** Detecting "dispatches on its own name" is a
heuristic — a program comparing a string to `"root"` may be checking a username,
not its own name. Requiring a strict `=> Personality::` shape finds 52 names
across 19 crates; a looser one finds 78 across 39. The following hits are
**false positives** and should be discounted from any count: `root` (`sudo`,
`sshd`, `useradm` — checking a *user*), `lo` (`ifconfig`, `dhcpcd`, `hwinfo` —
a network interface), `all`/`list`/`internal`/`error`/`cpu`/`memory`/`min`/
`time`/`localhost`/`anonymous` (argument values). The reproduction below prints
the list so it can be re-checked rather than trusted.

**A second, sharper form of the same defect: the alias that *is* produced.**
Six aliases do have a producer, which means two implementations of one command
exist and the multi-call branch is dead code that can silently disagree with the
live one:

| Program | Alias | Who really provides it |
|---|---|---|
| `userspace/head` | `tail` | `coreutils`'s `tail` — and `tail-diff.sh` tests *that* one |
| `userspace/chown` | `chmod` | `coreutils`'s `chmod` -- **crate retired 2026-09-12** |
| `userspace/who` | `w` | `userspace/w` — see the entry below |
| `userspace/pv` | `fuser` | `userspace/fuser` |
| `userspace/sysstat` | `iostat` | `userspace/iostat` |
| `userspace/chpasswd` | `passwd` | `userspace/passwd` |

`head`/`tail` is the clearest: `userspace/head/src/main.rs:219` implements
`tail`, there is no `userspace/tail` crate, and `coreutils/src/bin/tail.rs` is
the one under differential test. So the standalone `tail` can never run, and
this is invisible to the filename-collision check in
`B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES` — the two executables have
*different* names; only the behaviour is duplicated.

**How to reproduce:**

```sh
python - <<'PY'
import re, pathlib
US = pathlib.Path("userspace"); CU = US/"coreutils"/"src"/"bin"
cu = {p.stem for p in CU.glob("*.rs")} | {p.name for p in CU.iterdir() if p.is_dir()}
CHAIN = re.compile(r'((?:"[a-z][a-z0-9_.+-]{1,20}"\s*\|\s*)*"[a-z][a-z0-9_.+-]{1,20}")\s*=>\s*Personality::')
LIT = re.compile(r'"([a-z][a-z0-9_.+-]{1,20})"')
for main in sorted(US.glob("*/src/main.rs")):
    crate = main.parent.parent.name
    t = main.read_text(encoding="utf-8", errors="replace")
    if (m := re.search(r"^#\[cfg\(test\)\]", t, re.M)): t = t[:m.start()]
    names = {n for ch in CHAIN.findall(t) for n in LIT.findall(ch)} - {crate}
    dead = [a for a in sorted(names) if not (US/a/"Cargo.toml").is_file() and a not in cu]
    if dead: print(f"{crate:<14} {' '.join(dead)}")
PY
```

**The proper fix**, in order:

1. **Decide the shape first** — this is the same question as
   `open-questions.md` → **B-Q7**, and doing anything here before it is answered
   means doing it twice. Note that this entry is *evidence* for B-Q7: §8's rule
   is "one tool = one crate = one binary = one identity", and the standalone
   side violates it in at least 19 crates, which is where all of this dead code
   came from.
2. **Prefer splitting the personalities into real binaries over adding links.**
   Per `coreutils-canonical-answer.md`, one executable answering to many tool
   names is the shape this OS's capability model exists to exclude: the kernel
   grants permissions per file, so one file serving `sudo`, `visudo` and
   `sudoedit` must hold the union of what all three need. Adding symlinks would
   make the commands work *and* create exactly the least-privilege problem §8
   was written to avoid.
3. **Add a test that fails when a personality has no producer.** The whole
   defect is that nothing checks; the reproduction above is most of such a test
   already. Without it this recurs the next time someone writes an `argv[0]`
   branch.
4. For the six shadowed aliases, delete the dead branch and keep one
   implementation — after comparing them, since they disagree (`head`'s `tail`
   vs `coreutils`'s `tail` are separate codebases).

**If never fixed:** no regression and nothing breaks today, because nothing
stages these binaries into an image yet (`create-ext4-rootfs.sh` stages
fastpy-compiled ELFs). But it is a large amount of completed work that a user
can never invoke, and every week that passes adds another personality written
the same way. The first image that stages Rust userspace binaries will ship a
system where `e2fsck` — the checker for our only filesystem — is not a command.
