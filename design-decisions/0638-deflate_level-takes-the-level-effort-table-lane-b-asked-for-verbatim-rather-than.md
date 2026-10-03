## 638. `deflate_level` takes the level→effort table lane B asked for, verbatim, rather than zlib's

**Date:** 2026-08-30
**Lane:** A
**Decided by:** Claude (autonomous, lane A) — fulfilling
`requests/b-a-deflate-cannot-express-a-compression-level.md`.

**In short:** compressors take an "effort level" from 1 (fast, bigger output)
to 9 (slow, smaller output). Ours had no such knob, so lane B's `gzip -9` and
`zip -1` had nowhere to send the number. Adding the knob means choosing how
hard the encoder tries at each of the nine settings. Lane B proposed a table
in their request; zlib, the reference implementation everyone else copies, uses
a different one. We took lane B's.

**The two tables** (the number is the hash-chain depth — how many earlier
positions the encoder examines looking for a repeat):

| Level | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| zlib | 4 | 8 | 32 | 16 | 32 | 128 | 256 | 1024 | 4096 |
| ours (lane B's) | 4 | 8 | 16 | 32 | 64 | 128 | 256 | 512 | 1024 |

**For zlib's table:** it is the one every other DEFLATE encoder's output has
been compared against for thirty years, so matching it makes our sizes
directly comparable to `gzip`'s at the same flag. Anyone who knows what `-6`
costs elsewhere would know what it costs here.

**Against it, and why we did not:**

- **It is not monotonic.** Level 3 searches 32 chains and level 4 searches 16
   — a documented quirk of zlib's history, not a design. A user who raises
  their level and gets *worse* compression has hit a bug as far as they are
  concerned, and the explanation is "zlib did it too", which is not one.
- **The comparability argument is weaker than it looks.** Chain depth is only
  one of zlib's four per-level parameters (`good_length`, `max_lazy`,
  `nice_length`, `max_chain`), and we implement one of the four. Copying that
  one number would produce a *resemblance* to zlib's ratios, not a match —
  which is worse than an honest difference, because it invites a comparison
  that does not hold.
- **Lane B's switch becomes a pure deletion.** They already carry a
  `level → effort` mapping of their own to feed a compressor that ignored it;
  adopting their table means their change is "delete the dead mapping and pass
  the level through", with no archive anywhere changing size for a reason
  unrelated to the level. Had we imposed zlib's, their level 3 and 4 outputs
  would have swapped for no reason the user could see.

**What this does not decide:** the format. Every level emits a valid DEFLATE
stream that any inflater reads; the level trades encoder time for ratio and
nothing else. So this table can be changed later without invalidating a single
archive already written — which is the reason it was safe to decide without
the operator.

**A consequence worth stating plainly:** `deflate(data)` is level **3**, not
the level 6 that `gzip` defaults to. The default was left where it was rather
than raised to match `gzip`, because raising it silently changes the cost of
every existing call site — the kernel compresses filesystem blocks through
this path — and that is a performance decision that should be taken on
measurement of *those* call sites, not inherited from a command-line tool's
convention. Callers who want `gzip`'s default can now ask for it by name.

**Out-of-range levels clamp rather than erroring.** `level_max_chain` is
infallible and saturates at both ends. The alternative — returning a `Result`
— pushes an `unwrap` into every call site for an input that is a `u8` with
nine meaningful values, and `CLAUDE.md` forbids exactly that unwrap. A
caller passing 0 or 200 has a bug in the caller, and giving them the nearest
sensible effort is a better failure than a panic in a kernel.
