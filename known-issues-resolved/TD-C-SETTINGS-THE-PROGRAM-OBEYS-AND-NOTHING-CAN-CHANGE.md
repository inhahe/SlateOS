## `TD-C-SETTINGS-THE-PROGRAM-OBEYS-AND-NOTHING-CAN-CHANGE` (lane C, 2026-09-18) -- **CLOSED 2026-09-22**

> **Closed: every setting listed below has a writer now.** The two the entry
> left open were done without being recorded here -- `pdfviewer`'s `dark_mode`
> (a reading-mode toggle, so a document can be shown in the colours it was
> written in) and `calendar`'s `week_starts_monday` (`W`, which its shortcut
> card already advertised). `scripts/frozen-flag-survey.py` reports nothing
> outstanding in the app scan.
>
> **A note on how nearly I got this wrong.** I checked `calendar` with
> `grep -n week_starts_monday | head -6`, read six lines that were all
> declaration and reads, and concluded it was still frozen. The writer is on
> line 2988 and there are fourteen matches: `head` cut the evidence three
> lines before it. That is the same shape as running the wrong crate and
> believing the green -- a measurement that was true about what it measured
> and silent about what I asked. **A truncated search answers a different
> question from the one typed**, and the tell is the same in both cases: a
> suspiciously tidy result for something that should be messier.

**In short:** A number of our apps have a setting that the code genuinely
honours -- it gates a draw, or picks a sort, or decides what characters a
password may contain -- and that nothing anywhere can change. The value it was
given at construction is the only value it will ever have. From inside the code
it looks like a finished feature, because every part of it *is* finished except
the one that lets a person use it.

**This is the exact mirror of pre-push gate 50 (`write-only-fields`)**, and the
worse half of the pair. A write-only field is state nobody reads: dead weight,
no user ever affected. A **frozen** field is read and obeyed, so the feature is
live, visible and stuck -- and usually *displayed*, which turns it from a
missing feature into a false offer.

**Verified and fixed.**

| App | Frozen | What it meant |
|---|---|---|
| `rssreader` | `sidebar_selection` | no feed or folder could ever be selected; the article filter's `Feed`, `Folder` and `Starred` arms were unreachable |
| `rssreader` | `is_expanded` | no folder could be collapsed; the "closed" indicator could not be drawn |
| `rssreader` | `sidebar_visible` | the sidebar could not be hidden |
| `rssreader` | `sort_order` | the window displayed "Sort: Date (newest first)" -- a label that could not say anything else -- over a `sort_by` that worked perfectly |
| `passwordgen` | `use_lowercase`, `use_uppercase`, `use_digits`, `use_symbols`, `exclude_ambiguous` | **`length` was the only field of `password_opts` with a writer.** The options panel drew all five as "Yes"/"No" and no key could change one |

**`passwordgen` is the one to look at**, because the consequence is not
cosmetic. Every password it produced contained symbols. Sites that forbid
symbols are common, so for those the generator was simply unusable, and the
user could see an option called "Symbols: Yes" that they could not act on.
Ambiguous characters (`l` and `1`, `O` and `0`) could never be excluded either.
Both are now keys, and turning off the last remaining class is refused --
`generate_password` answers an empty pool with an empty string, so a generator
with nothing selected would have produced nothing at all, silently.

**Four more read and confirmed, not yet fixed.** Each is obeyed by real logic
and has no writer anywhere in production:

| App | Frozen | Stuck at | What it means |
|---|---|---|---|
| `pdfviewer` | `dark_mode` | **`true`** | `page_color()` returns `rgb(40,42,54)` for every page, and `text_color` inverts with it. **Every document renders in inverted colours and no key restores the white page** -- though the comment beside it calls this "the viewer's own `dark_mode` for reading", which is a thing you would switch |
| `calendar` | `week_starts_monday` | `true` | every month grid begins on Monday, for everyone, forever |
| `hexeditor` | `case_sensitive` | `true` | search was always case-sensitive; there was no case-insensitive search in the program. **Fixed 2026-09-18:** `Ctrl+I` in the search bar toggles it and the bar says which way it is set, because a search that silently ignores case -- or silently insists on it -- turns a miss into "it is not in the file", which is a claim about the file. 198 tests |
| `imageviewer` | `show_toolbar` | `true` | the toolbar could not be hidden, including when looking at an image. **Fixed 2026-09-18** (`B`), with `show_status_bar` on `S` |

