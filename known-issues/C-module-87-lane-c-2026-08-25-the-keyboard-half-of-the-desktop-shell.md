## MODULE 87 (lane C, 2026-08-25) -- the keyboard half of the desktop shell

**In short:** module 83 swept `gui/desktop/src/lib.rs`'s *pointer* handling and
left the keyboard alone, so the chord table in `DesktopAction::for_chord`, the
Alt-Tab switcher's four stepping methods, `run_desktop_action`'s arms and the
virtual-desktop bounds had never been asked a question. Fifty-three of the
file's tests had never been named by any defect. Twenty-six deliberate faults,
twenty-two caught. Three of the four escapes are one shape -- **the recovery
code is unreachable from every fixture, because no fixture ever builds the state
it recovers from** -- and one of those three had a test named for exactly the
property it failed to prove. The fourth escape is a different and worse thing:
a test that copies the production expression into its own body.

**The pass, 26 defects:**

```
26 defects: 22 caught, 4 escaped, 0 never asked, 0 under-caught,
14 under-declared
```

Caught: the one-window switcher guard, the opening index, the stepping direction
both ways, the release path's outcome, Shift+Alt+Tab's binding, Alt+F4's, Super+D's,
Super+Right's and Super+Down's, the historic loose-chord regression on both
arrows (the `Super+Right` binding that stopped naming Ctrl and swallowed
`Ctrl+Super+Right` again), the desktop-switch direction, Escape's binding and its
arm, Super+D's desktop filter, the maximised-window test, the focused-window
`?`, the taskbar's desktop filter and its sort order, and the off-by-one past
the last virtual desktop.

### Lesson 41: a test named for a hazard is not evidence the hazard was built

Two escapes, `E` and `G`, are both in code that exists only to survive a state
the ordinary path does not produce -- windows closing while the Alt-Tab switcher
is open, leaving its index past the end of a list that is recomputed on every
step.

`E` removes the clamp from `prev_alt_tab`:

```rust
-            self.alt_tab_index = step::wrapping_before(count, self.alt_tab_index.min(last));
+            self.alt_tab_index = step::wrapping_before(count, self.alt_tab_index);
```

The suite already had a test named for this, written when the clamp was added:

```rust
    /// Stepping backwards from a stale index must land inside the list, not on
    /// another index past the end.
    #[test]
    fn stepping_backwards_survives_the_windows_closing_underneath_it() {
        let ids = ...four windows...;
        shell.start_alt_tab();
        shell.next_alt_tab();
        shell.next_alt_tab();
        for id in &ids[1..] { close(&mut shell, *id); }
        shell.prev_alt_tab();
        assert!(shell.alt_tab_index < shell.taskbar_windows().len());
    }
```

The assertion is right, the name is right, and the test is green with the clamp
deleted. `start_alt_tab` on four windows opens on index 2; two `next_alt_tab`
calls go 3, then **wrap to 0**. So by the time the windows close, the index is
already 0 -- and 0 is in range for every non-empty list, so `wrapping_before`
gives the same answer clamped or not. The fixture never produced a stale index
at all. It is a test about recovering from a hazard that never constructs the
hazard.

`G` is the same failure in the same subsystem. It moves `finish_alt_tab`'s
`self.alt_tab_active = false` below the `?` that finds the window:

```rust
-        self.alt_tab_active = false;
-        let id = self.taskbar_windows().get(self.alt_tab_index)?.id;
+        let id = self.taskbar_windows().get(self.alt_tab_index)?.id;
+        self.alt_tab_active = false;
```

Now a switcher whose window closed under it never closes -- the `?` returns
before the flag is cleared, and the Alt release that would have ended it has
already been spent, so the overlay sits over every window for the rest of the
session. `alt_tab_survives_the_windows_closing_underneath_it` reaches
`finish_alt_tab` through `next_alt_tab`, which wraps and therefore always lands
in range, so the `?` never fires and the two orderings are the same code.

**The remedy is one line, and it is not another assertion at the end.** It is an
assertion in the *middle*, that the precondition of the hazard holds before the
recovery is exercised. Both closing tests carry one:

```rust
        shell.start_alt_tab();
        shell.next_alt_tab();
        assert_eq!(shell.alt_tab_index, 3, "the last row, not a wrapped one");
```

