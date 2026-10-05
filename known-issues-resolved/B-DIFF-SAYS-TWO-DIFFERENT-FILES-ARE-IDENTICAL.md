## B-DIFF-SAYS-TWO-DIFFERENT-FILES-ARE-IDENTICAL (lane B, 2026-09-14)

**Status:** FIXED 2026-09-14 · `userspace/coreutils/src/bin/diff.rs`

**How it was closed.** Lines are `Vec<u8>` from `fs::read` to the writer:
`FileContent::Text`, `compute_diff`/`lcs_diff`/`myers_diff`'s four slices and
their `(Op, line)` result, `Hunk::lines`, `normalize_line`, `is_blank`. The
renderers emit the line's own bytes through one `write_body_line` rather than
`format!`, because GNU writes it raw and `diff -u | patch` reads it back.
`normalize_line` folds with `to_ascii_lowercase`/`is_ascii_whitespace`, which
the two measurements above showed is what GNU does — so `-i` and `-w` each
shed a divergence as a side effect. `truncate_or_pad`, the only place a width
is needed, still counts characters when the line is valid UTF-8 and falls back
to bytes when it is not, so no line that aligned before moved.

**Measured:** `scripts/diff-diff.sh` went from **43 passed / 64 differed** to
**46 / 61** — three cases fixed, and `comm` over the two runs confirms
**nothing newly differs**. Four unit cases were added, including a control
asserting the *same* bad byte still compares equal; reintroducing the lossy
decode turns the bug's case red and correctly leaves the control green.

**The part worth keeping.** The three cases that went green are
`diff bytes.txt bytes2.txt`, `diff -q bytes.txt bytes2.txt` and
`diff base.txt bytes.txt`. **The harness already had those fixtures and was
already failing them.** The bug was being reported on every run and was
invisible because it sat among 64 other divergences — a harness with a large
standing red count cannot tell anyone that something new broke. `diff` is now
the tree's biggest such backlog at 61, which is an argument for baselining it
the way `argv-utf8` and `raced-globals` are baselined, so the number that gets
watched is *new* divergences rather than all of them.

`diff` reports **no difference** between two files that differ, and exits 0.
Measured, with `cmp` as the control:

    $ cmp x.txt y.txt
    x.txt y.txt differ: char 10, line 2

    $ diff x.txt y.txt          # ours
    $                           # nothing at all, exit 0

    $ diff x.txt y.txt          # GNU diffutils
    2c2
    < cafM-i
    ---
    > cafM-^?

The two files are `alpha/caf\351/gamma` and `alpha/caf\377/gamma`. Byte
`0351` and byte `0377` are different bytes; neither is valid UTF-8 on its
own.

### Cause

```rust
// Convert to string. We use lossy conversion here only for the purpose of
// displaying diff output; the comparison is byte-accurate via the line
// strings.
let text = String::from_utf8(data)
    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
let lines: Vec<String> = text.lines().map(String::from).collect();
```

**The comment is wrong, and it is wrong in the specific way that hides the
bug.** There is no byte-accurate path: `lines` *is* the lossy text, and the
comparison runs on it. Every byte that is not valid UTF-8 becomes U+FFFD, so
any two distinct bad bytes become the same character and compare equal.

`from_utf8_lossy` is named in CLAUDE.md's self-review list — *"No
`from_utf8_lossy` — that's silent data corruption"* — and this is exactly the
failure it is named for. The comment reads as though someone had already
thought about it, which is what makes it worse than no comment.

### Why this is sharper than the `patch` bug fixed today

`patch` **refused**: exit 2, a message, nothing written. Loud and safe.
`diff` **answers wrongly and confidently**. Two consequences:

* `diff expected actual && echo OK` — the idiom every test harness and build
  script uses — passes when it should fail.
* `diff -u` output is fed to `patch`. A diff of a file holding one Latin-1
  byte emits a patch with U+FFFD in the context lines, which then fails to
  match, or matches and writes the corruption in. Today's `patch` fix makes
  `patch` byte-exact, so `diff` is now the weaker half of that pair.

### The fix

The same one `patch` just had: keep lines as `Vec<u8>`. `diff` already reads
with `fs::read`, so the bytes are in hand and thrown away one line later —
there is no I/O change needed, only the type. `FileContent::Text(Vec<String>)`
becomes `Vec<Vec<u8>>`, and the hunk renderer writes the line rather than
formatting it.

One thing to settle first, noted while converting `patch`: GNU writes the
operand **raw** into the `--- path` / `+++ path` header, so that header must
be emitted as bytes. Quoting it would invent a format, and `patch` reads it
back.

`scripts/patch-diff.sh` gained a Latin-1 fixture today; `scripts/diff-*`
should get the same pair of files, and the case above — two files differing
only in a high byte — belongs in it as the regression, because it is the one
that returns the wrong answer rather than an error.

### The one question the conversion had to settle first, now measured

`normalize_line` folds case with `str::to_lowercase`, which is Unicode-aware.
Carrying lines as bytes means `to_ascii_lowercase` instead, so `-i` would stop
folding `É`/`é`. That is a user-visible change and worth checking rather than
assuming — so it was measured, with an ASCII pair as the control:

| input pair | GNU `diff -i` says |
|---|---|
| `cafÉ` vs `café` (U+00C9 / U+00E9) | **DIFFERENT** — not folded |
| `ABC` vs `abc` | SAME — folded |

So GNU folds ASCII case and not Unicode case, which is what a byte-wise
`tolower()` does. **`to_ascii_lowercase` is not a concession to the byte
conversion — it is what GNU actually does**, and our `to_lowercase()` is a
present-day divergence that the conversion removes. Nothing blocks the fix.

`is_whitespace()` in the same function got the same treatment, with two
controls so the probe is known to be sensitive in both directions:

| input pair | GNU `diff -w` says |
|---|---|
| `a<U+00A0>b` vs `ab` | **DIFFERENT** — U+00A0 is not whitespace |
| `a b` vs `ab` (control) | SAME — folded |
| `a<TAB>b` vs `ab` (control) | SAME — folded |

`is_ascii_whitespace` is therefore exactly GNU too. **Both of the conversion's
semantic questions came back the same way**: the byte version is not a
concession, it is closer to the reference than what is there now, and `-i` and
`-w` each lose a divergence. Nothing about the `diff` fix is blocked on a
judgement call.
