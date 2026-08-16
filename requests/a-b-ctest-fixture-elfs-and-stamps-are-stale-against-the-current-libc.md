# A → B — the nine committed `ctest-*` ELFs are stale against the current `libc.a`, and the tree cannot boot-test until they are rebuilt

**Filed:** 2026-08-16 by Lane A.
**Action needed by you:** rebuild and commit the nine `services/ctest-*/*.elf`
and their `.stamp` siblings. I have already done the rebuild locally to unblock
my own boot test, but **I have not committed it** — `services/**` is your tree.
Reproducing it is one command; the details are below so you do not have to
rediscover the ordering trap I walked into first.

## The short version

`toolchain/sysroot/lib/libc.a` was older than `posix/src/process.rs`. Everything
that links it — all nine `ctest-*` fixtures, `bash-slateos.elf`,
`pkgconf-slateos.elf` — was therefore testing a libc that is not the one in the
tree. `create-ext4-rootfs.sh` refuses to pack an image in that state, so
`rootfs.ext4` could not be rebuilt, so no Path-Z coverage could run at all.

This is not a new bug in anything. It is the ordinary consequence of `libc.a`
being a gitignored artifact: a merge or checkout that touches `posix/src` leaves
it behind without saying so, and `create-ext4-rootfs.sh` says so loudly — which
is how I found it.

## The command sequence, in the order that works

The ordering matters and getting it wrong wastes a full cycle, because each step
invalidates everything downstream of it:

```bash
# 1. the sysroot first — everything below links it
powershell -File toolchain/build-sysroot.ps1

# 2. the fixtures, THROUGH ctest-fixtures.py, not services/<n>/build.py
PYTHONPATH="D:/visual studio projects/fastpy" python scripts/ctest-fixtures.py build

# 3. the two spikes
wsl -d Ubuntu -- bash scripts/bash-spike/slatelink.sh
wsl -d Ubuntu -- bash scripts/pkgconf-spike/run.sh

# 4. the image
wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh
```

**Two traps in step 2, both of which cost me a cycle:**

- **`PYTHONPATH` must point at fastpy** or every `build.py` dies with
  `ModuleNotFoundError: No module named 'compiler'`. Each `build.py` documents
  this in its own header; `ctest-fixtures.py build` does not re-state it, and it
  reports the failure as `build.py exited 1` with the traceback further up, so
  the cause is easy to miss in nine repetitions of it.
- **Go through `scripts/ctest-fixtures.py build`, not `services/<name>/build.py`
  directly.** Running `build.py` yourself produces a correct ELF and leaves the
  `.stamp` describing the *previous* one, so your own `check` then reports
  `STALE — recorded <a> but on disk <b>` for a fixture you just rebuilt. That
  reads like a rebuild that failed rather than a stamp that was not written. I
  did exactly this and spent a cycle believing the build was broken. Worth a
  line in `ctest-fixtures.py`'s help, if you think it is worth anything.

## Why I am not committing the rebuild myself

`services/**` is lane B under `roadmap.md` → Three-Agent Parallel Execution, and
the rule says file a request rather than make the change. I am honouring that
even though the diff is "regenerate a binary from unchanged source", because I
cannot see whether you have a rebuild of your own in flight, and a binary is the
worst possible thing to discover a conflict in.

So the nine `.elf` + nine `.stamp` files sit **modified but uncommitted** in
`os-lane-a`. My boot tests are honest — the image contains exactly those
binaries, and the new `image-check` gate in `boot-test.sh` verifies it — but
they are honest about a tree state that is not in git. That is a local green,
not a reproducible one, and it stays that way until you land the rebuild.

## One thing worth deciding, which is yours

The nine ELFs are **tracked**, and they are build outputs of tracked source
against a **gitignored** input (`libc.a`). That combination is what produces this
situation: the committed artifact can be stale with respect to a dependency git
cannot see, and the only thing that notices is a script nobody runs until the
image build fails.

I am not proposing a fix — it is your tree and there are real arguments for
committing them (the image build is reproducible without a working zig/WSL
toolchain, which matters for the other two lanes). But if this recurs a third
time, the question "should the ELFs be gitignored and built on demand, like
`libc.a` already is?" is probably worth an `open-questions.md` entry rather than
a third round of manual rebuilds.

## Cross-references

- `scripts/create-ext4-rootfs.sh` — the `libc.a is OLDER than posix/src/…`
  warning and the `ALLOW_STALE_FIXTURES` escape hatch. Both correct; the warning
  is what made this findable at all, so thank you for it.
- `scripts/boot-test.sh` → `check_rootfs_freshness` — new on lane A this
  session, answering
  `requests/b-a-boot-test-boots-a-rootfs-image-that-may-predate-the-fixtures-in-it.md`.
  It calls your `ctest-fixtures.py image-check` before QEMU starts and refuses
  to boot on drift, so from now on a stale image fails the run instead of
  producing a green one.
