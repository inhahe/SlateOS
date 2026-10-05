## §478 — A control that cannot act is drawn from the fact that it cannot act

**Date:** 2026-08-20
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the Settings app drew seven push buttons that looked completely
ordinary and did nothing when clicked, because the features behind them — an
accounts service, an activity log, package rollback, a reinstall path — do not
exist yet. A user could not tell "not implemented" from "my click missed". They
are now drawn greyed out. The decision worth recording is not *that* they are
greyed out but *how*: the app is not told twice that a button is inert, once for
the click and once for the paint. It is told once, and the paint is derived.

### The shape

`PageSink::button_at` already carried the fact, as `what: Option<RowHit>` —
`Some` for a button with somewhere to send a click, `None` for the seven. The
obvious fix adds a second argument (`enabled: bool`, or a dimmed colour at the
call site). That is a second statement of one fact, and two statements of one
fact drift: this lane has now fixed that exact drift four times (§475 hit-testing
vs drawing, §476 a slider's mapping read in two directions, §477 a pane's
rectangle converted twice, and this).

So the target picks the painter:

```rust
match what {
    Some(what) => { self.hit_rect(…, what); self.draw(… render_button(…, color)); }
    None       => self.draw(… render_disabled_button(…)),
}
```

**A button painted live while having nothing behind it is now inexpressible** —
not "guarded against", not "tested for", but unsayable, because no argument
exists to say it with.

### Three smaller calls inside it

**`render_disabled_button` takes no colour.** A live button's colour says what
kind of action it is: accent for the ordinary one, red for the destructive one.
A button that cannot act has no kind. Passing the colour through and dimming it
would keep "Remove Account" reading as an alarm about something that cannot
happen. Dropping the argument entirely also means the disabled path cannot
accidentally be given a live colour.

**Same width and height as the live button.** Both come from the same
`button_width`/`BUTTON_HEIGHT`, so the page does not reflow depending on which
features happen to exist. A layout that shifts when a service lands is a layout
that has to be re-checked when a service lands.

**A dimmed button still registers no click band.** This was already the
behaviour and the original report argued for it; the argument survives the
change and is worth restating, because "disabled" in most toolkits means
*consumes the click and ignores it*. Here it must not: a band that swallowed the
click would take away the only pointer-level feedback the user gets ("nothing
happened here") *and* block whatever is drawn beneath from ever receiving it.
Dimming says why nothing happened; not registering keeps the click honest.
`pressing_a_dimmed_button_is_ignored_rather_than_swallowed` holds this in place.

### The alternative that was rejected

*Hide the seven buttons entirely until their features exist.* It is defensible —
nothing false is shown — and it was rejected on two grounds. It makes the pages
change shape as services land, so every page's layout has to be re-verified
piecemeal rather than once. And it removes the only signal a user has that the
feature is *intended*: a greyed-out "Go Back" on the Recovery page says rollback
is coming; an absent one says the OS has no such concept. The dimmed button is a
smaller lie than the missing one.

### One repair the change forced

"Change Password" was not going through `button_at` at all — it was painted by a
raw `render_button` inside a `row(…)` closure, which is why it could never have
had a click band and why no amount of care at the `button_at` call sites would
have covered it. A `PageSink::button_row` now routes a button-in-a-row through
`button_at`, so there is one path. This is the recurring lesson stated the other
way round: collapsing the duplicate is what *finds* the copies, because anything
left outside the single path stands out.

### Testing note

The tests take the paint from the render tree and the clickability from the page
walk — two independent places — and assert they agree in both directions. One
mutation escaped the first run: restoring the disabled button's full-brightness
label was invisible, because the tests read only the fill colour. A dimmed fill
under a live label is half a disabled button and reads on screen as a live one.
Both commands are checked now. The same lesson as §476 and §477 in yet another
costume: **a check that reads only part of what the user sees only proves that
part.**
