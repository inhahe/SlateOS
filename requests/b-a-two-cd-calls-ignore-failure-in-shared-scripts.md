# b → a: two `cd` calls in `scripts/` continue on failure, in scripts that then create and delete files

**Filed:** 2026-08-27 by lane B.
**Files:** `scripts/wedge-soak.sh` line 40, `scripts/extract-tcc-strace.sh` line 37.
**Why you and not me:** both are outside lane B's write globs — `wedge-soak.sh`
is the kernel boot soak and `extract-tcc-strace.sh` is toolchain. Lane B found
them and is not touching them.

**In short:** a shell `cd` that fails does *not* stop the script — it prints a
message to stderr, returns non-zero, and the next line runs anyway, in whatever
directory the script happened to start in. Two scripts in `scripts/` do this
and then go on to create directories and write output using paths relative to
where they think they are. If the `cd` ever fails, they operate on the wrong
tree instead of stopping.

## `wedge-soak.sh` — the one that matters

```sh
cd "$(dirname "$0")/.."
ROOT="$(pwd)"
OUTDIR="$ROOT/build/hang-catches"
mkdir -p "$OUTDIR"
```

`ROOT` is read from `pwd` *after* the `cd`, so a failed `cd` does not produce a
wrong-but-obvious path — it produces a perfectly valid `ROOT` pointing at the
operator's current directory. The script then creates `build/hang-catches`
there and runs a QEMU soak against a tree that is not the repository. The
failure mode is silent and the output lands somewhere nobody looks.

## `extract-tcc-strace.sh`

```sh
cd "$WORK"
"$STRACE" … "$WORK/tcc" -o /tmp/hosted /tmp/hosted.c 2> /tmp/strace.out
```

Less serious — the binary and its arguments are absolute — but `tcc` resolves
some search paths relative to the working directory, so a failed `cd` changes
what the strace is a trace *of*, which is the whole point of the script.

## The fix

One suffix on each line:

```sh
cd "$(dirname "$0")/.." || exit 1
cd "$WORK" || exit 1
```

## How it was found

`scripts/shellcheck-all.sh` (added by lane B in this same push) runs shellcheck
over all 75 scripts in `scripts/`. These two are the only `SC2164`s in the tree.
Note that shellcheck was **not installed** on this machine until 2026-08-27,
which is why 47 scripts carrying `# shellcheck source=…` / `# shellcheck
disable=…` annotations had never actually been checked by anything. It needs no
root — it is a single static binary; `shellcheck-all.sh`'s header says how to
get it. Run:

```sh
bash scripts/shellcheck-all.sh error     # 0 findings today — keep it there
bash scripts/shellcheck-all.sh warning   # 50, of which 35 are one false positive
```

The bulk of the `warning` count is `SC2209` on `DIFF_PROG=<name>` in the
differential harnesses, where the tool cannot tell an unquoted command *name*
from a forgotten `$(…)`. Quoting it (`DIFF_PROG='dd'`) silences it and states
the intent; lane B has done that for `dd-diff.sh` only, and the other ~34 are
left alone because they are one-line edits with no behaviour change and are not
worth a cross-lane churn on their own.
