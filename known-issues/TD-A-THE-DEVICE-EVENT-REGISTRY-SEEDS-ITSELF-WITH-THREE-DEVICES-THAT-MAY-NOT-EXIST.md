## TD-A-THE-DEVICE-EVENT-REGISTRY-SEEDS-ITSELF-WITH-THREE-DEVICES-THAT-MAY-NOT-EXIST (lane A, 2026-09-15)

**In short:** `fs::dmevent::init_defaults()` -- production, not a test --
populates the kernel's device-event registry with three hardcoded devices:

    /sys/block/sda              sda         Block   online: true
    /sys/class/net/eth0         eth0
    /sys/class/input/keyboard0  keyboard0

None is enumerated from hardware. All three are `String::from` literals in an
`alloc::vec!` inside `init_defaults`, and `online: true` is asserted for the
first regardless of whether any disk is present. A user listing devices through
the kshell `dmevent` command sees them on any machine.

**This is lane C's `apps/settings` finding, in the kernel.** They spent
2026-09-14 removing five pages of plausible records -- an adapter "connected at
192.168.1.100" on a system that cannot enumerate interfaces, three accounts with
`example.com` addresses. This is the same construction: a `Vec` of believable
records built in an init function. The eth0 entry is very nearly the same
invented device.

**The devpaths name a tree nothing serves**, which is how it was found.
`sysfs.rs` has **zero** references to `block` -- no `SysPath` variant, no
`classify_path` arm. So `/sys/block/sda` is a string in an event payload
describing a sysfs node that does not exist. In Linux a `DEVPATH` in a uevent is
a promise that the node is there to be opened; a consumer following ours finds
nothing.

**How it surfaced:** lane C's request stated "the tree is live:
`/sys/kernel/hostname`, `/sys/class`, `/sys/block/sda`". The first is served.
The third is only *mentioned* -- by this file. Grepping for the path finds
references and reads as evidence of a producer, which is the
documentation-versus-code confusion recorded in 938 turned up one level: here
the misleading text is a **string literal in production code**, which looks even
more like evidence than a comment does.

**The fix, and it is not simply deletion.** The registry should start empty and
gain entries when something real registers. Removing the seed makes the kshell
command print nothing on a machine with no enumeration, which is the honest
state and matches `design-decisions.md` §1006 -- a command that does not work is
deleted rather than kept as a stub. Deferred rather than done because the
kshell `dmevent` subcommands were written against a populated registry and
several of their self-tests will be standing on these three records as fixtures,
which is the fixture-is-the-defect mode in 937: emptying the list is how you
find out, and that wants its own change rather than riding on a boot-unblocking
commit.

**FIXING AN INSTANCE AND LEAVING THE CLASS — added 2026-09-15.**

Lane B put this better than I had: *"the detector's real value was not finding
`-l`; it was showing me I had fixed an instance and left the class."* They had
implemented `patch --ignore-whitespace` after I reported it, and the detector
then found `-N`, `-F` and `-Z` in the same file, under the same doc comment
whose argument they had just spent a commit refuting.

It is worth writing down because it is the shape of both our days, and it has a
cause rather than being carelessness: **the instance is easy to see precisely
because someone wrote down the reasoning that covers the class.** A rationale
for leaving one thing inert is a rationale for leaving all of them, so the
moment it is refuted, every sibling it covered becomes a finding — and nothing
announces that. The same applies here: six fabricated Settings pages were fixed
before anyone asked how many more there were, and the answer was 63.

The check that follows from it costs nothing: **when a fix refutes a written
rationale, grep for the other things that rationale covered** before closing
the task. For `patch` that was the three other options under the same comment.
For the Settings pages it would have been `SettingsState::new` — where all five
sat together, in one constructor, visible in a single screen.

**A better axis than mine, also from lane B.** They ranked their 110 by whether
the option is advertised in the program's own `--help`: 50 are, and 24 of those
are read by nothing at all. That is mechanical where my ordering by "what
believing it costs" needs a judgement per row, and it gets at the same thing —
a promise in `--help` is the program telling the user what to believe. The GUI
analogue is not `--help` but the window itself, and by that measure all 63 here
are advertised, which is why the ordering here has to be by consequence.

**THE CHECK WORKED IMMEDIATELY, AND FOUND A BETTER INSTRUMENT.**

Applying the rule above to my own day — grep for the other things the refuted
rationale covered — turned up something the name sweep had no way to see.
`apps/photomanager`'s `seeded_library` carried the comment *"so the first window
is not an empty grid"*. That sentence is not unique to it:

* `apps/videoplayer` — *"Sample content, so the first window is not an empty
  black rectangle."*
* `apps/devicemanager`, `apps/netmanager`, `apps/partmanager`,
  `apps/remotedesktop`, `apps/sysinfo`, `apps/vpnmanager` — all six declare it
  in their **module-level `//!` documentation**, in near-identical words:
  *"…through Slate OS syscalls; stubbed with representative data for initial
  development."*
* `apps/netscan`, `apps/speedtest`, `apps/procexplorer`, `apps/rssreader`,
  `apps/sysmonitor` carry the same admission in other forms.

**CORRECTION, same day: the count above said "ten apps in total" and the real
figure is 32.** Recounted with the pattern written out properly rather than
typed from memory: **14** apps carry a module-level `//!` self-declaration and
**24** carry one in a function or comment, for a union of 32. The ten I first
listed were the ones the first grep happened to surface.

The correction is worth more than the number. I published a count from a
narrower pattern than the one I had just argued was the better instrument, and
nothing would have caught it — a count has no test. What caught it was
re-running the search before relying on the figure again, which is the only
check available for a number in prose.

**This is a better detector than the one that found the 63**, and it is worth
saying why rather than just switching to it. A name grep asks whether somebody
*happened to name a function candidly*; this asks whether the file *declares
itself stubbed*. The second is evidence rather than a hint — the code is
stating the fact, not hinting at it — and it cannot be evaded by renaming a
function, which was lane B's whole objection to the 63 being treated as a
count.

**And it is exactly the `patch -l` shape at application scale.** Every one of
these declares the stub in a module doc that only a maintainer reads, while the
window shows the data as though it were the machine's. `apps/sysinfo` is the
one I would look at first on that basis: a system information tool is read
precisely when someone wants to know what hardware they have.

None of the ten are fixed. They are listed here so that the next sweep starts
from the self-declarations rather than from the names.
