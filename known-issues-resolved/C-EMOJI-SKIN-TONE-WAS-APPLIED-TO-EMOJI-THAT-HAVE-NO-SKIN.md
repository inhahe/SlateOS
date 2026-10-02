## `C-EMOJI-SKIN-TONE-WAS-APPLIED-TO-EMOJI-THAT-HAVE-NO-SKIN` (lane C, 2026-08-20) — FIXED

**In short:** the emoji picker has a row of six skin-tone swatches along the
bottom. Choosing one used to change *every* emoji in the picker, not just the
people. Pick "dark", click the pizza, and what landed on your clipboard was a
pizza followed by a small brown square — because a skin-tone modifier that
follows something without skin is not absorbed into the glyph; it draws on its
own. Seventy-five of the picker's eighty-two emoji were affected. Now the tone
applies only to the emoji Unicode says can wear one, and the grid shows the
tone as you have set it instead of only revealing it after you have clicked.

**Where:** `apps/emojipicker/src/main.rs` — `SkinToneModifier::apply`, and the
glyph drawn by `render_grid`.

**What was wrong.** `apply` was a two-line append:

```rust
let mut result = String::with_capacity(base_emoji.len().saturating_add(4));
result.push_str(base_emoji);
result.push(ch);
```

It asked nothing about what it was appending to. Unicode has a property for
exactly this question — `Emoji_Modifier_Base` — and the set is far narrower
than "looks like a person": a raised hand and a dancer are in it, but a
grinning face is **not**, and neither is anything in the other seven
categories. Of the eighty-two entries in the shipped database, seven are
modifier bases.

The defect was invisible to the test suite because all five existing skin-tone
tests tinted `"\u{1F44D}"` (thumbs up), which is one of the seven. Nothing ever
passed `apply` an emoji it would break.

A second, quieter half: `render_grid` drew `entry.emoji` raw while
`render_preview` and the click path both drew `skin_tone.apply(...)`. The grid —
the thing you are looking at when you choose — was the one surface that did not
show the choice.

### Fixed 2026-08-20

**`EMOJI_MODIFIER_BASE`**, a sorted, disjoint table of the 39 codepoint ranges
carrying the Unicode 15.1 `Emoji_Modifier_Base` property, with
`is_emoji_modifier_base(char)` deciding membership by binary search and
`takes_skin_tone(&str)` asking it about an emoji's first scalar. A table read
off `emoji-data.txt` rather than a hand-set flag on each database entry: the
flag would have to be re-judged on every entry added, and would be wrong
silently.

**`apply` returns the emoji unchanged when it has no skin**, and when it does
have skin puts the modifier **directly after the base** rather than at the end
of the sequence, dropping a variation selector caught between the two (a
modifier already forces emoji presentation, so `base FE0F modifier` is not a
well-formed `emoji_modifier_sequence` — UTS #51).

**The grid draws `skin_tone.apply(...)` too**, so the tone is visible where it
is chosen rather than only in the preview and the clipboard.

**Recently-used still records the untinted emoji**, which was already true and
is now pinned by a test: it means changing the tone re-tints the recent list
instead of leaving rows frozen at whatever tone was active when each was
picked.

### Known limit (deliberate, documented at `takes_skin_tone`)

A ZWJ sequence can carry a modifier base in a later segment — "man
technologist" is a man joined to a laptop — and toning one properly means
toning *each* base segment. The current rule tones only the first. Every ZWJ
entry in this database is flag-shaped and takes no tone at all, so the
distinction does not bite today; **adding a person-shaped ZWJ sequence to the
database is what should trigger generalising it.** The behaviour is pinned by
`the_modifier_goes_after_the_base_it_tints_not_at_the_end`, which asserts the
man is tinted and the laptop is not.

### Testing

Eight new tests (62 green, clippy clean, rustfmt clean). Ten mutations, all ten
caught. Two are worth recording:

- **The whole database is swept, split by the property, and both halves are
  asserted to be non-empty.** A test that tinted one hand-picked emoji is what
  let this bug live for the life of the file; a test that sweeps but happens to
  run on a database with nothing in one half would repeat the mistake
  silently, so `entries_by_skin` fails loudly rather than passing vacuously.
- **Where the modifier goes could not be tested against the shipped
  database at all.** Every tone-taking entry in it is a single character, so
  "after the base" and "at the end of the sequence" produce the same string,
  and that mutation escaped the first run. It took a *constructed* multi-scalar
  input to separate them. General form: **a rule about ordering within a
  sequence cannot be tested by data that is one element long**, however much of
  it there is.

See `design-decisions.md` §480.
