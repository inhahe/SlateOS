## 856. A settings page is built when something obeys it, not when something stores it

**In short:** the Settings application has 29 pages and nine of them were
blank "under construction" signs. Choosing which to fill needs one question:
if this page showed the setting, would the setting be true? "Default Apps"
passes — the file manager really does run the program named there — so that
page is now built and shows the real list. "Startup Apps" fails: the system
keeps a list of programs to run when you log in, but nothing in the system
ever runs them, so a page listing them would be telling the user those
programs start when they do not. It stays blank on purpose.

**Date:** 2026-09-16. **Lane:** C. **Decided by:** Claude (autonomous).

**What happened.** Both pages were picked up in the same sitting and they
looked like the same job: read a list the system already publishes, draw a row
per entry. Startup Apps was most of the way written — a module reading
`/proc/autostart` through `procinfo`, with tests — before the comment on the
`_ =>` arm in `apps/settings/src/main.rs` stopped it. That comment said the
page was blank deliberately, because **nothing launches a startup entry**:
three user interfaces already manage that list and no launcher reads it.

The comment argued only against giving the page *controls*, and that gap is
exactly what let the near-miss happen. A read-only list looks like the safe
version of the same page — it promises nothing, changes nothing, merely
reports. It is not safe. A row reading `Backup — Enabled, Session` states that
the program runs at session start. Nothing runs it. That is the same
fabrication as a toggle that saves a value nobody reads, **relocated from the
control to the label**, and it is worse in one respect: a dead toggle at least
does not assert an outcome.

**The test that separates them.** Not "is there a real file to read?" — both
have one, published by the kernel or written by another program. The question
is *does a consumer exist that makes the row true?*

| Page | Store | Consumer | Verdict |
|---|---|---|---|
| Startup Apps | `/proc/autostart`, real | none — every reader only displays it | placeholder |
| Default Apps | `settingsfile` group `fileassoc`, real | `apps/explorer` spawns what it names, on every open | built |

Checked rather than assumed: every use of the autostart table in the tree is a
reader (`apps/sysinfo` displays it, `procfs.rs` publishes it, `startupopt.rs`
counts it against a cap). For associations the consumer is four lines of
`explorer::open_entry`, which looks the extension up and calls
`Command::new(program).arg(path).spawn()`.

**The alternative, and why it was rejected.** Startup Apps could have been
built as a list that says plainly underneath it that nothing starts these
programs. That is honest in the note and dishonest in the rows, and the rows
are what a person reads. A list under the heading "Startup Apps" implies the
programs start whether or not a caption denies it; the denial has to be read
and believed, the implication does not. The place where that table *is*
honestly reported is `apps/sysinfo`, a system-information program, where it is
presented as data about the machine rather than as behaviour the user is
configuring. It is already reported there.

**Cost of being wrong in each direction.** A page withheld too long is a
missing feature that someone notices and asks for. A page shipped too early is
a false statement about the user's own machine that nobody can detect from the
screen — it looks exactly like a working one. The two errors are not
symmetric, so the rule is biased: build the page when the consumer lands, and
leave the comment explaining what is waited for. The Startup Apps arm now
carries that comment, extended to cover the read-only case so the next reader
does not re-derive this.

**Where it bites.** `apps/settings/src/main.rs`, the `_ =>` arm of
`build_page`, and `apps/settings/src/defaultapps.rs`. Eight pages remain
placeholders — Ethernet, Fonts, InstalledApps, LockScreen, Power, StartupApps,
VPN and WiFi — and each gets the same question asked of it before it is built.
The one that will most likely pass next is Fonts, whose consumer is the text
renderer, and the one that will most likely fail is Power, whose brightness
control has setters and no door.