Without it the fixture's arithmetic is invisible: nothing in the test says which
index it is recovering *from*, so a change to `start_alt_tab`'s opening index --
which module 87's own defect `C` makes -- silently turns the test back into one
that proves nothing. With it, the test fails loudly instead. This generalises:
**whenever a test is written for a failure path, assert that the failure state
was reached.** The assertion costs a line and is the only thing standing between
a regression test and a tautology.

### Lesson 42: a test that re-derives the answer proves its own copy

Escape `T` widens the predicate `run_desktop_action` uses for Super+D:

```rust
-                    .filter(|w| w.on_glass() && w.desktop == self.current_desktop)
+                    .filter(|w| w.mapped && w.desktop == self.current_desktop)
```

`on_glass()` means mapped *and not minimised*, so the widened version asks a
window that is already minimised to minimise again -- a request the compositor
must ignore and, worse, one the user has to undo twice to get back where they
were. There is a test named for exactly this, `show_desktop_does_not_ask_an_
already_minimized_window_to_minimize`, with a comment explaining why the narrow
predicate is the right one. It was green.

Its body was:

```rust
    let asked: Vec<WindowId> = shell
        .windows
        .values()
        .filter(|w| w.on_glass() && w.desktop == shell.current_desktop)
        .map(|w| w.id)
        .collect();
    assert_eq!(asked, [still_here], "...");
```

It never pressed Super+D. It copied the production filter into the test body and
asserted against its own copy -- so it proved the copy, and the copy was not
changed by the defect. This is a worse failure than an untested branch, and it
is worth being precise about why: **an untested branch is invisible, whereas a
test like this is visibly wrong in the other direction.** It appears in the
coverage listing, it carries the right name, it has a comment arguing for the
right design, and it will be counted by anyone auditing whether the property is
covered. It is coverage-shaped and proves nothing.

The rewrite presses the chord and reads what the shell asked for:

```rust
    let outcome = shell.handle_hotkey(&super_d);
    assert!(outcome.consumed);
    let asked: Vec<WindowId> = outcome.requests.iter().map(...).collect();
    assert_eq!(asked, [still_here], "...");
```

Note that grep is a poor detector for this. The copied line is not literally
identical to the production one (`self.current_desktop` versus
`shell.current_desktop`), it sits in a different file from the code it
duplicates, and there is nothing syntactically wrong with a test that filters a
collection -- four other tests in this crate do it legitimately. The
reintroduction sweep is the detector, because the sweep asks the only question
that distinguishes the two cases: *does changing the production code change the
test's answer?* A test that re-derives the answer says no, by construction.

### Lesson 43: a name containing "only" is a universal claim, and needs a negative case

Escape `H` drops the key from the release guard:

```rust
-            if (key.key == Key::LeftAlt || key.key == Key::RightAlt) && self.alt_tab_active {
+            if self.alt_tab_active {
```

Alt+Tab is *held*: Alt stays down while Tab is pressed and released, over and
over, and the switch is committed by the **Alt** release. Under the defect the
first Tab release commits instead, so every use of Alt+Tab lands on the second
window and the user can never reach the third.

Exactly one test in the crate released a key at all:
`a_key_release_only_ends_the_window_switcher`. It releases LeftAlt with no
switcher open (nothing happens), then opens the switcher and releases LeftAlt
(it finishes). Both halves are right. Neither can see the defect, because the
only key either half releases is the one key for which naming Alt and not naming
it agree.

The name says *only*. That is a claim about every key, and the body is two
examples of the one key the claim exempts. The general form is worth stating:
**a test whose name contains "only", "just", "never" or "nothing else" is
asserting a universal, and a suite of positive examples cannot discharge one.**
The negative case is the whole content of the claim. The closing test supplies
it -- release Tab, assert the switcher is still up and has not moved -- and then
releases Alt as well, so it says what the rule *is* and not merely what it is
not.

### Result

```
26 defects: 26 caught, 0 escaped, 0 never asked, 0 under-caught, 4 under-declared
```

Four closing tests: three new ones in `gui/desktop/src/lib.rs`
(`stepping_backwards_from_the_end_lands_in_the_list_not_past_it`,
`a_switcher_whose_window_closed_under_it_still_closes`,
`releasing_tab_does_not_end_the_window_switcher`) and one **rewrite** in
`gui/desktop/src/pointer_tests.rs`. The rewrite is worth flagging as such: the
count of tests did not go up, and the honest description of the change is not
"added coverage" but "replaced a test that proved nothing with one that proves
the thing its name always claimed". All eighteen under-declared catchers across
the two passes were folded back into the palette, which is what dropped the
corpus's single-prover count by twenty.

