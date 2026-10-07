## 1494. The shortcut card answers the pointer: its keys are buttons along its foot

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the card of keyboard shortcuts -- the place a user changes
what a key does -- could only be used with the keyboard. Its commands were
keys (Enter for new keys, F2 to change what a shortcut does, Insert to add
one, Delete to remove one, Escape to close), and a click on the card went
straight through it to whatever was underneath. Someone who works with a
pointer, a switch or their voice could read every shortcut and change
none, and so could a screen reader or a script: there was nothing on the
card to press. Now each of those keys is also a button along the card's
foot -- New keys, Change action, Remove, Add, Close; Cancel while keys are
recorded; Move and Keep when the keys pressed are already taken; Choose and
Cancel over the list of actions, OK and Cancel under the program field --
and a press on a button *is* its key, handed to the card's editor as the
keyboard hands it. A click on a row chooses it and a double click asks for
its new keys; a click chooses an action from the list and a double click
takes it; the wheel scrolls that list. A greyed button says why while the
pointer rests on it. A press off the card still goes through to the
desktop, as before: the card is kept up while the desktop is used.

**Where:** `gui/desktop/src/shortcut_editor.rs` -- `CardButton`,
`ShortcutEditor::{buttons, layout, render_foot, automation, choose_row,
choose_entry, scroll_entries, entry_place}`, `CardLayout`, `CardSpot`,
`CardPart`; `gui/desktop/src/hotkeys.rs` -- the room for the foot
(`foot`), `row_rects`, `Hotkey::parse`; `gui/desktop/src/lib.rs` --
`shortcut_card_layout`, `key_on_shortcut_card`, `mouse_on_shortcut_card`;
`gui/desktop/src/accessible.rs` -- `ShellPart::Shortcuts`; and
`guitk::widget::automation::Refusal::NotAKey`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **Buttons for the keys, drawn on the card** | keep the card keyboard-only, and give tools a "press this key" action | A user who cannot press F2 is the case that matters, and a tool can only press what is on the screen "as the user would" (§1491); a key action would let a script do what no pointer user could, and the automation vocabulary is of widgets' own actions, not of keystrokes. | A row of buttons where a line of hints was enough for keyboard users: the hint line stays, so they lose nothing. |
| **A button's press is its key, handed to the editor** | a pointer path of the editor's own beside its keys | One meaning for both: the editor's six states and what Enter and Escape mean in each are tested once, and a click cannot come to mean something the key does not. | A button's label and its key must agree; `CardButton::key` is one table. |
| **The card takes presses on itself and lets the rest through** | a modal card that takes every press, or the old card that took none | It is opened to be read while the desktop is used (§572's reference card); a press on it means the card, a press off it means what is under it -- neither is lost. | Two rules where one would be simpler; the routing puts the card after the menus and before the desktop's own things, which `handle_mouse_inner` says in place. |
| **The foot laid out for the list's five buttons in every mode** | each mode's own buttons, the card sized to them | The card keeps its size while keys are recorded or a question asked, so nothing under the pointer moves when a button is pressed. | Recording shows one button in room for five lines' worth at the largest text sizes. |
| **The foot measured in the theme's button shape, wrapping across one column** | a fixed width | A theme's wider padding and a larger text size still fit: the buttons take more lines rather than run off the card. | The rows' room depends on the theme's buttons, so `shortcut_context` asks for the theme. |
| **Keys given to a recording as text, `Ctrl+Alt+T`, as the shortcuts file writes them** | refuse text, a key at a time | A tool can say the keys it means in the form a user reads on the card; they are pressed as the user presses them, through the same path. A name that is no key is refused (`NotAKey`) rather than recorded as nothing. | One more refusal in the toolkit's vocabulary. |
| **An entry of the action list walked to with Up and Down by tools** | a click at its row | It reaches an entry scrolled out of the list as the user's arrows do, with the list following, without a tool having to scroll first. | A tool's choice moves through the entries between, as the keys do. |

**If reversed:** the card goes back to taking no press, and its buttons,
pointer handling and tools' parts go with it; the editor's keys are
untouched.
