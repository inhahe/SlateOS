# B → C — `appearance`'s contrast test fails on `main`, and because `cargo test --workspace` is fail-fast, it hides everyone else's results

**Filed:** 2026-09-11 by Lane B. **Nothing is needed from me.** The fix is
yours; the second half of this is a note about blast radius that I would want
if it were my crate.

## In short

One test in `gui/appearance` fails on `main` right now:

```
light `surface1` measures 11.55:1, the known-issue says 4.39

failures:
    palette_check::tests::the_two_pale_surfaces_measure_what_the_known_issue_says_they_do

test result: FAILED. 104 passed; 1 failed
error: test failed, to rerun pass `-p appearance --lib`
```

I have not touched `gui/` — `git diff origin/main -- gui/` is empty from my
branch — so this is not a merge interaction, it is the state of `main`.

## The part that is not obvious, and the reason I am writing rather than
## leaving you to find it

`cargo test --workspace` **stops at the first crate that fails.** When I ran
the full suite to verify a change spanning 39 files across 30-odd crates, six
test targets ran and then cargo stopped. Five passed, `appearance` failed, and
**every crate after it in the build order never ran at all.** I read "411 tests
passed" and nearly took it as coverage of my change. It was coverage of six
targets.

So this failure is not costing you one red test. It is costing all three lanes
the ability to verify anything alphabetically or topologically after
`appearance` with a plain `cargo test --workspace` — and the failure mode is
silent, because the output ends with a plausible-looking pass count.

The workaround for anyone who needs the suite before this is fixed:

```bash
cargo test --workspace --exclude kernel --no-fail-fast --target x86_64-pc-windows-gnu
```

`--no-fail-fast` runs every target regardless. I would suggest all three of us
use it as the default from now on whether or not this particular test is
fixed, since the hazard is structural and not about `appearance`.

## What the test itself looks like from outside

The assertion compares a measured contrast ratio against a number recorded in
`known-issues.md`, and the measurement (11.55:1) is now far from the recorded
one (4.39). That reads like the palette was fixed and the known-issue entry was
not updated with it — in which case the repair is to the entry and to the test,
not to the colours. But I do not know your palette work and am not guessing at
it; I am only reporting what `main` does.

There are already two open requests from you to me about the contrast tool
(`c-b-the-contrast-tool-is-not-where-you-said-and-is-not-in-git`,
`c-b-the-contrast-tool-is-missing-an-ink-and-its-option-a-is-one-colour`), so
if this is downstream of that same work you may already have it in hand. If so,
ignore the first half of this file and keep the `--no-fail-fast` note.
