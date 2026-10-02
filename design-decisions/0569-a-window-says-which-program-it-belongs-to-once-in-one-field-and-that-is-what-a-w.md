## 569. A window says which program it belongs to, once, in one field — and that is what a window rule matches

**Date:** 2026-08-26
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** The Settings panel has always let you write rules like *"the
terminal should reopen where I left it"*. None of them could ever fire, because
a rule matched on a "process name" or a "window class" and no window on this
system had either — the compositor knew a window's title and nothing else about
what program it was. So a window now carries an **app id**: a short name the
program chooses for itself (`terminal`, `slateos-editor`) that is the same for
every window that program opens, and never changes while it runs. Rules match
on that. The two old ways of naming a program are gone, replaced by one.

### The problem

`window_rules.rs` offered five ways to match a window: exact title, title
substring, `ProcessName`, `WindowClass`, and `Any`. The three default rules
shipped in every new profile used `ProcessName` twice and `WindowClass` once.
All three were dead on arrival — `WindowInfo`, the compositor's description of
a window, carried an id, a pid, a title, a rectangle, some state flags and a
workspace number. Nothing that answers "which program is this?". The engine's
`evaluate` took a process name and a class as arguments and every conceivable
caller would have had to pass `""` for both.

### The decision

One field, `app_id`, declared by the client when it creates the window and
reported back in every window list. `MatchCriteria::ProcessName` and
`MatchCriteria::WindowClass` collapse into a single `MatchCriteria::AppId`.

**Why not two fields, the way X11 has them.** `WM_CLASS` carries an *instance*
and a *class* — nominally "this copy" and "this kind" — and thirty years of
practice is that nobody can remember which is which, applications set them
inconsistently, and rule-writing tools show both and let the user guess. Wayland
looked at that and shipped one `app_id`. The duality is not information; it is a
question every client answers differently. Copying it into a system that had
neither would be importing a known defect for compatibility with nothing.

**Why not derive it from the process name.** A process name is a fact about the
kernel's process table, not about the window, and the shell would have to ask a
service it does not talk to in order to learn it. Worse, it is the wrong grain:
one browser process opens twenty windows of what the user thinks of as
different things, and twenty terminal windows may be one process or twenty.

**Why not derive it from the title.** A title answers *which document*; an app
id answers *which program*. They change on different schedules — the title
changes as the user works — and deriving one from the other would give each of
a program's windows a different id, which is precisely the thing an app id
exists not to do.

**Why it is fixed at creation.** No request changes it. A program able to rename
itself mid-session could walk out from under a rule the user wrote about it:
"make the chat window skip the taskbar" would be defeated by the chat program
calling itself something else. Fixing it at creation costs a program nothing —
it knows what it is before it opens a window — and removes the whole class of
evasion.

**Why an empty id matches nothing.** A window may decline to name itself, and a
rule may be written with the value box untouched (the settings panel lets you
press Save on an empty string). Both are the empty string, and the tempting
reading — "empty means unset, so match anything" — is wrong in both directions.
An unnamed program is not *every* program; a rule about no program is not a
rule about the whole desktop. One stray Save would otherwise rearrange every
window on the screen.

**Why it is advisory and never a security claim.** The client declares it, so a
client can lie. Nothing may gate a capability on it. `WindowInfo::pid` comes
from the connection and cannot be forged, and stays for exactly the cases where
the answer has to be true rather than merely useful. An app id is for *the
user's* rules about *the user's* programs, and a program that lies about its
name to escape the user's own window rule has achieved nothing.

### What it costs

`CONTROL_VERSION` 3 → 4 and `WINDOW_LIST_VERSION` 3 → 4, because both records
grew a length-prefixed string. The three default rules are rewritten; the two
that named processes now name app ids, and the third — "Dialogs: no resize",
keyed on a window class — had no honest translation at all, because a dialog is
not a program. The compositor knows dialogs by `Layer`, which this engine does
not match on. It is replaced by a rule that both matches something real and can
be carried out: the shell's own four surfaces say `slateos-shell` and are not
applications.

The config-file spellings `process:` and `class:` are replaced by `app:` with
no compatibility shim, and an unrecognised criterion is now a **refusal** rather
than a silent fall-through to `Any`. That is the sharper half of the change: a
rule that silently widens from one program to every window is far worse than a
rule that fails to load, because the file still says `process:vim` while every
window on the desktop is being made always-on-top. The engine had no callers, so
no user could have such a file — but the parser will see one the first time a
config outlives a version bump.

### Where it lives

`gui/remote/src/control.rs` (`WindowSpec::app_id`), `gui/remote/src/window_list.rs`
(`WindowInfo::app_id`), `gui/window/src/app.rs` (`App::app_id`, defaulting to the
executable's stem, lower-cased), `gui/compositor/src/lib.rs` (`Window::app_id`),
`gui/desktop/src/window_rules.rs` (`MatchCriteria::AppId`),
`gui/desktop/src/lib.rs` (`ManagedWindow::app_id`).
