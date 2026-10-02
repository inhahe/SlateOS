## `B-tar-EVERY-ARCHIVE-RECORDED-THE-OWNER-AS-A-BARE-NUMBER` (lane B, 2026-08-29) -- **FIXED 2026-08-29**

**In short:** a tar archive records who owns each file twice -- once as a
number, and once as the account *name* that number stood for on the machine the
archive was made on. Only the name is portable: user number 1000 is a different
person on every machine. Ours filled in the number and left the name blank, for
every file of every archive it ever wrote. Restore such an archive on a
different machine and the files come out belonging to whoever happens to hold
number 1000 there -- which on a multi-user machine is simply somebody else.

### Measured, both sides

The `uname` field of the first header, an archive of one file owned by
`inhahe` (uid 1000):

```
gnu : uname=[inhahe]  gname=[inhahe]
ours: uname=[]        gname=[]
```

The blank field is not meaningless -- it is the encoding for "no name
recorded", which is exactly what `tar --numeric-owner` writes on purpose. So
ours was silently producing numeric-owner archives whatever it was asked for,
and nothing in the output said so. `tar -tvf` on our archive showed
`1000/1000` where GNU's showed `inhahe/inhahe`, which was the only visible
symptom.

### The fix

`Creator` gained an `OwnerNames`: the `pwdb` account database plus a
`BTreeMap` cache per id, loaded once per run. `Creator::header` now sets
`uname`/`gname` beside the numbers it already set.

Three measured details, from `tar-uname1.sh` and `tar-uname2.sh`:

| Case | GNU | Ours |
|---|---|---|
| id is in the database | the name | same |
| id is **not** in the database | field left empty | same |
| name is 32 bytes or longer | cut to 31 + NUL, silently, rc 0 | same |

The empty-field case is the one worth stating, because "fall back to the
number" is the tempting wrong answer: writing `"4242"` as a *name* would make a
reader restore ownership to an account literally called `4242` if the
destination machine had one. Leaving it empty says "you only have the number",
which is true.

The 31-byte cut is not a guess either. It follows from tar's `tar_copy_str`,
which stops at the first NUL *or* at the field width and so leaves the array's
own zero byte in place -- unlike the `name` field, which is legally unterminated
when full.

### Why it survived so long

`scripts/tar-diff.sh` normalised it away. `--numeric-owner` was in the GNUFMT
flag set, with a comment saying so, precisely so that this one known gap could
not mask every other difference in the same 512 bytes -- and an xfail at the
bottom recorded the gap. That was the right call when the gap was open and the
wrong shape to leave standing: a normalisation is a blind spot with a comment
on it. The flag is now gone from GNUFMT and the xfail with it, so all 13
`create_case`s compare `uname`/`gname` byte for byte along with the rest of the
header -- and `header_field` names the field if they ever diverge again. The 5
`interop_case`s gain it too: they read our archive with GNU's `-tv`, which
prints the owner *name* when the field holds one.

The generalisable point: **an xfail plus a normalisation is one gap recorded
twice, and only the xfail is load-bearing.** The normalisation is what stops
the differential from ever telling you the gap is closed -- so when the xfail is
retired, the normalisation has to go in the same change, or the fix is
unverified.
