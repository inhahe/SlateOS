## TD-C-SETTINGS-THAT-ONLY-CONFIRM-THEMSELVES -- LANE C DONE 2026-09-16

**Re-read 2026-09-16: all three rows this entry left "real, open" are closed,
two of them by work done after it was written.** Checked one at a time rather
than trusted:

| row | state on re-reading |
|---|---|
| `videoplayer` screenshot settings | **fixed.** The options are drawn with the fact that none can be taken, and `the_screenshot_options_say_no_screenshot_can_be_taken` pins it. |
| `fontmanager` default size | **fixed.** `SETTINGS_NOT_CARRIED` is drawn directly under the value, deliberately there rather than at the foot of the panel, because "the numbers above are what read as confirmation". |
| `settings/remote.rs` | **superseded.** `remote::` is named nowhere in `apps/settings/src/main.rs`, so the page cannot be opened and its values confirm themselves to nobody. Whether it is wired up or deleted is open-questions C-Q17, which is the operator's. A disclaimer on an unreachable page would be a notice nobody can read. |

**Two rows of the `gui/` table above are now moot**: `backup_settings` was
deleted on 2026-09-16 (see
`TD-C-THREE-MORE-SHELL-SETTINGS-MODULES-ARE-REACHED-BY-NOTHING`), and
`power_settings` survives only because it is kept deliberately
(`TD-C-POWER-SETTINGS-IS-KEPT-ON-PURPOSE-DO-NOT-SWEEP-IT`). The disclaimers
they gained were not wasted -- they were true while those pages existed -- but
do not go looking for them.

**Why this note exists at all.** The entry says its table is "what makes the
second reading a lookup instead of an investigation", and on the second reading
it was not: three rows said open and two of them had been fixed the same day
the entry was written. A tracking file that lags is worse than one that is
missing, because it is trusted. Marking work done is part of doing it.

**In short:** across this tree there are 78 settings that a program reads for
exactly one purpose: to show the value back to the person who set it. Nothing
else ever looks at them. Change the setting, restart, read it in the window or
the banner, and it will agree with you -- which is the only check available,
and it passes. 43 of the 78 are in this lane.

