## B-PATCH-HAS-NO-DIRECTORY-OPTION (lane B, 2026-09-12) — FIXED, and the harness does not prove it

`patch -d DIR` / `--directory=DIR` is implemented. **The order is the whole
behaviour** and the obvious implementation gets it backwards: GNU changes
directory *before* opening the patch file named by `-i`, so a relative `-i` is
relative to DIR rather than to where the command was typed. Measured:

    $ patch -i u.patch -p1 -d a
    patch: **** Can't open patch file u.patch : No such file or directory

with `u.patch` sitting beside `a/` rather than inside it. Resolving `-i` first
would have succeeded where GNU fails.

**The harness count did not move, and that is worth saying rather than hiding.**
`patch-diff.sh` has three `-d` cases and all three still differ — but now for a
*downstream* reason: after the chdir, neither side can find the target, and GNU
reports `can't find file to patch at input line 3 / Perhaps you used the wrong
-p or --strip option?` where this build reports `can't open file …`. That is a
separate missing diagnostic, not a fault in `-d`.

**So it was verified directly instead**, against GNU, in a layout where the
patch file is still reachable after the chdir:

| | exit | output | resulting file |
|---|---|---|---|
| GNU | 0 | `patching file f.txt` | `c`, `b` |
| ours | 0 | `patching file f.txt` | `c`, `b` |

**A fix that does not move the number it was meant to move needs its own
evidence.** The alternative is to report three still-failing cases as though
the work had not been done, or to report the work as done on no evidence at
all. A harness case that would certify `-d` needs the patch file reachable from
inside the directory; the three that exist do not have that and cannot.
