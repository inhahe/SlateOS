# F → C — hand a program the shell starts an activation token, so it opens in front for the right reason

**From:** lane F (`gui/compositor`, `gui/window`). **To:** lane C
(`gui/desktop/src/main.rs`). **Filed:** 2026-10-10. **Status:** OPEN --
nothing breaks meanwhile.

**In short:** a program may now take the keyboard by itself only when the
user's own action asked for it (design-decisions 1386). A program the shell
starts did not receive the user's click -- the shell did -- so the shell
should hand it the compositor's word for that click: an *activation token*,
in the started program's environment. `oswindow` presents it with the
program's first window without the program doing anything. Today a program
started with no token still opens in front (the "smart" default, GNOME's and
KDE's), so nothing is broken; what the token adds now is that a program the
user started and then left -- they went on typing elsewhere while it loaded --
opens behind instead of taking their keys mid-word. And it is what lets the
strict policy (F-Q12) be turned on without every launch opening behind.

## What is asked

One call in `drain()`, before each `command.spawn()`:

```rust
let mut command = Command::new(&launch.program);
command.args(&launch.args);
// ...
// The user's click, handed on: the program's first window may take the
// keyboard because the user asked for it (design-decisions 1386).
if let Err(e) = session.events_mut().vouch_for(&mut command) {
    eprintln!("desktop: no activation token for {}: {e}", launch.display_line());
}
```

`EventLoop::vouch_for` asks the compositor for a token (one round trip) and
sets `SLATE_ACTIVATION_TOKEN` on the command -- or removes it, when the
compositor cannot draw one, so the shell's own inherited token is never passed
on. An `Err` is only a broken connection; the launch should go ahead without
a token rather than fail. Ask *as the user starts the program*, as `drain()`
does: the token stands for the user's latest action in the shell's windows,
and is good once.

Anything else in the shell that starts a program should do the same; as far
as lane F can see, every launch goes through `take_launches` → `drain`.

## Not asked: the tray

A click on a program's tray icon needs nothing from you: the compositor gives
the clicked program an activation itself when it passes the click on
(`ClickTrayIcon`), so the program can restore its window from the tray. A
program restoring itself *without* such a reason no longer takes the keyboard
-- that was any program's way to take the keys, and the clipboard with them,
whenever it liked.

## When it is done

Please say so with a notice to lane F. Nothing in lane F waits on it, but
F-Q12's strict option does.

## If it is never done

Programs the shell starts open in front as they do today. A program the user
started and then left still takes the keyboard when it finally opens, and the
strict policy could not be turned on without every launch opening behind.
