## 570. A window rule fires once, when the window arrives — and the shell hands the requests back rather than sending them

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** Window rules say things like *"open the editor maximised"*. The
compositor tells the shell what windows exist several times a second, so the
shell has to decide **how often** to consult the rules. Consulting them every
time would mean a window you just un-maximised snaps back within the frame, and
a "do this once" rule would be used up on the first frame after you wrote it.
So the rules are consulted exactly once per window, the first time that window
is seen. And the shell does not send the resulting requests itself — it returns
them to whoever is holding the connection.

### Once per arrival, not once per list

`WindowRulesManager::evaluate` is `&mut self` and is not a pure query: it
increments each rule's match count, and it *deletes* one-shot rules. That makes
the calling frequency a correctness question rather than a performance one.

- **Per list** (the naive reading of "apply the rules to the windows"): a
  one-shot rule is destroyed on the very next frame, spent on whichever windows
  happened to be open when it was written. `initial_state` is re-sent on every
  frame, so a window the user restores is re-maximised before the next repaint
  and cannot be restored at all. Match counts climb at the frame rate and mean
  nothing.
- **Per newly-arrived window** (chosen): a rule fires once for the window it is
  about. "Initial state" means the state it *starts* in, and stops being the
  shell's business the moment the user touches it.

"Newly arrived" is read off `previous.is_none()` — the shell keeps its windows
in a map keyed by id, and the compositor's ids come from an `IdSeq` and are
never reused, so absence from the previous map is a sound test and no second
"already seen" set is needed.

The consequence is that the two shell-local answers, `skip_taskbar` and
`skip_alt_tab`, must be **carried across lists** the way the taskbar icon is.
Nothing in a window list could confirm them later — they are not facts about
the window, they are the answer a rule gave about it once.

`remember_state` is the deliberate exception: it is fed from **every** list,
because "remember the last position" means the position the window was last
*at*, and a window moves long after it arrives. It is skipped while the window
is minimised or maximised, because the rectangle then belongs to the state
rather than to the window, and restoring a window to its maximised rectangle
would be a window that looks maximised but is not.

### Requests out, not sent

`apply_window_list` returns `Vec<ShellRequest>` rather than sending anything.

- **For:** `DesktopShell` stays connectionless — it is a pure function of window
  lists and input, which is what lets ~30 existing tests drive it with no
  compositor at all. The session, which already owns the connection and already
  translates `ShellRequest`s onto the wire for taskbar clicks and hotkeys, gains
  one more source of the same thing. Errors are handled in one place.
- **Against:** the caller can drop the value, and the compiler will not say so —
  `#[must_use]` is deliberately *not* on it, because the demo binary in
  `main.rs` has no compositor to send to and the ~30 test call sites have
  nothing to send. A shell that evaluated rules and dropped the answer looks
  identical from either end, which is the failure this trades for; it is covered
  instead by an end-to-end session test that asserts a rule reaches the wire.

### Two lists, one filter

`skip_taskbar` and `skip_alt_tab` are separate actions because users mean
different things by them: a chat window kept out of the taskbar is still
somewhere you want to Alt+Tab to, and a monitoring window you never switch to
still wants a button. But `taskbar_windows()` was serving the taskbar, the
overview *and* the Alt+Tab index at once, so honouring both flags from one list
would have made each flag silently mean the other.

They are now `taskbar_windows()` and `switcher_windows()`, both derived from one
`listed_windows(also)` helper with one sort. Written twice they would be free to
drift in *order*, and `alt_tab_index` counts into the switcher's list: an index
into a differently-ordered list activates a window the user was not looking at.

**Where it lives.** `gui/desktop/src/lib.rs` — `DesktopShell::apply_window_list`,
`rule_requests`, `listed_windows`; `gui/desktop/src/session.rs` `pump`.