Unlike module 86, no production bug fell out: every defect here was a fault the
sweep invented, and the shell's keyboard handling was correct as written. What
was wrong was the evidence for it.

`gui/desktop/src/lib.rs` is now 33 of 64 tests never-asked, down from 53 of 61.
The swept corpus stands at **2952 tests, 2310 unproved -- 21.7 % proved, 0
dangling, 184 single-prover**; the figures recorded above after module 86 were
21.5 % and 204.

### Follow-on: what lesson 42 looks like when it is mechanical

Lesson 42 came out of one test that copied a production predicate. The obvious
next question is whether that was a one-off or a class, so lane C went looking
for other places where a test holds its own copy of something production owns.

The search found two shapes, and they are worth keeping apart because only one
of them can be automated.

**The shape a machine can find** is a duplicated *list*. Three desktop test
modules each declared `const OFFERED: [AccentColor; 14] = [...]`, byte for byte
the body of `AccentColor::presets()`, and looped over the copy. Nothing made the
copies follow the original; a fifteenth accent would have been offered by the
settings panel and walked by no test. Worse, `test_accent_color_count` pins
`presets().len() == 14` *next to the list*, which reads as the palette being
guarded and is exactly why nobody noticed the four loops that were not. The fix
was structural rather than another test: delete the three copies and walk
`AccentColor::presets()` directly, so the divergence has nowhere to happen.

The same shape in the general case is `const ALL: [Foo; N]` -- a declaration
that claims to name every variant of an enum, which the language does not
check. Add a variant and the array is still a valid array of N `Foo`s; it has
simply stopped being all of them, and the loop that walks it silently stops
asking about one case. `scripts/check-variant-lists.py` now audits every such
list in lane C against its enum. Verdict today: **59 exhaustive lists, 0 out of
step** -- so this is a hazard rather than a live bug, which is the result worth
having but only because it was checked rather than assumed. The script carries
a `--self-test` because a miscounting checker reports a clean tree exactly the
way a clean tree does, and that is not hypothetical: its first version reported
`CursorShape` as 12 of its 13 variants. `requests/c-a-wire-the-variant-list-gate-into-boot-test.md`
asks lane A to ring it from `boot-test.sh`, where its six siblings live.

Two notes that will save the next reader some work. First, the in-language fix
does not exist yet: `const _: () = assert!(Foo::ALL.len() ==
core::mem::variant_count::<Foo>())` is `E0658` on this tree's host toolchain
(stable 1.95.0, checked directly rather than assumed), and no `gui/` or `apps/`
crate carries a `#![feature]` gate. If it stabilises, the script should be
deleted in favour of the assertion -- an error *at* the list beats a report
*about* the list. Second, an exhaustive `match` elsewhere in the crate is not
the guard it looks like: it does break the build when a variant is added, but
it breaks it in `label()`, the author fixes it there, the compiler goes quiet,
and the list is still the old length.

**The shape a machine cannot find** is the original one: a copied *expression*.
`show_desktop_does_not_ask_an_already_minimized_window_to_minimize` did not
contain a literal copy -- it was `self.current_desktop` in production against
`shell.current_desktop` in the test -- so grep was never going to find it, and
a stricter grep would only have produced false positives to wade through. The
detector for that shape is the sweep itself, which asks the only question that
distinguishes a copy from a check: *does changing the production code change
the test's answer?* No separate audit is warranted, because the audit already
runs, module by module.

That is the audit closed. One duplicated list removed, one class of duplicated
list gated, and the remaining class handed back to the instrument that was
already detecting it.

### Lesson 44: check what the compiler catches by breaking it and reading the line number

The audit above turned up a third shape, and it is the one worth generalising,
because it is not about tests at all.

`Palette::roles()` in `gui/appearance/src/lib.rs` returns
`[(&'static str, Color); 21]` -- one entry per colour field of `Palette` -- and
three sweeps consume it: the two-mode divergence check, the 4.5:1 legibility
floor, and the shell's conversion sweep that asserts a module draws only from
the palette it was handed. Its doc comment said:

> The array's length is part of the signature so that adding a field without
> adding it here fails to compile.

That is false, and it had been false for as long as the comment existed. An
array of 21 entries is a valid `[_; 21]` however many fields the struct has;
the length in the signature only catches the *reverse* mistake, an entry added
to the array without the count being changed. Adding a twenty-second `Color` to
`Palette` produces exactly two errors, both `E0063`, both at the struct literals
in `for_mode` -- and none at `roles`. So the compiler does stop the commit, just
in the wrong place: the author fills in the two literals, the compiler falls
silent, and the new colour is in the palette and in none of the sweeps.

