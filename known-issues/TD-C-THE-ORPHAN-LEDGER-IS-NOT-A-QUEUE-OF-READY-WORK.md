## TD-C-THE-ORPHAN-LEDGER-IS-NOT-A-QUEUE-OF-READY-WORK

**In short:** `scripts/orphan-modules-baseline.txt` lists 46 modules nothing
calls, and it reads like a to-do list: plug each one in and the debt goes away.
It is not. A large share of them are user interfaces for subsystems that **do
not exist yet**, so "wiring" them would connect a control to nothing and produce
a more convincing lie than leaving them alone. Before picking one off the list,
check that the thing it talks to is real. Four were sampled on 2026-09-07 and
four were blocked.

### Verified blocked, with the reason

| Module | Needs | State |
|---|---|---|
| `gui/desktop/src/icons.rs` | a desktop icon model | **Two** exist — this one, and `kernel/src/fs/deskicons.rs` (lane A, done). Wiring either entrenches a duplicate; asked in `requests/c-a-two-desktop-icon-models-and-mine-cannot-be-wired-until-we-pick.md` |
| `gui/desktop/src/clipboard_viewer.rs` | a system clipboard | **None exists.** No clipboard in `gui/compositor`, none in the `guiremote` protocol; the only hits in `gui/toolkit` are `listview.rs`/`textview.rs` doing their own local selection. A history viewer for a clipboard nothing populates |
| `gui/desktop/src/screen_capture.rs` | a video encoder | Screen *recording*, not screenshots. `roadmap.md` still lists "Video-encoded capture fallback (H.264/VP9)" as an unstarted `[C]` item. The compositor's `capture_stream_frame` gives frames; nothing turns them into a file |
| `gui/desktop/src/blur.rs` | raw ARGB pixel buffers | **In the wrong crate for its consumer.** It is a software blur over pixel buffers, and the shell has no pixels — it sends `RenderCommand`s and the *compositor* owns the framebuffer. Either it moves to `gui/compositor` or the protocol grows a "blur behind this surface" flag; both are design questions, not wiring |

`blur.rs` is the interesting one: it is not blocked on a missing subsystem but
on being in the wrong place, which the baseline cannot express and which looks
identical from the outside.

### One chain that is genuinely ready

`gui/desktop/src/widgets.rs` (2,275 lines) is self-contained in the way the
others are not. `DesktopWidgetManager` offers `render(&Palette) ->
Vec<RenderCommand>`, `tick(now_ms)`, `hit_test`, and add/remove/move/resize —
and the shell already owns the background surface those commands would go to.
Nothing external is missing.

Its one gap is an *entry point*: there is no way to add a widget, because the
shell has no desktop context menu. And `context_ext.rs` — in both
`gui/desktop/` and `gui/toolkit/` — is **also** on the baseline. So two islands
plug into each other: right-click the desktop → add a widget → it renders on
the background surface and ticks itself.

Doing widgets without the menu would repeat the mistake taskbar auto-hide
avoided: a working feature with no way to reach it. The two go in together, or
neither does.

### Why this entry exists rather than a longer classification

The remaining ~40 were not audited. Sampling four cost most of one working
session, and a table of guesses about the other forty would be worse than no
table — the failure mode this whole file keeps recording is a confident answer
nobody checked. What is written above is what was verified. **Check before you
wire; expect blocked more often than not.**