`pdfviewer` is the one that matters most: a document reader that cannot show a
document in the colours it was written in.

**`explorer`, found only by the whole-crate re-read (2026-09-18, fixed).** The
file manager has three view modes -- `Details`, `List`, `Icons` -- and
`set_view_mode` was the only writer of `view_mode` and had no caller, so the
window was permanently in `Details`. The other two are not stubs: they are
obeyed by the layout, the navigation step size, the header and the item
renderer, and could never be seen. `1`/`2`/`3` select them now, with a test
that the *drawing* differs and not merely the field.

This one is the argument for re-reading whole crates: `explorer` has **eight**
source files, and every sweep before this read `main.rs` alone.

**`metronome`, and the strongest form of the signal.** Its practice panel
draws three lines:

```
Practice Target: 140 BPM (up/down to adjust)
Practice Increment: +10 BPM
Practice Measures: 4
```

The first is adjustable and says so. `practice_increment` and
`practice_measures` **had no writers**, so practice mode always sped up by ten
every four measures. **Fixed 2026-09-18:** in the settings panel, `Left` and
`Right` move the increment and a digit names the measure count outright --
stepping to nine with an arrow is eight keypresses for a number the user
already knows. Both labels now name their keys, as the target line already
did. 74 tests, up from 69. The line above them advertising its own keys is what
makes the other two read as settings rather than as a description -- they are
laid out as a group, and one third of the group works.

**`slides` has the same shape and is filed separately**
(`TD-C-SLIDES-CAN-ADD-A-TEXTBOX-AND-NOTHING-ELSE`): "Theme:" once and
"Transition:" twice, none of them changeable.

**A third probe, and the limit of all three.** Looking for a *displayed* value
with no writer -- a field read inside a `format!` or a `text:` and assigned
nowhere -- gives 173 hits across 65 apps, and it is the best signal of the
three because a value on screen is a claim to the user. **It is still only a
lead generator**, for a reason worth writing down: it reads `self.field` and
cannot see which `self` it is in, so every field of every helper struct in the
file is folded in with the app's own. That is why `calendar` appears to have a
frozen `day` and `month` (they belong to a date), and it is the same blindness
that made the *first* probe miss `passwordgen`: a field written as
`self.password_opts.use_symbols` is not matched by a search for
`.use_symbols =` in one direction, and a field replaced wholesale as
`self.time_signature = sig` looks frozen from the other.

**Refined, and then measured.** Restricting the probe to the fields of the
*application* struct -- rather than every `self` in the file -- cuts 173 hits
across 65 apps to **40 across 22**, and it independently reproduced
`metronome`'s two, which had been found by hand. That is the good news. The
bad news is what reading the 40 showed:

| App | Field | Verdict |
|---|---|---|
| `videoplayer` | `volume` | **false positive.** `self.volume.increase(5)`, `.decrease(5)`, `.toggle_mute()` -- mutated through its own methods, which the probe does not count as writes |
| `editor` | `font_size` | **false positive as a *label*.** It is genuinely frozen at 14.0, but it is never *shown*: the probe matched `tree.text(x, y, s, c, self.font_size)`, a call that draws text *at* that size rather than a label that displays the number |

**So the probe has three blind spots and only one of them is fixed.** It now
knows which struct a field belongs to. It still cannot see a field mutated
through its own methods, and its test for "displayed" is the substring `text:`,
which matches any drawing call that happens to take the field as an argument.
**Both remaining blind spots produce false positives, which is the dangerous
direction** -- a probe that under-reports wastes an afternoon, while one that
over-reports gets working programs filed as broken. Of the 40, the two read so
far were both wrong.

**A fourth probe, built after the authoring finds, and it is the clearest
result of the four.** The question that found `notes` -- *can the user produce
the thing the app is a list of?* -- was mechanised as "app-struct `Vec` fields
with no live `push`". It returns **92 collections across 48 apps**, and the
three most promising rows were all false positives on inspection:
`launcher`'s `apps` comes from `builtin_app_database()`, `dictionary`'s
`entries` from `build_dictionary()`, `colorpicker`'s `palettes` from
`vec![default_palette]` -- **assigned wholesale, which a search for `.push`
cannot see.** Most of the rest are derived lists (`legal_moves_for_selected`,
`cached_blocks`, `treemap_rects`) that no user is supposed to produce.

**The question is good and the mechanisation is not**, which is the whole
lesson in one line. `notes`, `slides` and `diagram` were found by asking what
three programs are *for* -- a sentence each, written by hand -- and no query
written afterwards reproduces that. **Stop building these; read the app.**

