## 1386. A program takes the keyboard on its own say-so only when the user's action asked for it: activation tokens, and the smart default for new windows

**Date:** 2026-10-10
**Lane:** F
**Decided by:** Claude (autonomous), answering the half of
`requests/e-cf-a-consent-prompt-is-answered-only-on-purpose.md` that lane E
left "yours to judge" (focus-stealing prevention for every window), and the
keyboard half of `requests/c-abf-a-program-asking-for-a-capability-reaches-no-one.md`
F2. Whether new windows should follow the strict rule as well is put to the
operator: `open-questions/F-Q12.md`.

**In short:** until now any program could take the keyboard whenever it
liked -- by opening a window, or simply by asking to "restore" a window it
already had. Whatever the user typed next went to that program, and so did
the clipboard, which only the window holding the keyboard may read. Now a
program can take the keyboard by itself only when the user is already in it,
or when the user's own action asked for it: a launcher (the start menu, a file
manager) hands the program it starts a one-time *activation token* standing
for the user's click, and the program shows it with its first window. A
program that cannot show one asks for attention instead: its taskbar button
flashes and its window waits behind the one the user is in. New windows with
no token still take the keyboard, for now, as on GNOME and KDE -- otherwise
every program started from a terminal would open behind it.

### The rule

A program's window may take the keyboard on the program's own say-so when:

| The keyboard is held by ... | Allowed |
|---|---|
| no window | yes |
| a window of the same program (connection) | yes |
| another program's, and the program presents an activation still good | yes |
| another program's, and the activation it presents is not good | no |
| another program's, no activation, a **new** window | yes -- unless the policy is strict |
| another program's, no activation, a **restore** or an **activate** | no |

Refused, a new window opens directly *beneath* the window holding the
keyboard and asks for attention; a minimised window that tried to restore
itself stays minimised and asks for attention. The user moving the keyboard --
a click, a shortcut, the taskbar, the shell -- is untouched by any of this.

**An activation** stands for one of the user's actions. The compositor numbers
them -- every key press, typed text and button press -- and stamps each window
with the latest it took part in: when it takes the keyboard, and at each
action while it has it ("user time", as Mutter calls it, but kept by the
compositor rather than claimed by the program). An activation is good while
the window holding the keyboard has seen no later action. So a program the
user started and then left -- they went on typing elsewhere while it loaded --
opens without taking the keys mid-word.

Activations come from:

- **A token** (`RequestBody::GetActivationToken`): sixteen bytes from the
  kernel's random source, standing for the latest action in the drawing
  connection's windows. The launcher puts it in the started program's
  environment as `SLATE_ACTIVATION_TOKEN`; `oswindow` presents it with the
  program's first window (`UseActivationToken`) without the program doing
  anything; a running program handed one another way presents it with
  `Activate` (an existing window) or `use_activation_token` (its next one).
  Good once. A token presented that the compositor does not hold counts
  *against* the program: a program that claims the user started it and
  cannot show it gets less benefit of the doubt than one that claims nothing.
- **The shell clicking a program's tray icon** (`ClickTrayIcon`): the click
  was the user's, on the shell's panel, and meant for the program -- so the
  program may bring its window back, which is what "restore from the tray"
  is.

### Why each choice

| Question | Decided | Alternative | Why |
|---|---|---|---|
| Where the line is | The user's action, carried by a token | A rule by band ("never from a window in front of yours"), tried first on 2026-10-10 | The taskbar is in that band and holds the keyboard after any click on it, so every program started from the start menu would have opened without the keyboard. The compositor cannot tell the panel from a prompt by band; it can tell an action the user took from one they did not. |
| Mechanism | Wayland's xdg-activation-v1: a token drawn by the program the user is in, handed to the program it starts in an environment variable, good once | Process ancestry (Windows: "started by the foreground process") | Ancestry needs the kernel to say whose child a connection is, which a TCP client cannot, and a native program's connection is TCP until `requests/f-d-the-c-library-s-slateos-channel-calls.md`. A token works over any transport and needs only the launcher's cooperation. Ancestry remains the way to cover terminals (F-Q12). |
| What a token is compared with | The keyboard holder's user time, stamped at focus as well as at each action | The global latest action; or the holder's own typing only | The global count would make a scroll over a background window, or a key the shell grabbed, invalidate every launch. The holder's typing alone would let an old token beat a window the user has just clicked into but not yet typed in. |
| Token bytes | 128 bits from `randrange::fill_secret` (the kernel CSPRNG) | A counter | Any program may present a token -- the one that uses it is by design not the one that drew it -- so a guessable one could be presented by a program racing the one it was drawn for. No random source: no token, an error, and launches carry none (which the smart default still lets open in front). |
| Bounds | Eight outstanding per drawing connection, sixty-four in all | One global queue | A global queue lets one program drawing tokens in a loop push out the shell's before the program it started presents it. |
| A program the user has not touched asks for one | Refused (`NoToken::NoAction`), so its launches carry none | Drawn, and good for nothing | A worthless token would get the program it starts refused the keyboard, where one started with none is treated as any program is -- a session manager starting programs at login should not make them worse off by vouching. And it keeps programs nobody has used out of the table. |
| New windows with no token | Take the keyboard (GNOME's "smart", KDE's default) | Strict: refused, as GNOME's experimental "strict" and KDE's "extreme" do | A terminal cannot hand on a token (its shell's environment was fixed when the terminal started), and neither the shell nor any lane E launcher hands one on yet, so strict would open every launch behind. Strict is `NewWindows::Strict`, one call away; the switch is F-Q12. |
| Restores with no token | Refused | Allowed as new windows are | A program restoring a window it already had was not just started: it has no excuse a launch has, and allowing it is exactly how a program takes the keys at a moment of its choosing. Nothing in the tree calls `restore()`. |
| Where a refused new window goes | Directly beneath the window holding the keyboard | The top of its band, as before | In front, a window that cannot be typed into looks as though it can, and the user's next keys go somewhere they cannot see. Mutter places a refused window the same way. |

### References

GNOME Shell's own account, "Understanding GNOME Shell's focus stealing
prevention" (blogs.gnome.org/shell-dev, 2024-09-20); the xdg-activation-v1
protocol (wayland.app/protocols/xdg-activation-v1: single-use tokens, the
`XDG_ACTIVATION_TOKEN` variable, compositors may refuse a token without a
recent serial); KDE's move to stricter focus-stealing prevention on Wayland
(osnews 143040); Mutter's `intervening_user_event_occurred`, the user-time
comparison this keeps.

### What it changes today

For the programs in the tree, nothing visible: none restores itself, none
presents a token yet, and new windows with none still open in front. The
strict rule can be tried with `compositor --focus-new-windows strict`. What
changes is that a program *cannot* take the keyboard by restoring a window,
and that the launchers can now hand on a token -- the shell's
(`requests/f-c-hand-a-started-program-an-activation-token.md`) and lane E's
(`requests/f-e-hand-a-started-program-an-activation-token.md`), and a
single-instance program's second start, which can now bring the first
forward properly (`requests/e-cf-a-program-can-ask-to-have-only-one-window.md`).

### Revisit if

- The launchers hand tokens on, and terminals are covered (by ancestry, once
  native programs reach the compositor over a channel): then strict new
  windows cost little -- F-Q12.
- The shell's prompts open without taking the keyboard
  (§1242, `requests/f-c-build-the-shells-window-terms-from-spec-new.md`): a
  window the shell keeps should then be one no new window takes the keyboard
  from, token or not.
