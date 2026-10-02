## 500. A settings change reaches a running compositor as a notification that carries nothing

**Date:** 2026-08-21
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** After §499 the compositor finally drew window corners and drop
shadows the way the user asked — but it read the user's file once, at startup.
Change the setting in Settings and nothing happened until you logged out and
back in. This adds a message the Settings app can send saying "your appearance
file changed"; the compositor then re-reads the file itself. The message
deliberately does not contain the settings, because a message that *did* would
let any program on the machine dictate how every window is drawn.

### The setter that was not written

The obvious message is `SetAppearance(AppearanceSettings)`: the sender already
has the settings in hand, and sending them saves the compositor a file read.
It was rejected on both of its terms.

**Security.** Anything that can open the display socket can send any request on
it. If the request carried settings, every such program could restyle the whole
desktop — and a hostile set of settings is not an abstraction: title-bar text
the colour of the title bar, a close button the colour of the bar behind it, a
corner radius that eats the button. None of that is a crash, which is what
makes it a good attack; it is a desktop that quietly stops being operable.
A *notification* inverts the trust: the compositor re-reads the user's own
file, which the sender may have no permission to write, so the worst a hostile
client achieves is making the compositor read a file and repaint a screen that
already looks the way it looks.

**Ownership.** `AppearanceSettings` has one owner (`gui/appearance`) precisely
so that the shell, the Settings app and now the compositor cannot drift into
three interpretations of one preference — that is the whole of §499. A wire
encoding for the settings would be a fourth copy of the model, in a place where
it must also be versioned; the day someone adds a field, a compositor and a
Settings app of different vintages disagree about a struct rather than about a
file that both parse with the same code.

So: `RequestBody::ReloadAppearance`, tag `0x0F`, no payload. A test in
`gui/remote/src/control.rs` asserts the *encoded length* — header, sequence,
tag byte, nothing else — so that "just a corner radius, to save a file read"
fails at the point where it is added rather than in review.

### Why a protocol verb and not a file watch

The alternative with no protocol surface at all is for the compositor to watch
`appearance.yaml` and reload when it changes. That is strictly better in one
respect: it works no matter *who* wrote the file, including a user editing it
in a text editor, which the notification does not.

It was not chosen because nothing in this OS can watch a file yet — there is no
change-notification interface a userspace process can subscribe to, so a watch
today means the compositor stat-ing a path on a timer, which is a poll in the
one process that must not spend its frame budget on bookkeeping. The verb is
also not wasted work if the watch arrives later: a watch would make the verb
*redundant*, not wrong, and a compositor that both watches and accepts the
notification is correct, because the reload is idempotent by construction (see
below). Revisit when the filesystem gains change notifications; the trigger is
`fs/`'s change-notification work landing.

### The comparison lives in `set_appearance`

Any client may send `ReloadAppearance` at any rate. If each one forced the
full-screen recomposite that a settings change genuinely needs, the request
would still be an unlimited-rate way to keep the compositor redrawing the whole
desktop — a notification that carries no data but costs plenty. So
`set_appearance` early-returns when the new settings equal the old, and only a
real difference sets `full_recomposite`.

That comparison belongs there and not in the handler: `reload_appearance` is
not the only caller (startup calls it too, and a future file watch would), and
a guard in the handler would be a guard one caller has and the others do not.
`AppearanceSettings` already derives `PartialEq`, so this is a comparison of
the model rather than a hand-written field-by-field check that could forget the
field added next week.

### The reply is `Ok` whether or not anything changed

A reply that distinguished "reloaded, and it differed" from "reloaded, no
change" would be more informative and was rejected for being exactly that: it
tells whoever asked something about the contents of the user's settings file.
A program that cannot read `~/.config/slateos/appearance.yaml` could learn
whether a given guess matches it by writing nothing and watching the reply
change. That is a small leak, but it is a leak bought for no benefit — the
sender is the program that just wrote the file, and it already knows.

What the `Ok` asserts is true either way: the compositor has re-read the file.

### The sender is in `oswindow`, and the Settings app still cannot call it

`oswindow::EventLoop::appearance_changed()` is the public API; applications
never name `guiremote` directly, exactly as `watch_desktop` fronts
`subscribe_window_list`. The one application that should call it — Settings —
cannot: it has no compositor connection at all, because its `main` is a
one-frame smoke test. That is `TD-NO-APP-CONNECTS-TO-THE-COMPOSITOR` (142 app
crates in the same position), not something to paper over here by opening a
socket in a program that has no event loop to service it. Recorded there rather
than left implied.

### Verification

Seven tests, each proved a real regression test by putting its bug back and
watching it fail:

| Test | Bug reinstated |
|---|---|
| `a_reload_request_carries_nothing_a_client_could_restyle_the_desktop_with` | a payload byte appended to the encoding |
| `telling_the_compositor_the_settings_changed_sends_it_nothing_but_the_news` | `appearance_changed` returns `Ok` without sending anything |
| `a_reload_adopts_what_the_users_file_now_says` | `reload_appearance` does not re-read the file |
| `a_reload_that_changed_something_repaints_the_whole_screen` | adopting settings does not set `full_recomposite` |
| `a_reload_that_finds_nothing_changed_costs_nothing` | the equality early-return removed |
| `a_reload_request_off_the_wire_reaches_the_users_settings_file` | the handler arm stubbed to reply `Ok` and do nothing |
| `a_reload_request_names_no_window_and_so_needs_no_window_to_name` | the request routed through the window-ownership check |

The two `wire.rs` tests are the ones that matter for a new verb: they run the
real bytes through the real decode path, and a verb that encodes but never
decodes — or decodes to a request nothing maps — is the characteristic way a
protocol addition is half-wired.

`$HOME` is not read by `Compositor::new` and is read by `reload_appearance`.
That asymmetry is deliberate: a constructor that consulted the environment
would make every compositor test depend on the machine running it, while
re-reading the user's file is the entire meaning of a reload. The tests point
it somewhere harmless with `appearance::config::testing::with_scratch_config`,
and `main` now calls `reload_appearance()` at startup rather than loading the
file itself, so startup and reload cannot come to disagree about where the
settings live or what a missing file means.
