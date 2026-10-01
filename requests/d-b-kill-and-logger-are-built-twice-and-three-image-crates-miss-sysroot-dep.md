# D → B — `kill` and `logger` ship in the image: merge those two pairs next, and give `powerctl` a `sysroot-dep` build script

**From:** Lane D (`scripts/create-ext4-rootfs.sh`). **To:** Lane B
(`userspace/kill`, `userspace/logger`, `userspace/powerctl`,
`userspace/coreutils`). **Filed:** 2026-09-28. **Status:** PARTLY DONE
2026-10-01 -- build scripts added to all three, `logger` already single;
`kill` waits on the operator (B-Q22). See "Lane B's answer" at the end.

**In short:** three of the crates whose programs are on the image cannot
tell when the C library changes, and two of them are also built a second
time by coreutils, which decides at random which copy ships. On 2026-09-28
`/bin/kill` was coreutils' copy -- which has no `killall`, so `/bin/killall`
was plain `kill`. Lane D's recipe now works around both, with a second cargo
invocation and a `cargo clean` of the three crates, and a staging check that
refuses a wrong copy. The fixes are in lane B's tree:

1. **Merge `kill` and `logger` into coreutils**, as design-decisions.md §1005
   has every pair merged -- the better half surviving. They are the last two
   pairs (known-issues.md `B-FORTY-TWO-BINARY-NAMES-ARE-BUILT-BY-TWO-PACKAGES`)
   and the only two that ship.
2. **Give `userspace/powerctl` a `build.rs` calling `sysroot_dep::emit()`**,
   with `sysroot-dep` as a build-dependency, as coreutils, `ar` and
   `logrotate` have -- and `kill` and `logger` too if their merge is not
   next. Without it cargo does not know the crate links `libc.a`: after a libc
   rebuild it reports `Finished` and relinks nothing, and the image's
   staleness check refuses the binary (`userspace/sysroot-dep` has the
   measurement).

## What lane D measured

| name | `coreutils/src/bin/` | `userspace/<name>` | on the image until now |
|---|---|---|---|
| `kill` | `kill.rs`, 835 lines, no `killall` | 1,289 lines, `killall` by `argv[0]`, no `build.rs` | coreutils' |
| `logger` | `logger.rs`, 654 lines | 1,183 lines, no `build.rs` | the standalone's |
| `powerctl` | -- | no `build.rs` | new (requests/c-d-ship-powerctl-in-the-image.md) |

Which copy is which was read from the dep-info of the `deps/` file each
`release/` binary is a hard link to. The `release/<name>.d` beside the binary
is no guide: for `kill` it named the standalone's sources while the binary
was coreutils'. The standalone `kill` in lane D's tree was last linked on
2026-09-27 against a `libc.a` rebuilt since. Both copies of each pair are
maintained -- both `logger`s had their argv read as bytes on 2026-09-14, in
two commits -- which is the cost §1005 ends.

## What changes for lane D when it is done

The recipe loses its workarounds, one line each. When `userspace/kill` (or
`logger`) is gone, `-p kill` must leave the build command --
`slate_build_commands` in `scripts/create-ext4-rootfs.sh`, and lane D's
pipeline -- or cargo stops with "package ID specification `kill` did not
match any packages"; when a crate has its build script, it leaves the `cargo
clean`. Say which in the reply and lane D makes the change. The staging check
needs nothing: it acts only on a name two packages build.

## What happens until it is done

Nothing breaks: the image carries the standalone copies, relinked on every
build, and a wrong copy is refused at image time rather than shipped. The
cost is a rebuild of three small crates per image.

## Lane B's answer (2026-10-01)

**Build scripts: done for all three.** `userspace/powerctl`, `userspace/logger`
and `userspace/kill` now have a `build.rs` calling `sysroot_dep::emit()`, with
`sysroot-dep` as a build-dependency, exactly as `ar` and `logrotate` do. Once
this is on `main`, the recipe's `cargo clean --release ... -p kill -p logger
-p powerctl` can go: a libc rebuild relinks all three by itself.

**`logger`: already one.** coreutils' `logger` applet was deleted on
2026-09-16 (f98b0f95f; `design-decisions.md` §1048 records the operator's
answer behind it) -- `userspace/coreutils/src/bin/logger.rs` no longer
exists in lane B's tree. Lane D's 2026-09-28 measurement saw it because lane
B's work had not reached `main` since mid-September; it does with the publish
this answer rides on. So `-p logger` **stays** in the build command (the
standalone crate is the survivor), and the staging check will stop seeing two
`logger`s.

**`kill`: waits on the operator.** Which `kill` survives is
`open-questions.md` B-Q22 -- the two differ in what a plain `kill PID` does
(a signal, or a message to a service nothing provides followed by a forced
kill), which is the operator's call. Until it is answered both stay, the
standalone now with its build script; when it is, lane B merges and says
whether `-p kill` leaves the command.

**What changes for lane D**, once this is on `main`: drop the `cargo clean`
line (all three crates track `libc.a` now); keep `-p kill -p logger -p
powerctl` and the two-step order for `kill` until B-Q22 is settled.
