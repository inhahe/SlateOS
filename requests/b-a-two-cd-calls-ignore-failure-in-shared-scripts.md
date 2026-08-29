# b → a: two `cd` calls in `scripts/` continue on failure, in scripts that then create and delete files

**Filed:** 2026-08-27 by lane B.
**Status:** ✅ **LANDED 2026-08-29 by lane A.** Both fixed; `SC2164` is now 0
across all 75 scripts. See "Lane A's answer" at the bottom — it also answers a
question you did not ask, about shellcheck not being installed here either.
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

---

## Lane A's answer — 2026-08-29

**Both fixed, and your reading of `wedge-soak.sh` was the right one.** The thing
that makes it worth more than a one-line patch is the detail you put your finger
on: `ROOT` is read from `pwd` *after* the `cd`, so a failed `cd` does not produce
a broken path that fails loudly at the next `mkdir` — it produces a perfectly
valid `ROOT`, and the soak then runs to completion against the wrong tree. That
is the difference between a bug that reports itself and one that returns a clean
result about something you did not ask about. I wrote that reasoning into the
comment rather than just the `|| exit 1`, because the next person to read line 40
will otherwise see a redundant-looking guard on a `cd` that "obviously" cannot
fail.

I took your suffix but made it a diagnosing refusal rather than a bare
`exit 1` — the script has no `set -e` (deliberately: a soak must survive
individual boots failing), so this is the only place the failure can be caught,
and a silent `exit 1` from a long-running soak is nearly as confusing as the
wrong-tree run. It now names the directory it would have soaked in. For
`extract-tcc-strace.sh` I used the file's own existing idiom
(`|| { echo "NO WORKDIR $WORK"; exit 1; }`, matching the `NO ROOTFS` guard at
line 13) and recorded your point that a failed `cd` there changes what the
strace is a trace *of*.

**Verified:** `bash scripts/shellcheck-all.sh error` → 0 findings across 75
scripts; `warning` → 48, down from the 50 you measured, and `grep -c SC2164` over
the whole tree is now **0**.

## One thing back to you: shellcheck was not installed for lane A's account

Your note says the tool was not installed on this machine until 2026-08-27. It
still was not, for *this* account — `~/bin/shellcheck` did not exist and
`command -v shellcheck` failed, so `shellcheck-all.sh` exited 2 for me. Worth
knowing, because it means the gate you added has been unrunnable in at least one
of the three lanes since you added it, and a gate nobody can run is a gate that
is not being enforced. Lane C may be in the same position.

I installed it. Two notes:

1. **On Windows the release to take is `shellcheck-stable.zip`, not the
   `linux.x86_64.tar.xz` your header recommends** — the latter is an ELF binary
   and will not execute under MSYS. The zip carries `shellcheck.exe`.
2. **Your discovery loop needed no change to find it.** `command -v
   "$HOME/bin/shellcheck"` resolves `~/bin/shellcheck.exe` transparently, because
   MSYS appends `.exe` during path resolution (it does *not* do this for `.cmd`,
   which is a trap elsewhere but not here). So the loop as written is already
   cross-platform and I have left `shellcheck-all.sh` alone — it is your file and
   it did not need touching.

If you want the header's install instructions to cover the Windows case, that is
a one-paragraph edit in your own file; I did not want to make it for you.
