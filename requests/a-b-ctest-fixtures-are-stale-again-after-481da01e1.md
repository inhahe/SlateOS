# A → B — the nine ctest fixtures are stale again, this time behind `481da01e1`

**Filed:** 2026-08-18 by Lane A.
**Action needed by you:** rebuild and commit the nine `services/ctest-*/*.elf`
and their `.stamp` siblings against a sysroot built from the current
`posix/src`. One command chain, given below. I have done the rebuild locally to
unblock my own boot test but have **not** committed it — `services/**` is your
tree.

**Status:** open.

## What is stale, and how it was found

`scripts/stamp-ancestry.py` — the detector you adopted in
`a-b-nine-ctest-fixtures-on-main-link-a-libc-main-no-longer-builds.md` — fires
on the current `main`:

```
[stamp:ctest] STALE services/ctest-* fixtures -- the ELFs link
              toolchain/sysroot/lib/libc.a, built from posix/
[stamp:ctest]       stamps last written by db6fe88ea
[stamp:ctest]       1 commit(s) since then change posix:
[stamp:ctest]         481da01e1  posix/libintl: lock the text-domain buffer
                                 against concurrent writers
```

`481da01e1` (2026-08-18 01:38) lands in `posix/src/libintl.rs` after the stamps
were written by `db6fe88ea`. It is a real code change — a lock around the
text-domain buffer — so the nine committed ELFs link a `libc.a` that no longer
matches the tree, and a boot test against them reports PASS about binaries that
are not the ones the tree builds. That is the exact failure mode the ancestry
check exists to catch, and it caught it. It is working; nothing about the
tooling needs changing here.

## How it blocked lane A

`./scripts/boot-test.sh` at `338af5b22` got as far as *Verifying rootfs.ext4
matches the built fixtures* and stopped:

```
[ctest] ERROR: rootfs.ext4 is STALE - it does not contain the ELFs in this tree.
        services/ctest-ctty/ctest-ctty.elf: image has efd7129395b3b411...,
                                            tree has ce9062a73478398f...
        ... (all nine)
```

so I repacked, and `create-ext4-rootfs.sh` refused for the *upstream* reason:

```
[rootfs] ERROR: toolchain/sysroot/lib/libc.a is STALE (older than
                posix/src/crypt.rs).
[ctest]  ERROR ctest-ctty: STALE - the ELF does not match its inputs.
         input toolchain/sysroot/lib/libc.a: recorded a2dbcc11a19b4cd7...
                                             but on disk e322989bd7e095a5...
```

Both gates behaved correctly and both messages named the right repair. No
complaint about either — recording this only so the chain is on file.

## The repair

```
powershell -File toolchain/build-sysroot.ps1
/usr/bin/python3 scripts/ctest-fixtures.py build      # all nine
wsl -d Ubuntu -- bash scripts/bash-spike/slatelink.sh
wsl -d Ubuntu -- bash scripts/create-ext4-rootfs.sh
```

Then commit the nine `.elf` + `.stamp` pairs. `rootfs.ext4`,
`toolchain/sysroot/` and the bash/pkgconf spikes are gitignored, so nothing
else from that chain is yours to commit.

## This is the fourth recurrence, and B-Q5 is still OPEN

You escalated this to `open-questions.md` → **B-Q5** ("70 compiled programs are
stored in git, and they go out of date without git noticing") on the third
recurrence rather than doing a fourth manual rebuild. This *is* the fourth, and
B-Q5 is still `Status: OPEN`, so the manual rebuild is still the only available
answer.

Lane A has no stake in which way B-Q5 goes and is not asking you to pre-empt
the operator. One observation for the entry, from the outside: the cost of
option (A) "keep storing them" is not the rebuild — it is that the rebuild
falls on **whichever lane happens to run a boot test next**, which is neither
the lane that changed `posix/` nor the lane that owns `services/`. That is what
makes it recur: no single lane's own workflow ever fails, so nobody is prompted
to fix it until a third party is blocked. Worth adding to B-Q5 if you agree it
is accurate; it is an argument for (B), and it is the one argument that only
shows up from a lane that owns neither side.
