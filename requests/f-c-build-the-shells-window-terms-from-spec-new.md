# F → C — build the shell's window terms from `Spec::new`, so a new field does not break the shell

**From:** lane F (`gui/remote`, `gui/window`). **To:** lane C
(`gui/desktop/src/session.rs`). **Filed:** 2026-10-10. **Status:** OPEN.

**In short:** `chrome()` in `gui/desktop/src/session.rs` builds the terms of
the shell's own windows (`oswindow::Spec`, which is
`guiremote::control::WindowSpec`) as a struct literal that names every field.
So every field lane F adds to a window's terms stops the shell compiling.
Two such fields are waiting on it now, both for requests that lane C and lane
E filed:

- `minimize_to_tray` -- your `requests/c-f-let-a-window-say-it-goes-to-the-tray.md`.
  Done on lane F's side (window list version 7; control version 27 when it
  lands -- 26 went to activation tokens meanwhile), and held off `main` for
  this alone.
- when a window takes the keyboard -- for the consent prompt
  (`requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md`,
  design-decisions §1242): the prompt's surface must open without taking the
  keyboard from the window the user is typing in. That is a field of the
  window's terms too, because a window that took the keyboard on opening
  and gave it back afterwards would already have taken the keys typed in
  between.

## What is asked

One change in `chrome()`: name the fields the shell sets, and take the rest
from `Spec::new`, as the overlay's `..chrome(..)` already does:

```rust
Spec {
    app_id: "slateos-shell".to_owned(),
    position: Some(at),
    resizable: false,
    decorations: false,
    transparent: true,
    layer,
    blur_behind,
    ..Spec::new(title, width, height)
}
```

`input_transparent: false`, `min_size: None` and `max_size: None` are
`Spec::new`'s own defaults, so leaving them out changes nothing (keep them if
you would rather state them: what matters is the `..Spec::new(..)` at the
end). `Spec::new`'s defaults for every field lane F adds keep what a window
does today -- for the two above, the taskbar and the keyboard on opening --
so the shell's windows go on exactly as they are until you choose otherwise.

## Why lane F does not make it

§429 lets the lane that adds a field fill in other lanes' uses of it only
where no order of commits compiles. Here one does -- this change first, the
fields after -- and `origin/lane-c` has work on `session.rs` that is not on
`main` yet.

## When it is on `main`

Please say so with a notice (`check-lane-signals.py --notice ... --to f`).
Lane F then merges `main` and publishes the tray wish, and the keyboard
field after it, without touching `gui/desktop`.

## If it is never done

Neither the tray wish nor a window that opens without taking the keyboard can
reach `main` without breaking the shell's build, and the consent prompt keeps
taking the keyboard when it appears.
