## TD-C-ELEVEN-SETTINGS-PAGES-SAY-COMING-SOON-AND-FIVE-OF-THEM-ARE-ALREADY-WRITTEN

**Date:** 2026-09-14. **Lane:** C.

**In short:** the Settings application has twenty-nine pages down its side.
Eleven of them, when you click on them, say *"This page is under
construction"* under a roadworks sign. Five of those eleven are finished
somewhere else — complete, tested panels sitting inside the desktop shell
with no way to open them. So a user is told a feature does not exist yet
while the code for it is compiled into the machine they are running.

**Where:** `apps/settings/src/main.rs` — `build_page` dispatches eighteen of
the twenty-nine pages to a real builder and sends the rest to
`build_placeholder_page`, which draws the roadworks. The finished panels are
`gui/desktop/src/*.rs`, all on `scripts/orphan-modules-baseline.txt`.

| placeholder page | finished panel | lines |
|---|---|---|
| ~~Notifications~~ | **done 2026-09-14** — `gui/notifsettings` + a real `build_notifications_page`; `desktop::notification_settings` is now a dead copy to delete | — |
| Power | `desktop::power_settings` | 1 695 |
| WiFi, Ethernet, VPN | `desktop::network_settings` | 4 107 |
| Default apps | `desktop::default_apps` | 2 325 |
| Startup apps | `desktop::startup_settings` | 2 129 |
| Wallpaper, Fonts, Lock screen, Installed apps | — nothing written | — |

**This is not a new decision. It is §815's second half, unstarted.** That
entry is an *operator* decision and says plainly: *"Everything you open from
a menu moves to the Settings app, and the shell's copies are deleted."* The
deletions happened — three panels went on 2026-09-07. The **moves** did not.
So the tree is currently in the worst of the three states C-Q6 offered:
neither copy reachable, one of them deleted.

**What makes it worth doing rather than deleting.** Deleting the shell's
copies is the cheap reading of 815 and it would leave eleven roadworks signs
where five features are. The operator picked the most expensive option
deliberately — "It is also the most work, which is why it was worth asking
rather than assuming" — and the expensive part is exactly this: the panels
have to be rebuilt in the Settings app's idiom before the shell's copies can
go.

**Why it is a port and not a copy.** `apps/settings` builds every page
through a `PageSink`, walked once by a `DrawSink` to paint and once by a
`HitSink` to hit-test, *"so the two can never disagree about where a row
is"*. The shell's panels each draw and hit-test in their own way. Moving one
means re-expressing it as a `build_*_page`, which is where the work is and
also where the value is: the target idiom is the one that cannot drift.

**How this was found, and the correction worth keeping.** The first pass
read the page *enum*, saw `Notifications`, `Power`, `DefaultApps` and
`StartupApps` listed, and concluded the shell's panels were duplicates ready
to delete under 815. Eleven thousand lines were about to be deleted on the
strength of a list of names. The enum is the sidebar, not the
implementation; `build_page`'s `_ =>` arm is the implementation, and it is
where four of those four go. A page that exists in a menu and a page that
exists are different things, which is the same mistake as counting a grep's
hits and calling them callers.

**~~Order to do them in. Power first~~ — corrected within the hour, and the
reasoning was backwards.** The paragraph here said to start with Power
because *"nothing in the shell consumes power settings, so the panel has no
second consumer to keep in step"*. That is not a reason to start; it is a
reason not to. **No second consumer turned out to mean no consumer at all.**

Checked, per this file's own standing rule that you look at what a module
talks to before wiring it:

| page | who reads the setting | verdict |
|---|---|---|
| Power | **nobody.** And there are *two* dead models: `power_settings.rs` has a `PowerConfig`, and so does `power.rs` | building it produces controls that change nothing |
| Default apps | **nobody.** The only mention of associations outside the panel is a line in `apps/explorer`'s module doc | same |
| Startup apps | **nobody** in the session's launch path | same |
| WiFi / Ethernet / VPN | **nobody** (checked 2026-09-14) | same |
| Notifications | the shell's pane holds `app_settings` in memory — a real reader, but not a persisted one | the closest to ready, and still a three-part job |

**So the blocker is not the port.** It is that these settings have nowhere
to be read from. The pattern this tree already uses for exactly that is a
third crate both halves depend on: `gui/appearance` with `ReloadAppearance`,
and `gui/inputsettings` with `ReloadInput` (control verb `0x14`). There is a
`gui/notifications`, but it is a **binary**, not a shared model.

So each of these is a three-part change and not a move: a settings crate,
a page that edits it, and a consumer that re-reads on a verb. Doing only the
middle part is how a panel full of live-looking controls that change nothing
gets built — which is strictly worse than the roadworks sign it replaces,
because the sign is honest.

**`power.rs` is worth its own line, because it also shows a blind spot in
the orphan scanner.** It is 2 880 lines and is *not* on the baseline — but
only three of its public items are reached (`PowerMenuRow`, `PowerMenuStyle`,
`render_power_menu`, the Start menu's power rows). The scanner asks whether a
*module* has any named item, so one reached item hides the rest of the file.
A second dead `PowerConfig` sat there in plain sight.

**Revised order.** Notifications first, because it is the only one with a
reader today, and because doing it builds the settings-crate-plus-reload-verb
chain that the other four will each need. The rest wait on that chain, or on
a consumer existing at all — and for Power that means a power manager, which
is not lane C's.

**The last unknown is closed, and it closes the entry: none of the four have
a reader.** The network row was the one row that had not been checked, on the
grounds that `net*/**` is lane C's own glob and a consumer might therefore be
lane C's to write. Checked 2026-09-14: the only `NetworkConfig` outside
`gui/desktop/src/network_settings.rs` is `apps/installer`'s, which is
*install-time* answers to a questionnaire and not a running machine's
configuration. Nothing under `net/`, `net80211/`, `netipc/`, `netproto/` or
`netring/` reads a network setting, and nothing could: those crates are
protocol and transport, with no daemon above them to own an interface.

So the chain the Notifications port built is available and unused, and all
four remaining pages are blocked on the same missing thing -- a service that
owns the hardware and re-reads its settings on a verb. Power needs a power
manager, network needs a network daemon, default apps and startup apps each
need something in the session's launch path. **None of those are lane C's**
(`services/**` and `init/**` are lane B's), so this entry is blocked rather
than deferred, and the honest state of the Settings app is eleven roadworks
signs of which ten are telling the truth.

**Do not "just build the page" for any of them.** The entry above already
says why and it is worth repeating at the bottom where the next reader will
be: a page of live-looking controls that change nothing is strictly worse
than the roadworks sign it replaces, because the sign is honest.
