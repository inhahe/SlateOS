## TD-C-THE-NOTIFICATIONS-PAGE-HAS-NO-PROGRAMS-TO-LIST

**Date:** 2026-09-14. **Lane:** C.

**In short:** the Settings application's Notifications page works, and on a
real machine it will be empty. It lists the programs the user has a rule
for, and nothing ever creates the first rule. The desktop is the only thing
that sees which programs send notifications, and it has no way to tell the
Settings application — they are separate processes and there is no verb for
it.

**Where:** `apps/settings/src/main.rs` — `build_notifications_page` renders
`self.notif.settings.apps`. `gui/desktop/src/lib.rs` — `DesktopShell::notify`
is where a program first becomes known.

### The obvious fix was tried and is wrong

Have `notify` record a default rule for a program it has not seen, and save
the file. Written, and it fails for a reason worth keeping:

**It writes a configuration file from the notification path.** Every test
in `gui/desktop` that posts a notification — and many do — would write
`notifications.yaml` into whatever `XDG_CONFIG_HOME` is set at that moment.
For a test not wrapped in `with_scratch_config` that is *the developer's own*
`~/.config/slateos/notifications.yaml*. For one running while another test
holds a scratch turn, it is that test's fixture.

It was caught by the second kind: `a_rule_written_to_the_config_file_lets_
that_program_through` passed alone and failed in the full run. A test that
passes in isolation and fails in company is shared state, and the shared
state here was a real file on a real disk.

This is the same family as
`TD-C-A-TEST-THAT-WRITES-TO-AN-ABSOLUTE-POSIX-PATH-WRITES-TO-THE-DEV-DRIVE-ROOT`,
which has a whole script guarding it. The lesson generalises further than
either: **a settings write does not belong on a path that runs in response
to something a program did.** It belongs on a path that runs in response to
something a *person* did.

### Update, same day: something does create rules, and it is the right
### something

The notification pane's per-app switch now writes one. It always reported
the change; the shell discarded the report. Applying it means a user who
silences a program **where they notice it** — in the pane, on the
notification that interrupted them — gets a rule in `notifications.yaml`,
and the Settings page lists it.

So the page is empty only for someone who has never touched the switch,
and it fills with exactly the programs they have an opinion about rather
than with every program that has ever spoken. That is arguably the better
list: you set a rule where the problem is and review it in Settings.

**This is the write that is safe.** It is the same file the rejected fix
wrote, from the same process, and the difference is the whole point of the
rule this entry states: a person clicked a switch, so a settings write is
what should happen. The rejected version ran on the path that *receives* a
notification, where the trigger is another program and a test suite posting
notifications becomes a test suite writing configuration files.

**So `SubscribeNotifiers` is no longer urgent, and may not be wanted.** The
case for it was an empty page; the case against was always that the list of
programs that notify you is not public. What is left is the narrower
question of whether Settings should be able to add a rule for a program the
user has not met yet — which is a feature request, not a defect, and one
the answer above may make unnecessary.
### What the fix actually needs

The list of programs that notify is *live desktop state*, like the window
list and the tray. It is not a setting, and pushing it through a settings
file to get it into another process is using the wrong pipe — which is
precisely why the wrong pipe leaked.

The shape the tree already has for exactly this is a subscription: the
compositor carries a list, a shell or a settings application asks for it,
and it is pushed when it changes. `SubscribeWindowList` and
`SubscribeTrayIcons` are both this, gated through `require_shell`. A third
would be `SubscribeNotifiers` — the programs that have sent a notification
this session — and the Settings page would list the union of that and the
rules already in the file.

**Blocked on nothing, and deliberately not started here.** It is a protocol
addition with a privilege question attached (the list of programs that
notify you is not public), and it should begin a session rather than end
one. What is above is the whole of the preparation: the fix that looks
obvious, the measurement that killed it, and the shape the tree already uses.

**Meanwhile the page is correct and honest.** With no rules it says
"Programs appear here once they have sent you a notification", which is
true, and describes the feature this entry is about rather than pretending
to be finished.