**Date:** 2026-09-15. **Lane:** C (35 of the 78 are lane B's).
**Decided by:** Claude (autonomous) -- filed, not yet fixed.

**Status 2026-09-15, later the same day: all of `gui/` is answered, and
`apps/mediaconvert`.** Eight pages now name what they do not apply:

| page | what it was confirming | what it says now |
|---|---|---|
| `backup_settings` | a weekly backup schedule | nothing runs backups automatically, and these are not saved; run `backup` |
| `power_settings` (Advanced) | screen brightness for AC and battery | the kernel can set brightness and does not expose it -- request filed with lane A |
| `power_settings` (Battery) | a critical-battery threshold | nothing watches the battery against it |
| `sound_settings` (Input) | microphone gain and monitor volume | this system has no audio at all |
| `update_settings` | active hours for restarts | nothing installs updates |
| `datetime_settings` | an NTP sync interval | the time client chooses its own, as the protocol requires |
| `startup_settings` | a maximum startup delay | startup order is decided before this window exists |
| `storage_settings` | a disk-cleanup threshold | nothing watches free space or deletes anything |
| `apps/mediaconvert` | quality, codecs, metadata stripping | nothing reads these except this panel |

**The scanner still reports every one of them, and will forever.** The repair
is not to delete the field; it is for the program to stop implying the value
took effect. The field is still read only into output afterwards, so the
finding stands -- correctly, because "this value reaches the operator and
nothing acts on it" is still true. A checker that went quiet when a disclaimer
appeared would be measuring the disclaimer rather than the defect. This table
is what makes the second reading a lookup instead of an investigation.

**Two of the nine said something more specific than "not implemented", because
that phrase would have sent the reader the wrong way.** Brightness is
*unreachable*: `kernel/src/fs/brightness.rs` has working setters whose only
callers are its own tests, and no syscall or `/sys/params` node reaches them --
so "not implemented" would invite someone to implement it in `gui/`, where it
cannot be done. The NTP interval is inert *twice*: nothing carries it to
`ntpd`, and `ntpd`'s poll interval is the protocol's own and would not honour
an arbitrary number if something did -- so a reader told only the first half
would file a request whose answer is no.

**`apps/` triaged in full, 2026-09-15.** Twenty-four fields across ten apps.
Reading them one at a time turned up three shapes the scanner cannot
distinguish, and two finds bigger than the rows that pointed at them.

| app | verdict |
|---|---|
| `mediaconvert` (6) | **fixed** -- panel says the settings are not applied |
| `diskimager` (4) | `block_size` x2 **fixed** (the label was *wrong*, not inert); `output_path` a false positive; `format` latent, see below |
| `netmanager` (3) | **already covered** -- banner drawn unconditionally every frame |
| `ircclient` (2) | **fixed**, and the rows were a pointer to something worse |
| `remotedesktop` (2) | **already covered** -- banner drawn after the background, unconditionally |
| `videoplayer` (2) | **real, open** -- screenshot settings for a capability already removed |
| `settings/remote.rs` (2) | **real, open** -- no disclaimer of any kind |
| `fontmanager` (1) | **real, open** -- a default font size nothing carries anywhere |
| `netscan` (1) | **already covered** -- "no network access at all" |
| `weather` (1) | **already covered** -- `CANNOT_FETCH_LINES` |

**The three shapes worth naming, because the scanner reports all three
identically:**

1. **Already covered by a program-level disclaimer whose scope reaches the
   value.** `netmanager`, `remotedesktop`, `netscan`, `weather`. The test is
   not "is there a disclaimer somewhere" -- `mediaconvert` had one and still
   needed a second. It is whether the existing disclaimer covers *what the
   value would need in order to be true*. A VPN server address needs a
   network, and the banner says there is none. A quality setting needs a
   conversion, and "the queue will not run" does not say there is none.
2. **A record kept so a later message can name it.** `diskimager`'s
   `output_path` is stored by `start_create` alongside being passed to the
   writer, purely so "Image created: {}" can name the file that really was
   written. Deleting it would make the message worse. Same shape as lane B's
   `lp`, which parses a printer name it never uses so its refusal can say
   which printer it refused.
3. **Not inert but WRONG.** `diskimager`'s `block_size` said 4096 while every
   copy used a 1 MiB constant. That is a different defect with a different
   fix: an inert setting gets a notice, a false statement gets corrected.

**A latent trap left by the same pattern, recorded so it is not re-derived:**
`diskimager`'s `CreateOptions::format` offers Raw / ISO / **GzipCompressed**
and is read only to draw a label. Nothing branches on it and nothing can set
it, so today it says Raw and the copy produces raw -- correct by coincidence.
The day anyone adds a picker, choosing GzipCompressed would produce a raw
image named `.img.gz`, and the failure would surface much later as a corrupt
archive. It should go the way of `block_size`: the label drawn from what the
copy actually does.

**Still open:** `userspace/`'s 28, which are lane B's.

**Why this is worse than a setting nothing reads at all.** A dead setting is
silent, and silence at least does not argue. An echoed setting produces
positive evidence that it took effect, in the program's own voice, at the exact
moment the operator is checking. It is the same shape as every fabrication
cleared out of this tree this week -- **the observation that would falsify the
claim is the same observation that confirms it** -- and this is the most
persuasive form of it, because the confirming observation is the program's own
output rather than a number it made up.

Lane B named the case first, in `userspace/logind`:

> `IdleActionSec` is in this list even though a field-level scan calls it READ,
> and that difference is the point. Its only reader is the startup banner,
> which prints `idle_timeout=600s` back at the operator -- so the one thing the
> setting does is CONFIRM ITSELF. A scanner asking "is this field ever read?"
> cannot see that, because printing is a read; the question that finds it is
> "does anything ACT on it?".

**How they are found:** `scripts/find-echoed-settings.py`. Reports, does not
gate. The rule is per-**struct**, not per-field, which is what makes it usable
at all: a `--show-config` dump reads *every* field into a print and is honest,
so the finding is a field read only into output **while its siblings are read
by code that acts**. That exonerates a dump wholesale without exonerating a
straggler inside one.

**The count is split three ways and the splits matter more than the total:**

| | count | meaning |
|---|---|---|
| reaches a `println!`/`write!` | 24 | shown to a person for certain |
| built with `format!` only | 54 | shown *if* it reaches a screen |
| stranded (reported separately) | 23 | the formatter has no caller -- a different defect |

That last row is why this is not simply 101. A field read only into a print
has two explanations wanting opposite fixes: the print runs and misleads, or
the print sits in a formatter nothing calls. The second is
`find-stranded-serialisers`' finding and counting it here would be two tools
reporting one defect.

**And `[format]` means different things in different halves of the tree.** For
a command-line program it is genuinely ambiguous -- `userspace/curl`'s
`user_agent` is formatted into a request header, `userspace/objdump`'s `radix`
picks a number base, and neither is shown to anybody. A GUI app has no stdout:
every label it draws is `format!`-built and handed to the toolkit. So under
`apps/` and `gui/`, `[format]` means **shown**.

**This lane's 43, by file:**

| file | n |
|---|---|
| `apps/mediaconvert` | 6 |
| `gui/desktop/network_settings.rs` | 5 |
| `gui/desktop/power.rs` | 5 |
| `apps/remotedesktop`, `apps/settings/remote.rs`, `apps/videoplayer`, `gui/desktop/sound_settings.rs`, `gui/desktop/update_settings.rs` | 2 each |
| `apps/fontmanager`, `apps/netscan`, `apps/paint`, `apps/weather`, `gui/desktop/datetime_settings.rs`, `gui/desktop/startup_settings.rs`, `gui/desktop/storage_settings.rs` | 1 each |

**One verified by hand, because a count nobody checked is the thing this lane
has spent the week finding.** `apps/mediaconvert`'s settings panel offers
Quality, Strip metadata, Preserve aspect, and video and audio codec. It draws
all five. Nothing acts on any of them, because that program cannot convert
anything -- which it already says, in three lines, one of which warns against
deleting an original on the strength of its queue. The settings panel is the
half that was not covered by those three lines.

**The fix, per program, is the one lane B already applied to `logind` and
`tuned`:** keep the field, and have the program name the settings the operator
actually set that it does not honour. Not a blanket "this is a mock" banner --
a specific list, built from what was parsed, silent when nothing inert was set.
That distinction is what makes it useful rather than noise.

**Order of work:** `apps/mediaconvert` first, as the verified one and the
largest single cluster in this lane. `gui/desktop`'s four settings pages next,
since they are the operator's actual settings surface and the place where the
confirmation is most convincing.

**Until then:** nothing degrades, and nothing is at risk of data loss from
these specifically. What is wrong is that a person can set a value, check it,
and be told yes.
