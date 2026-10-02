## TD-C-RUSTDOC-LINKS-GO-NOWHERE-IN-FIVE-GUI-CRATES -- FIXED 2026-09-13

**Fixed 2026-09-13: all five crates report zero.** And the number worth
keeping is not 129, it is the rate.

| | warnings |
|---|---|
| counted 2026-08-21, when this entry was written | 55 |
| counted 2026-09-13, before the fix | 129 |
| after | 0 |

`compositor` went from 8 to 39 and `guitk` from 11 to 25 in three weeks, in a
codebase where nobody forgets to run the tests and nobody runs `cargo doc`.
This entry predicted it in its own last line -- *a warning count nobody
watches is one nobody will ever bring to zero* -- and the prediction is now
measured rather than asserted: about a warning a day.

**Two classes were worse than a broken link**, and both are invisible to a
reader of the source: `Signal<T>` and `Vec<StyledSpan>` written without
backticks, where rustdoc reads `<T>` as an HTML tag and swallows the rest of
the paragraph -- two paragraphs of module documentation were simply not on
the page; and three sentences in `guiremote` describing a `Client` type that
does not exist and leaves no trace in `grep`.

**Why there is no gate yet, and what the gap actually is.** The obvious move
is a `cargo doc` check in the push hook. It is affordable -- measured at 1.3 s
when nothing changed and 8.6 s after touching `guitk`, against a hook that
already runs seventy-one checkers and compiles sixty crates for one of them.
But a gate for this already exists: `scripts/check-doc-links.py`, written
2026-09-01 after the same discovery in `coreutils`. It does not cover these
crates **on purpose**:

```python
# Roots scanned. Lane B's trees; a crate outside these is another lane's to
# gate, and a gate scoped wider than its owner can fix is a gate that blocks
# people who cannot act on it.
ROOTS = ("userspace", "services", "init", "posix")
```

So widening it is the wrong fix and lane C simply has no equivalent. The right
one is a sibling scoped to `gui/**` and `apps/**` that skips when no file of
theirs is in the push -- the short-circuit that script already implements --
and it has to be wired into whatever `scripts/check-gates-are-wired.py`
watches, or it is a gate nobody asks. That is the next piece of work and it is
logged as its own entry rather than left in this one.

**In short:** The GUI crates' generated documentation has 55 broken or
misleading cross-references. Some are links that point at nothing at all, so a
reader clicking "see `export_text`" lands on an error page; some point at
internals a reader of the public docs cannot see; and two in the toolkit are
unclosed HTML tags, which make rustdoc swallow the rest of that paragraph. None
of it affects behaviour — this is documentation quality only.

**Where:** counted 2026-08-21 with `cargo doc -p <crate> --no-deps
--target x86_64-pc-windows-gnu`, one warning per problem:

| Crate | Warnings | Worst class |
|---|---|---|
| `osfont` (`gui/font`) | 18 | 14 links to private items, 3 unresolved |
| `desktop` (`gui/desktop`) | 11 | 8 links to private items, 2 unresolved |
| `guitk` (`gui/toolkit`) | 11 | 2 **unclosed HTML tags** — these eat text |
| `compositor` | 8 | 3 unresolved, 3 private |
| `guiremote` (`gui/remote`) | 7 | 3 private, 1 unresolved |
| `oswindow`, `appearance` | 0 | — |

**The three kinds, worst first:**

1. **`unresolved link to X` (9 total).** The link target does not exist —
   usually a method that was renamed or removed and the doc comment beside it
   was not. `gui/desktop/src/calendar.rs:547` promises "the text format produced
   by `export_text`"; there is no `export_text`. This is the only class that is
   actively *wrong* rather than merely unhelpful, because it documents an API
   that is not there.
2. **`unclosed HTML tag` (2, both `guitk`).** Rustdoc treats `<` as markup, so
   an unescaped one in prose opens a tag that never closes and the remainder of
   the paragraph is absorbed into it and never rendered. The text exists in the
   source and is invisible in the docs, which is the failure mode hardest to
   notice.
3. **`links to private item` (34).** A public doc comment links to something a
   reader of the public documentation cannot follow. Not wrong, but the link is
   dead for its audience.

**The proper fix** is per-warning and mechanical: repoint or delete the
unresolved links (checking which is right — an unresolved link often means the
sentence around it is also stale), escape or close the two HTML tags, and for
the private-item links either make the target public, drop the link and name the
item in plain text, or move the sentence to a `//` comment where it is not
promising a reader a hyperlink. There is no design decision here.

**If never fixed:** the docs stay quietly worse than the code, and the noise
floor hides the next real one. Three of these are already *stale claims* about
APIs that do not exist, which is how a reader ends up writing against a function
that was deleted. It does not get worse on its own, but every new crate adds to
it, and a warning count nobody watches is one nobody will ever bring to zero.