**The conclusion for anyone picking this up:** the probes in this entry are
worth running once, as a way of choosing what to read. **They are not worth
believing.** Every defect recorded here was confirmed by reading the code, and
in four separate cases today a probe's hit dissolved on contact with it --
`passwordgen` (real, but found only after a probe missed it), `metronome`'s
time signature (replaced wholesale), `slides`' `next_slide` (a redundant
duplicate of working code), and `videoplayer`'s volume (mutated by method).

**`metronome` is the worked example of the second failure.** Its
`beats_per_measure` and `beat_value` have no writers, which reads as a
metronome with a fixed time signature -- the one thing a metronome must be able
to change. They are fields of a `TimeSignature` struct that `set_time_signature`
replaces whole, on the `T` key, from nine predefined signatures. **The feature
works; only the field is still.** Third time today that a sub-field or a
spelling nearly turned a working program into a filed defect, which is the
argument for the rule these entries keep restating: **a probe finds candidates,
and only reading the code finds defects.**

**The last five read, and all five are safe.** Each is a preference with no
writer, and each is frozen at the value you would have chosen anyway:

| App | Frozen at | Consequence |
|---|---|---|
| `markdowneditor` `autosave_enabled` | `true` | autosave is always on; it cannot be turned off, but nothing is lost by that -- **it could be turned off after all, for the window only; since 2026-09-27 (lane E, C-Q26) the choice is kept in `markdowneditor.yaml`** |
| `diskimager` `verify_after_write` | `true` | images are always verified; the window draws it as a checkbox that cannot be unchecked |
| `imageviewer` `show_status_bar` | `true` | **fixed** -- `S` |
| `spreadsheet` `show_toolbar` | `true` | **fixed** -- and it had to be `Ctrl+T`, because this handler's catch-all starts editing the cell on any printable character, so a bare `T` would have stopped being typeable into a spreadsheet. A fix that breaks typing is worse than the panel it frees |
| `mindmap` `show_sidebar` | `true` | **fixed** -- a plain `B`, safe here because the editing and search modes return before the main match |

**So these are false offers, not hazards**, and that distinction is worth
keeping: had `verify_after_write` been frozen at `false`, a tool that writes
disk images would silently never verify one while showing a box implying it
had. The defaults being right is luck as much as design -- nothing in the code
records that the frozen value is the safe one, because nothing in the code
knows it is frozen.

**Remaining candidates, unread.** The probe over `apps/*/src/main.rs` for `pub`
`bool` fields with no assignment, no `&mut` and at least one read found 157
across 47 apps. **Most are not defects** -- `is_dir` on a directory entry is
supposed to be fixed at construction, and the probe cannot tell a property from
a preference. Still worth reading: `imageviewer` `show_status_bar` ·
`spreadsheet` `show_toolbar` · `mindmap` `show_sidebar` · `markdowneditor`
`autosave_enabled` · `diskimager` `verify_after_write` · `systemrestore`
`enabled` (it sits in `ScheduleConfig` and means "whether scheduling is on",
so it is a preference and not a property).

**`fontmanager` `system` is the worked example of a false positive**, and worth
keeping here so the next person does not re-file it: it is documented as
"whether this is a system font (cannot be uninstalled)". It is a fact about a
font, it is supposed to be fixed at construction, and a writer for it would be
the bug. The probe cannot see that; only the doc comment and the name can.

**The discriminator to apply to each** is not "does it have a writer" but two
questions in order: *is this a preference or a property of the thing it sits
on?*, and if a preference, *is it displayed?* A displayed preference that
cannot be changed is the false-offer case and should be fixed or removed. An
undisplayed one is a smaller matter. `apps/paint`'s `should_quit` is neither:
it has no writer **and** no reader, so it is an ordinary write-only field --
and it cannot be fixed, because the framework gives an app no way to close its
own window.

**Why a gate is not proposed here.** Gate 50 can be mechanical because
"written and never read" is decidable from the text. "Frozen" is not: the probe
cannot distinguish `is_dir` from `dark_mode`, and a gate with a 157-entry
baseline of mostly-correct code teaches people to add to the baseline. The
check that works is the per-app one `rssreader` now carries --
`every_advertised_shortcut_does_something` -- which asserts that what the UI
offers, the program answers. That is worth copying to any app with a settings
panel: **draw the option, then assert something can change it.**
