## §480 — A test that only ever uses the input that works proves the code works on that input

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the emoji picker stapled the chosen skin tone onto every emoji,
so picking "dark" and clicking a pizza put a pizza followed by a small brown
square on the clipboard. Seventy-five of its eighty-two emoji were affected and
five tests covered the feature, all of them tinting the same thumbs-up — one of
the seven emoji for which the code was right. The fix is a Unicode property
table. The decisions worth recording are *where* the property comes from, and a
testing rule about inputs that cannot distinguish two implementations.

### Where "does this emoji have skin?" is allowed to come from

Two places could answer it, and the choice is not obvious:

| | Answer comes from | Cost |
|---|---|---|
| A flag on each `EmojiEntry` | whoever adds the entry | must be re-judged per entry, and is wrong *silently* |
| A codepoint table | Unicode `emoji-data.txt` | 39 ranges of literal data to carry |

Chosen: **the table**. The deciding argument is not that the table is more
accurate — a careful author would get the flags right — but that the flag's
failure mode is invisible. Adding "🧑‍🚀 astronaut" with `skin: false` produces
no error, no warning and no wrong pixel until somebody picks a tone; the entry
just quietly refuses to tint. The table is answerable to a published document,
so `the_modifier_base_table_agrees_with_unicode` can check it against
hand-verified codepoints and the check means something.

The counter-argument is real: the table is 39 lines of magic numbers that no
reader can verify by eye, and it will go stale as Unicode adds bases. Accepted,
because staleness in the table is *uniform and datable* ("this is Unicode
15.1") whereas staleness in per-entry flags is scattered and undatable. When it
needs updating, it is one transcription against one file.

The surprising part is worth writing down for whoever reads the table next:
**`Emoji_Modifier_Base` is much narrower than "looks like a person".** 😀 is
not a modifier base. Neither is 😂 or 😎. Faces are yellow *as a design
choice*, not as a default skin tone, and Unicode does not let you tint them.

### Where the modifier goes, and what the grid shows

**The modifier goes directly after the base it tints, not at the end of the
sequence,** and a variation selector caught between the two is dropped (a
modifier already forces emoji presentation, so `base FE0F modifier` is not a
well-formed `emoji_modifier_sequence`). This distinction is invisible in the
shipped database — every tone-taking entry there is one character long — but it
is the difference between a correct rule and one that happens to agree with the
correct rule on today's data.

**The grid draws the tinted emoji, not the plain one.** Previously the tone
showed up in the preview and on the clipboard but not in the grid, so the
surface you look at while choosing was the one surface that did not show the
choice. This is §478's instinct pointed the other way: there, the UI asserted
something the state did not know; here, the UI declined to show something the
state did know.

### Testing note — the rule this one adds

**An input set can be large, realistic, and still incapable of telling two
implementations apart.** Mutating "put the modifier after the base" into "put
it at the end" survived the first mutation run against a sweep of all
eighty-two database entries, because in every one of them the two produce the
same string. More data does not help; only *differently shaped* data does. The
fix is one constructed input — a ZWJ sequence — asserted by hand.

Stated generally: **a rule about ordering within a sequence cannot be tested by
data that is one element long, however much of that data there is.** The
companion to §479's "measure against something with no shared origin": that
rule is about where the expectation comes from, this one is about whether the
input can express the difference at all.

The other half is §479's lesson arriving from a new direction. There, a control
was drawn but never hit-tested. Here, a function was tested five times over on
the one input for which it was correct. Both are coverage that counts the
gesture rather than the case: five skin-tone tests is a plausible-looking
number, and all five were the same test.
