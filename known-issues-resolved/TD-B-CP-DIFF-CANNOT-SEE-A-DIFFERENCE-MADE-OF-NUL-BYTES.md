## TD-B-CP-DIFF-CANNOT-SEE-A-DIFFERENCE-MADE-OF-NUL-BYTES (lane B, 2026-09-14)

**Status: CLOSED 2026-09-16** — the fix landed 2026-09-14 (`49416d81a`) and
this entry went on saying "reverted unverified" for two days. The verification
it asked for was never done; it is done now, and permanent:
`scripts/check-cp-diff-sees-nul.py`.

**Both halves it demanded, as a gate rather than a one-off.** Two trees
differing only in NUL bytes now compare DIFFERENT through the capture, and
byte-identical trees still compare EQUAL. The gate extracts the real
`contents()` out of `cp-diff.sh` and sources it, so it grades the function as
edited rather than a reimplementation of it -- and its self-test sabotages that
function by deleting the `sha` line, proving the gate can fail.

**Writing it cost four silent ways to not run, which is the finding.** Each
produced output indistinguishable from a verdict:

1. `tempfile.TemporaryDirectory()` yields `C:/Users/...`, which MSYS bash
   cannot resolve. The source failed, `contents` was never defined, both
   captures were empty -- and empty equals empty, so the gate announced
   "two trees differing only in NUL bytes compare EQUAL" and told the reader
   `contents()` needed its hash line back. **The harness was fine.**
2. `Path.write_text` translated the function to CRLF. `contents() {
` is a
   syntax error, so the function was never defined. Same empty, same verdict.
3. Under `bash -c`, a function is NOT visible inside a command substitution on
   this Git Bash -- MSYS emulates fork by re-execing and the definition does
   not survive. `declare -F contents` reported it DEFINED in the same script
   where `$(contents ...)` said `command not found`. Fixed by running the
   probe from a script file.
4. The substitution cannot simply be dropped to avoid (3): the capture is what
   eats the NULs, so a probe reading the function on a pipe would not
   reproduce the defect at all.

The repair for all four is one function, `ran_at_all()`, which requires two
markers: `DEFINED` (the sourcing worked) and `NONEMPTY` (calling it produced
output). A defined function returning nothing compares equal to itself on every
tree and reads as a clean verdict, which is how (1) got as far as a confident
wrong answer. **A comparison that could not be made is not a comparison that
failed** -- and the gate written to catch one instance of that defect produced
four of its own before it worked.

`scripts/cp-diff.sh` compares the copied tree's file contents by capturing them
in a command substitution:

```sh
o_body=$(contents "$o_dir" | scrub "$o_dir"); g_body=$(contents "$g_dir" | ...)
```

A command substitution **drops NUL bytes**, and bash says so, once per file:

    cp-diff.sh: line 419: warning: command substitution: ignored null byte in input

Both sides lose them identically, so two files differing **only** in NUL bytes
compare EQUAL and the harness reports the copy as faithful. That is the same
shape as the `diff` bug fixed earlier today: comparing a lossy projection of
the thing rather than the thing.

`contents()` is what feeds it, and it `cat`s each file raw — so every fixture
holding a NUL reaches the capture. The warnings have presumably been printed on
every run for as long as those fixtures have existed.

### The fix, written and then backed out

Add a per-file checksum inside `contents()`, above the body:

```sh
printf '== %s\n' "$f"
printf 'sha %s\n' "$(sha256sum <"$f" 2>/dev/null | cut -d' ' -f1)"
cat -- "$f"
```

The sum is over the raw bytes and is plain hex, so it survives the capture; the
readable body stays beneath it so a failure is still diagnosable rather than a
wall of differing hashes. Both sides get it, so no expected output changes —
what changes is that a NUL-only difference stops being invisible.

**Backed out because I could not verify it.** `all-diff.sh` was running, the
machine was saturated, and three attempts at an isolated probe either mangled
in the shell layers or timed out. A harness change that has not been shown to
(a) catch the case it is for and (b) still call identical trees identical is
exactly the kind that turns green into noise for the other two lanes. The
diagnosis is solid and the patch is above; applying it wants a quiet machine
and the two-way probe, not a confident-sounding commit.
