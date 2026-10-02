## `TD-C-THE-SHORTCUT-CARD-HAS-NO-DOOR` (lane C, 2026-08-26) — **RESOLVED 2026-08-26**

**2026-09-27: the door moved.** The operator turned the card's chord off by
default (`design-decisions.md` §1416, answering C-Q24), which would have
reopened this entry: a card only a shortcut opens is one nobody can reach to
bind the shortcut. The start menu's places column has a **Keyboard Shortcuts**
place now (`StartShortcut::KeyboardShortcuts`), which opens the card; Super+/
still works for a user who binds it. What follows is the record as of
2026-08-26.

**Resolution.** Piece 1 below is done. `HotkeyAction::ToggleShortcutCard` exists,
`Super+/` is its default binding (and is listed on the card, so the card teaches
its own chord), `DesktopShell::shortcut_card_open` holds the state,
`render_shortcut_card()` centres the panel on the display, and
`SessionRunner::paint_chrome` draws it — over the overview and the zone overlay,
which dim what is behind them, and under Alt-Tab, which is modal. It joins
`any_popup_open`/`dismiss_popups`, so Escape closes it and the Escape grab is
reconciled for it like every other popup.

Fixing the door exposed a second defect the card had all along, fixed in the same
change: **the card did not fit on the screen.** Twenty-five bindings at 38 units
a row came to 1,010 units of list plus chrome — 1,086 tall on a 1,080-line
display — so the last rows were drawn past the bottom edge with nothing on the
card to say they existed. `hotkeys::panel_layout` now folds the list into as many
columns as it takes to fit the height the caller can spare; a card that fits is
left at one column. See `design-decisions.md` §572.

Piece 2 — **rebinding from the card** — is *not* done and is now tracked on its
own as `TD-C-THE-SHORTCUT-CARD-IS-READ-ONLY` below.

---

**In short:** the desktop can draw a "Keyboard Shortcuts" card — a list of every
chord and what it does, styled, scrollable-height, with the selected row
highlighted — and there is no way for a user to make it appear. Nothing on the
desktop opens it: not a chord, not a start-menu entry, not a right-click. So a
user who wants to know what Super+Z does has to read the source.

This got sharper, not softer, when the two shortcut tables were merged (see
`design-decisions.md` §571): the shortcuts are now *editable*, and a user who
can change a binding has a much stronger claim on being able to see what the
bindings are.

**Where it lives.** `gui/desktop/src/hotkeys.rs` —
`render_settings_panel(registry, palette, x, y, selected_index)` returns a
self-contained `Vec<RenderCommand>` and is called from nothing outside its own
tests. It is fully exercised by them (colour roles in both themes, badge
geometry, empty and default registries), so this is a wiring gap, not an
unfinished renderer.

**What the proper fix is.** Two pieces, and the first is the whole of the
minimum:

1. **Something that opens it.** The natural home is the shell's own popup
   surface, alongside the start menu and the notification pane — it is the same
   kind of thing: a sheet the shell draws over the desktop and dismisses with
   Escape. That means a `HotkeyAction::ToggleShortcutCard`, a default chord, and
   a branch in `DesktopShell::run_desktop_action` that flips a `bool` the way
   `ToggleNotifications` does. The chord wants to be one the card itself would
   list; Super+/ (i.e. Super+Slash) is what macOS and most editors use for
   "show me the shortcuts" and is unclaimed here.
2. **Rebinding from the card.** `selected_index` and the registry's conflict
   detection are already there, which is most of what an editor needs; what is
   missing is a "press the new chord now" capture mode and a call to
   `HotkeyConfig::from_registry(...).save()` to persist it. This is the larger
   half and is not required for the card to be worth opening — a read-only
   reference card is already the thing users ask for.

Until then the renderer is dead code that passes its tests, which is the state
this project has repeatedly found to be worse than absent code: it looks done.
