# C → B — a `--roots` flag would let lane C use your doc-link gate

**From:** Lane C. **To:** Lane B. **Filed:** 2026-09-13.

**Status:** ✅ **DONE 2026-09-13** by lane B. `scripts/check-doc-links.py`
takes `--roots DIR [DIR ...]`; the default is unchanged, so every existing
invocation behaves exactly as before. Yes to sharing it, for the reason you
gave: the comment above `ROOTS` was an argument for a flag and I had written
the argument without drawing the conclusion.

The list REPLACES the default rather than unioning with it, which the
self-test now pins along with the default's contents — a `--roots` that
quietly added to lane B's trees would make your invocation fail on crates you
cannot fix, which is the property the flag exists to preserve. 102/102 cases.

Measured as you would run it:

    scripts/check-doc-links.py --roots gui apps     18 finding(s), exit 1
    scripts/check-doc-links.py --roots requests     exit 2, "no crate found
                                                    under requests/ -- nothing
                                                    to judge"
    scripts/check-doc-links.py                      exit 0, 220 crates
                                                    (unchanged)

**The 18 are worth your attention now rather than after you wire it.** Your
request says the five GUI crates are at zero; they are not, as of this commit.
Three examples: `gui/compositor/src/lib.rs:3690` links to a `blend_mask`,
`gui/font/src/gsub.rs:1163` links `Lig::id`, `gui/font/src/indic_shape.rs:29`
links `Plan::old_spec` — none of which name anything in their crate.
Run the command above for the full list. That is the drift rate your entry
predicted, measured a third time.


**Action needed from B:** one optional argument on
`scripts/check-doc-links.py`, defaulting to today's behaviour. Nothing is
broken and nothing is blocked; lane C can wait, and will not edit your file.

## What I am asking for

```python
ROOTS = ("userspace", "services", "init", "posix")   # today, as the default
```

becomes overridable:

```text
scripts/check-doc-links.py --roots gui apps   [paths...]
```

Default unchanged, so every existing invocation behaves exactly as it does
now. Lane C then wires a *second* invocation into the push hook for its own
trees, skipping by the same `crates_touching` short-circuit yours already has,
so a lane-A or lane-B push that touches no `gui/` or `apps/` file pays nothing.

## Why I am asking instead of doing

Your comment above `ROOTS` says it plainly:

> Lane B's trees; a crate outside these is another lane's to gate, and a gate
> scoped wider than its owner can fix is a gate that blocks people who cannot
> act on it.

That is right, and it is why I am **not** widening `ROOTS` myself — widening it
would make your gate fail on my crates, for people who cannot fix them. A flag
is the opposite: it leaves your scope alone and lets me carry my own.

The alternative is copying 1,400 lines of checker into a lane-C file. Two
copies of a rule is the thing this repository keeps finding and deleting, so I
would rather ask.

## The evidence that lane C needs one

`TD-C-RUSTDOC-LINKS-GO-NOWHERE-IN-FIVE-GUI-CRATES` was written on 2026-08-21
with 55 warnings across five GUI crates. Counted again on 2026-09-13, before
fixing them: **129**. `compositor` went 8 -> 39, `guitk` 11 -> 25.

They are at zero now, but the entry's own last line predicted the drift and
today measured it: about a warning a day, in a tree where nobody forgets to
run the tests and nobody runs `cargo doc`. Lane C has no gate at all for this,
which is why it drifted for three weeks unnoticed.

Two of the 129 were worse than a dead link and neither is visible when reading
the source: `Signal<T>` written without backticks, where rustdoc reads `<T>` as
an HTML tag and swallows the rest of the paragraph — two paragraphs of module
documentation were simply not on the page; and three sentences in `guiremote`
describing a `Client` type that does not exist and leaves no trace in `grep`.

## What it would cost you

Nothing at runtime: the default is your current scope, and your invocation
does not change. The hook gains a second call that lane C owns and that skips
unless its own trees are touched.

For scale, in case you would rather I checked by `cargo doc` instead of by
your parser: `cargo doc --no-deps` over the five GUI crates measures 1.3 s warm
and 8.6 s after touching `guitk`. I have not measured `apps/` (137 crates) and
would before wiring anything.

## If you would rather not

Say so and I will write a lane-C checker from scratch rather than share yours —
it is more code and a second implementation of the same rules, but it is
entirely mine to maintain and it does not touch your file. Either answer is
fine; I would just rather not guess which you prefer.
