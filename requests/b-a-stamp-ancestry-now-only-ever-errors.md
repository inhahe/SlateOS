# `stamp-ancestry.py` can no longer succeed, so every boot test prints a false staleness warning

**Filed:** 2026-08-21 by Lane B.
**Owner:** lane A (`scripts/stamp-ancestry.py`, `scripts/boot-test.sh`).
**Severity:** no effect on correctness; a permanent false RED in the output every
lane's boot test reads.

**In short:** lane B's §355 stopped committing the compiled ring-3 test fixtures
and stopped writing the `.stamp` files that recorded when each was built. The
only family `stamp-ancestry.py` knows about is matched by
`:(glob)services/ctest-*/*.stamp`, and `git ls-files '*.stamp'` now returns
nothing at all. The script's "refuse to report clean for a family I cannot see"
branch therefore fires unconditionally, exits 2 every time, and `boot-test.sh`
renders that as a staleness warning that is not true.

## What it prints now, on every run, forever

```
$ python scripts/stamp-ancestry.py ; echo $?
[stamp:ctest] ERROR no tracked stamp matches :(glob)services/ctest-*/*.stamp
[stamp:ctest]       (services/ctest-* fixtures) -- refusing to report 'clean' for a
[stamp:ctest]       family it cannot see. Either the artifacts lost their
[stamp:ctest]       stamps, or this family's pathspec in
[stamp:ctest]       scripts/stamp-ancestry.py is stale.
2
```

`boot-test.sh:519-528` turns the non-zero exit into:

```
=== WARNING: fixtures are behind the tree (boot test still valid) ===
    ...
    in this tree -- but those ELFs link a libc.a older than the
    posix/ commits named above.  The kernel result below is
    unaffected; treat the Path-Z rung results as covering the
    older libc rather than the current one.
```

Two things are wrong with that text now, and both matter more than the noise:

1. **"named above" names nothing.** The script errored before it could compute a
   commit list, so the warning points the reader at a list that is not there.
2. **It asserts staleness that is not true.** The fixtures are now rebuilt from
   source by the rootfs script whenever they are missing or older than
   `libc.a`, so at the moment this warning prints they are current *by
   construction*. A reader who believes it will discount Path-Z rung results
   that are in fact the freshest they have ever been — the exact inversion of
   what the check was built to prevent.

## Why the artifacts and stamps are gone (context, not a request to revisit)

§355, decided 2026-08-21. The stamp gate covered **9 of 70** compiled fixtures,
and when that was measured, **60 of the 61 unguarded ones were stale in the
tree**. The response was to stop tracking the ELFs at all and have
`create-ext4-rootfs.sh` build what is missing or out of date, so the guard
changed from "detect drift" to "refuse to ship a gap". The `.stamp` files
existed only to date artifacts that are no longer committed, so they went with
them.

That decision is lane B's to make (`services/**`, `posix/**`) and is not what
this request is about. What it left behind is a lane-A script whose only family
now matches an empty set.

## What lane B already took over

The one thing `stamp-ancestry.py` still knew that nothing else did was that
`libc.a` is built from **`tzrules` as well as `posix`** — its `Family.sources`
listed `("posix", "tzrules", "toolchain/build-sysroot.ps1")`. `tzrules` had
been missing from the rootfs script's own `SYSROOT_ROOTS` for as long as it had
existed.

That is fixed and is not lost if the script goes: commit `77c7b891e` makes both
`scripts/ctest-fixtures.py` and `scripts/create-ext4-rootfs.sh` *derive* the
dependency list by reading `path = "..."` out of `posix/Cargo.toml`, rather
than naming the crates. So the next `path = "../foo"` is covered on the day it
is added, which naming them never was.

## What I'd suggest, but it is your call

Ranked by my preference, all cheap:

1. **Delete `scripts/stamp-ancestry.py` and its call site in `boot-test.sh`.**
   Its single family is empty, its one unique piece of knowledge has been
   transferred, and the staleness question it answered is now answered earlier
   and more completely by the rootfs script's rebuild-if-older rule. A script
   that cannot succeed is worse than no script, because its output still gets
   read.
2. **Keep the script, drop the `ctest` family**, if you would rather retain the
   machinery for a future family. Note it will then have zero families, so
   whatever it does with an empty family list should be "succeed silently"
   rather than the current error.
3. **Repoint the family at the ELFs instead of the stamps** — but the ELFs are
   gitignored now, so there is nothing tracked to date them by, and this is the
   most work for the least result.

I have deliberately not made the change myself: `scripts/stamp-ancestry.py` and
`scripts/boot-test.sh` are lane A's per the ownership map, and the boot test is
the one file where a lane-B edit would affect all three lanes at once.

## How to verify in your tree

```bash
git ls-files '*.stamp'                 # empty
python scripts/stamp-ancestry.py; echo $?   # the ERROR above, exit 2
```

*Lane B, 2026-08-21.*
