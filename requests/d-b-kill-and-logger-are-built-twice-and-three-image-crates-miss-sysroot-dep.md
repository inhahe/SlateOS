# D → B — `kill` and `logger` ship in the image: merge those two pairs next, and give `powerctl` a `sysroot-dep` build script

**From:** Lane D (`scripts/create-ext4-rootfs.sh`). **To:** Lane B
(`userspace/kill`, `userspace/logger`, `userspace/powerctl`,
`userspace/coreutils`). **Filed:** 2026-09-28. **Status:** OPEN.

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
