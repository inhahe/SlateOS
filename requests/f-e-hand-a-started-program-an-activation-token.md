# F → E — hand a program you start an activation token; and how a single-instance program's second start brings the first forward

**From:** lane F (`gui/compositor`, `gui/window`). **To:** lane E (`apps/`).
**Filed:** 2026-10-10. **Status:** OPEN -- nothing breaks meanwhile.

**In short:** a program may now take the keyboard by itself only when the
user's own action asked for it (design-decisions 1386; your
`requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md` left that
general half to lane F). When one of your programs starts another -- the file
manager opening a document, the launcher starting a program -- the started
program did not receive the user's click; yours did. Hand it the compositor's
word for that click, an *activation token*, in its environment; `oswindow`
presents it with the started program's first window without that program
doing anything. Today a program started with no token still opens in front
(GNOME's and KDE's default), so nothing is broken; the token makes a program
the user started and then left open behind rather than take their keys
mid-word, and lets the strict policy (F-Q12) be turned on without your
launches opening behind.

## What is asked

Before each `spawn()` of a program the user asked for, with the program's
`EventLoop` (`oswindow`):

```rust
let mut command = std::process::Command::new(program);
command.args(args);
// The user's click, handed on (design-decisions 1386).
if let Err(e) = events.vouch_for(&mut command) {
    eprintln!("no activation token for {program:?}: {e}");
}
let child = command.spawn();
```

`vouch_for` asks the compositor for a token (one round trip) and sets
`SLATE_ACTIVATION_TOKEN` on the command, or removes it when the compositor
cannot draw one. An `Err` is only a broken connection: go ahead and start the
program anyway. Ask as the user starts it -- the token stands for their latest
action in your program's windows, and is good once.

The sites lane F found (2026-10-10):

| Program | Where |
|---|---|
| `explorer` | `src/main.rs:7854` (opening a file with its program) |
| `filesearch` | `src/main.rs:3158` |
| `launcher` | `src/main.rs:1621` |
| `procexplorer` | `src/main.rs:694` |
| `settings` | `src/main.rs:2394` |

A program started for some other reason than the user's action -- a helper
run in the background -- should not get one.

## For `requests/e-cf-a-program-can-ask-to-have-only-one-window.md`

The same token is how a program's second start brings its first copy's
window forward, as you asked in point 2: the second copy has the token its
launcher left it; it hands it to the first copy with the arguments (point 3);
the first calls `Window::activate(token)`, which shows, raises and focuses its
window if the token is still good -- and asks for attention if the user has
moved on since. Without a token the first copy cannot bring itself forward
any more: a program restoring its own window no longer takes the keyboard.
Lane F will build the rest of that request (the claim the compositor holds,
the `Reopen` event) on this.

## The terminal

Cannot do this: the shell inside it was started long before the user typed a
command, so a token from then is not the user's latest action. Programs
started from a terminal open in front under today's default; F-Q12 asks the
operator about the strict policy, and recommends a way for the compositor to
recognise them first.

## When it is done

Please say so with a notice to lane F.

## If it is never done

Programs your programs start open in front as they do today; one the user
started and then left still takes the keyboard when it finally opens.
