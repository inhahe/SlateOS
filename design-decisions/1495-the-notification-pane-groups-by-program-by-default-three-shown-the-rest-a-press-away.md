## 1495. The notification pane groups by program by default: three shown, the rest a press away

**Date:** 2026-10-07 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** the notification pane listed every notification it kept in
one long run, split only by when it came (Today, Yesterday, This Week,
Older). A chatty program -- forty messages from a chat client -- pushed
everything else out of sight. Now the pane groups each program's
notifications under its name, the program heard from last at the top: its
newest three show, "Show N more" brings the rest and "Show fewer" puts
them away, and a cross on the heading clears that program's notifications
at once. This is how Windows' notification pane -- the one `design.txt`
asks for ("a notification pane like on Windows") -- arranges them. The old
arrangement by time is still there as a setting (`pane.grouping: time` in
`notifications.yaml`); the Settings app's control for it is asked of lane E.

**Where:** `gui/notifsettings` (`Grouping`, `NotifSettings::grouping`,
`pane.grouping` in the file), `gui/desktop/src/notif_pane.rs` (`rows`,
`RowKind`, `programs_heard`, `clear_program`, `toggle_more`,
`render_program_heading`), `gui/desktop/src/notif_pane/accessible.rs`
(`PanePart::Group`, `GroupClear`, `GroupMore`), `gui/desktop/src/lib.rs`
(`load_notification_rules`, `poll_notification_rules`); asked of lane E in
`requests/c-e-a-choice-of-how-the-notification-pane-groups.md`.

| Choice | Instead of | For | Against |
|---|---|---|---|
| **By program is the default** | by time, as the pane was | `design.txt` points at Windows' pane, which groups by program; a program that floods the pane is the case grouping is for, and by time it buries everything else. | A user who knew the old pane finds it rearranged; the old order is one setting away. |
| **Programs ordered by their newest notification** | alphabetically, or by first heard | What came last is what the user opened the pane to see -- the order by time keeps that, and by program should too. | A program's place moves as it is heard from. |
| **Three newest shown, the rest behind "Show N more"** | every one shown; or one shown | Three is enough to see what a program has been saying and few enough that one program cannot fill the pane; Windows' and Android's panes collapse a program the same way. | A fourth, older notification takes a press to reach. |
| **The cross on the heading always shown** | shown under the pointer, as a card's is | Clearing a program is what its heading is for, and a cross that appears only under the pointer is one a user without a pointer -- or one who has not hovered -- never finds. | One more cross on the screen per program. |
| **Expansion forgotten when the pane closes** | kept for the session | Each opening starts from the same short list, which is what makes it a glance. | Reopening to read a long thread takes the press again. |
| **The setting written to `notifications.yaml`, read by the shell first** | a control first | The behaviour exists before any control, so no setting is ever shown that nothing reads (the order `known-issues` set for the four deleted behaviours). | Until lane E adds the control, a user changes it by editing the file. |

**If reversed:** `Grouping::default()` back to `ByTime`; the rows, the
headings and the setting stay, for whoever wants grouping by program.
