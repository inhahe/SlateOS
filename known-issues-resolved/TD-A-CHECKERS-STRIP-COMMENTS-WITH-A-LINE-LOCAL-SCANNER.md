## `TD-A-CHECKERS-STRIP-COMMENTS-WITH-A-LINE-LOCAL-SCANNER` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25 (both findings); two lower-risk copies remain, below

> **Resolution.** `strip_noise` in `check-recursive-locks.py` gained a
> `keep_literals=True` mode — literals are still *scanned*, so a `//` inside
> `"https://x"` opens no comment and a `"` inside a comment opens no string,
> but their characters pass through instead of being blanked. Seven new
> self-test cases assert the exact output (22 cases total), because the mode is
> about *which characters survive* and a body list would not notice a literal
> being blanked.
>
> `check-option-refusal.py`, `check-usage-status.py` and `check-query-status.py`
> now use it, each taking three identically-numbered views of one file:
> verbatim for reporting, comments-gone/literals-kept for matching, both-gone
> for brace counting. Four of the five hand-rolled scanners are retired.
>
> **Verified by replay rather than by assertion**, which is this family's own
> stated test convention. Against a 130 000-line revision from before the fix,
> with the exemption tables emptied, `check-usage-status.py` reports the same
> **24** findings and `check-option-refusal.py` the same **53**, byte for byte,
> under both the old and new scanners. Finding 2 was demonstrated the other
> way: on a synthetic block ending `return;  // deliberately no set_exit(1)
> here`, the old `check-query-status.py` reports a false positive and the new
> one is clean, while both still report an unmodified true positive
> identically.
>
> **A third defect surfaced while testing Finding 2.** `check-query-status.py`
> shipped with two `ALLOWED` exemptions that never exempted anything: replaying
> the introducing commit (`425d37b27`) with `ALLOWED` emptied reports zero
> findings, so the detector had never reached those two blocks. Both sites
> still exist and both stated reasons are still correct — the entries were
> written against a shape the checker does not match. They are now prose in the
> file (to paste back if the guard rule is ever widened) and the table is
> empty, with a staleness guard added mirroring the one its sibling has carried
> from the start. An exemption that exempts nothing is exactly the "rubber
> stamp" that file's own comment warns about, and nothing was looking for it.
>
> **Still open, deliberately.** `check-variant-lists.py` and
> `check-tick-wiring.py` keep their own `strip_comments`. Both already blank
> comments *before* counting braces — they are the two that documented the
> hazard in the first place — so neither has the defect this entry is about.
> They are worth folding in for the raw-string and char-literal cases, but that
> is tidying, not a fix.

**In short:** the fix above taught two gate scripts to ignore comments, but
each got its own small comment-stripper that only understands `// …` to
end-of-line. Rust also has `/* … */` (which nests) and raw strings
(`r#"…"#`). A `set_exit(1)` inside a block comment would still be read as
code, which is the same bug in a rarer shape. A survey of the other twelve
`scripts/check-*.py` (done 2026-08-25, read-only, while the boot test ran)
found the problem is both **wider** and **worse** than that, in two ways
recorded below.

**Where.** `strip_comments` in `scripts/check-option-refusal.py` and in
`scripts/check-usage-status.py` — two near-identical copies, which is itself
the smell.

### Finding 1 — there are five hand-rolled Rust lexers, not two

| Script | Blanks | Preserves offsets | Raw strings | Char literals | Self-test |
|---|---|---|---|---|---|
| `check-recursive-locks.py::strip_noise` | comments **+ literals** | ✅ | ✅ | ✅ | ✅ |
| `check-tick-wiring.py::strip_comments` | comments **+ literals** | ✅ | ❌ | ✅ | ❌ |
| `check-variant-lists.py::strip_comments` | comments only | ❌ | ❌ | ❌ | ❌ |
| `check-option-refusal.py::strip_comments` | comments only | ✅ | ❌ | ❌ | ❌ |
| `check-usage-status.py::strip_comments` | comments only | ✅ | ❌ | ❌ | ❌ |

So the shared implementation to converge on is **not**
`scripts/rust_scopes.py::_strip` as this entry first proposed — it is
`check-recursive-locks.py::strip_noise`, which is already the most complete of
the five (nested block comments, raw strings, char literals, offsets exact,
and a self-test that includes `apostrophe in block comment`). Three checkers —
`check-selftest-skips.py`, `check-vfs-permission-gate.py`,
`check-vfs-under-lock.py` — already import it by `importlib` (the filename's
hyphens make it un-`import`able normally), so the borrowing convention exists
and works.

It cannot be used *as-is* by the two detectors above for the same reason
`rust_scopes._strip` could not: it blanks string literals too, and these
detectors match *on* string literals (`"Usage: …"`, `"-x"`,
`.starts_with('-')`). The fix is one parameter — `strip_noise(src,
keep_literals=False)` — plus a self-test case for the new mode, then delete
the other four copies.

### Finding 2 — the brace counters read comments as code, which fails silently

This is the more serious half, and it is not the same shape as the bug fixed
above. `check-query-status.py` — which its own docstring calls the mirror of
`check-usage-status.py` — locates a block's start and end by **counting
braces** over lines that have had *strings* removed but **not comments**:
`strip_strings` there is one line (`"".join(s.split('"')[::2])`) and is applied
to raw `lines[k]` at `:134`, `:149` and `:196`, scanning a ±400-line window.
Two further reads on the same loop are not filtered at all — `PRINT.search(
lines[k])` at `:198` will collect a `shell_println!` written *inside a comment*
as one of the block's answers, and the hit guard at `:178`
(`ln.lstrip().startswith("//")`) only recognises a comment that occupies the
whole line, not a trailing one.

`check-usage-status.py` had the identical defect and **no longer does** — its
depth line reads `strip_strings(s)` where `s` is already
`strip_comments(lines[k])`, so the fix recorded above closed its brace counter
too, as a side effect rather than by intent. That is worth stating plainly:
the mirror pair was written to stay in step, one of the two drifted out of it
silently, and nothing in either script would have reported that.

`kernel/src/kshell.rs` currently contains **26 comments holding an unbalanced
brace**, which is precisely the thing a comment is allowed to contain:

```rust
i = i.saturating_add(1);  // skip `{`
i = i.saturating_add(1);  // skip `}`
/// Brace nesting depth.  Starts at 1 (the opening `{`).
// Don't include the final `}` line in the body.
```

Any one of these falling inside a scan window shifts the computed block
boundary, so the checker then examines *the wrong range of lines* — which can
both miss a real finding and manufacture a false one, with no diagnostic
either way. Two of the five lexers above already wrote this hazard down
independently, in almost the same words — `check-variant-lists.py`: *"Done
before any brace counting, because a comment is exactly the place an
unbalanced brace or bracket is allowed to appear"*; `check-tick-wiring.py`:
*"a `'{'` … in the source would otherwise throw the match off and swallow the
rest of the file"*. The two checkers that most need the warning are the two
that never received it.

**Not yet proven to be biting.** The 26 comments are real and the scan is
real, but no current finding has been traced to a window that contains one.
That is a reason to fix it, not a reason to wait: the failure mode is a gate
silently reading the wrong lines.

**Why none of it was done in the same change.** A boot test was running
against `scripts/` at the time, and editing a gate script mid-run invalidates
it. The line-local version closes the case that actually occurred; this closes
the class.

**Severity.** Low-to-moderate. Finding 1 narrows a gate, and a gate that is
too narrow fails open. Finding 2 lets a gate answer about lines it was not
asked about, which is worse than failing open because the answer still looks
authoritative.