The field the crate added most recently, `teal`, carries a doc comment that says
"a sweep that silently skips a field is the failure those sweeps exist to
catch." The hazard was understood, written down next to the code, and still not
guarded -- because everyone including its author believed the sentence in
`roles`.

**The lesson is the method, not the bug.** Twice in one session a claim about
what the compiler enforces was settled by breaking the code and reading the
error's line number rather than by reasoning about it, and reasoning would have
been wrong both times, in opposite directions:

| Claim | Reasoned | Measured |
|---|---|---|
| "the array length makes a new field fail to compile *here*" | plausible | false -- E0063 at `for_mode`, nothing at `roles` |
| "an exhaustive `match` elsewhere means a stale `ALL` list gets noticed" | plausible | true that it errors, false that it errors anywhere useful |
| "`assert!(ALL.len() == variant_count::<T>())` would fix it" | plausible | `E0658`, still unstable on 1.95.0 |

The measurement costs one `cargo check` against a deliberately broken tree,
which is under a minute, and it is the only thing that distinguishes "the
compiler is watching this" from "the compiler is watching something nearby."
Where a comment claims a compile-time guarantee, break it once and confirm the
error lands where the comment says it does. If it lands somewhere else, the
comment is describing a different guarantee than the one the reader will assume.

**A guarantee documented but not enforced is worse than none, because it is the
reason nobody looks.** An unguarded list at least looks unguarded.

The remedy here was in-language and available on stable: a struct pattern with
no `..` is exhaustive, so `roles` now destructures `*self` and a new field is
`E0027: pattern does not mention field` at the destructure itself. The two
non-colour fields are named and discarded (`panel_alpha: _, light: _`) rather
than swept up by `..`, so the decision that they are not roles is on the record
and a future non-colour cannot slip in behind them. Prefer this to a check
script wherever the shape allows it -- an error at the omission beats a report
about it, which is the same reason `check-variant-lists.py` documents its own
deletion for the day `variant_count` stabilises.

#### What the destructure does and does not buy (measured, same method)

Applying the remedy a third time -- to `DynamicTheme::schedule` in
`gui/desktop/src/wallpaper.rs` -- turned up its limit, by the same means: add a
sixth phase field, build, read the errors.

`E0027` fires on a field the pattern does not *mention*. Mention it and then
not use it and the result is `unused_variables`, a **warning**. So the
guarantee is precisely "the author is brought to this line", not "the author
does the right thing once here" -- a distinction worth writing into the comment,
because the stronger claim is exactly the kind of overstatement this lesson is
about. It still holds in practice on this tree, since the finish checklist
requires a warning-free build, but that is a project convention doing the work,
not the compiler, and the two should not be described as if they were the same.

The `schedule` case is also the first of the three that was a **production**
defect rather than a test one. `schedule` is the only place that says what hour
each phase begins, so a phase omitted from it is a colour that is set, saved,
round-tripped through the config, and never painted -- no test involved. The
lesson-42 audit found this shape by looking for stale *test* lists; that it also
occurs in production code is the more useful half of the finding, and the reason
to look at every hand-written list over a struct's fields rather than only the
ones under `#[cfg(test)]`.

The chain the destructure starts there is worth recording as the shape to aim
for, because each link is a compile error and the last one lands on the safety
property: `E0027` at the destructure -> the array grows and no longer matches
its `[...; PHASE_COUNT]` return type (`E0308`) -> `PHASE_COUNT` is bumped ->
`from_palette`'s parameter and the test's hand-written `SKY` exemption table
both stop compiling. `SKY` is the membership sweep's one exemption from "the
desktop paints only palette roles", so the phase cannot arrive as a colour the
sweep was never told about. A single `E0027` is a nudge; a chain that terminates
at the invariant is a guarantee.

### Lesson 45: a feature with no production caller is a feature that does not exist

`apps/indexer` had a config option, `index_contents`, that did nothing at all.
Setting it to `true` caused no file to be read, no trigram to be stored, and no
content search ever to match. `cmd_search` had a whole fallback branch for
content hits that could not be reached, and printed "No results found" instead.
The option had been in the config file, the `Display` for `Config`, the
serializer and the docs for as long as they had existed.

What made it invisible was the test. `test_search_content` built an index, then
called `index_file_content` itself for two entries, then searched and found
them. Every line of it passes against a build in which nothing in the program
ever calls `index_file_content` -- which was the actual state of affairs. It is
lesson 42 one level up: not a test that re-derives an expected *value*, but a
test that supplies the *step the program was supposed to perform*. It tested
that a function works. Nobody had tested that it is called.

The tell is available without reading any test: **grep the callers of the
function that does the work.** `index_file_content` had exactly two, both
inside `#[cfg(test)]`. A production function whose only callers are tests is
either dead code or an unwired feature, and the two are worth telling apart
because the second is a bug with a config key advertising it.

Two smaller findings from the same fix, both worth generalising:

- **"Off" and "broken" must not be the same observable state.** With
  `index_contents` on, the program did exactly what it did with it off. The
  new `ScanStats::files_content_indexed` counter exists so the two are
  distinguishable at a glance, and the regression test asserts the count and
  not merely that the search came back empty. Any option whose failure mode is
  "produces nothing" needs some positive evidence that it ran.
- **A derived cache is only derived if something re-derives it.**
  `trigram_index` was not written by `serialize`, on the reasonable-sounding
  grounds that it is a cache -- but nothing rebuilt it on load either, so it was
  empty in every process that had not just built it. `name_lookup` next to it
  *is* rebuilt, by `build_from_entries`, which is what made the omission look
  deliberate. When a field is left out of a writer, the question is not "is it
  derivable?" but "where, in the code, is it actually re-derived?"

The check that would have caught all of it is cheap and mechanical: for each
field a serializer omits, name the line that reconstructs it. If there is no
such line, the omission is a defect regardless of how derivable the field is
in principle.

### Lesson 46: a crate-wide `#![allow(dead_code)]` disarms the one lint that finds lesson 45

`apps/diskimager` shipped a complete ISO 9660 volume descriptor parser, a
Browse tab that draws the image's directory tree, and an info card showing
format, volume label and creation date. None of it could run. `load_image` --
the single entry point to all three -- had no caller outside `#[cfg(test)]`,
and there was no key, button or drop target anywhere in the program that
reached it. Meanwhile the Write tab rendered, in as many words, *"No image
loaded. Open an .iso, .img, or .bin file."* -- an instruction for an action
the program did not offer.

The compiler knew. `dead_code` names this exact condition, and in a binary
crate it analyses `pub` items too, because a bin has no external callers. It
said nothing because line 18 of the file was `#![allow(dead_code)]`.

That allow was not put there to hide this. It is the kind of line added early,
while a file is half-built and every second item is legitimately unused, and
then never removed -- and once it is in place it costs nothing to add the
nineteenth unreachable function under it. When the allow came out, the crate
was **already clean**: every item in it was reachable once `load_image` had a
caller. So the allow had not been earning anything for a long time; it was
purely suppressing the one diagnostic that mattered.

Two things generalise:

- **Check that a lint you rely on is armed, not merely quiet.** Removing the
  allow produced zero warnings, which is the same output as a lint that is
  still off. The way to tell them apart is to add a deliberately unreachable
  function and confirm it warns, then delete it. That was done here.
- **A crate-wide allow is a different object from a targeted one.** A
  `#[allow(dead_code)]` on one item, with a comment naming what will call it,
  is a claim about that item that a reader can check. The crate-wide form is a
  standing exemption for code that has not been written yet, and it applies to
  the code that was.

The companion finding is the widget on the other end: `guitk::dialog::
FileDialog` is 1758 lines, fully tested, and had **zero users in the entire
tree** until this fix. Two halves of one feature, each complete, with nothing
joining them. `scripts/scan-unwired.py` exists to find this shape; it reports
per binary with the share of the program `main` reaches, because the
overwhelmingly common cause of an unreachable function in lane C is a
placeholder `main` (`TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR`), which is one
debt and not one finding per function.

A third finding fell out of writing the test, and is worth keeping separate
because it is a plain bug rather than a wiring one: `extract_iso_string`
trimmed whitespace only. ECMA-119 §8.4.6 pads these fields with spaces, so a
conformant image was fine -- but a tool that zeroes the descriptor and writes
the label over the front produces NUL padding, and that went into the info
card as `SLATEOS_LIVE\0\0\0...`. The first fixture written for the end-to-end
test had exactly this shape, by accident, because `vec![0; n]` is the natural
way to build one; the test caught the parser rather than the fixture. A disk
imager reads the images that exist, not the ones the standard describes, so
the trim now covers NUL and a named test pins it.
