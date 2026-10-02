## TD-C-A-MODULE-DOC-IS-THE-ONE-CLAIM-NOTHING-CHECKS -- METHOD 2026-09-17

**In short:** every crate opens with a `//!` list of what it does, and nothing
anywhere checks that list against the code. Three of them were found wrong in
one day. The two scanners that catch this class —
`find-claimed-acts` and `find-stale-admissions` — read strings the program
*draws*, so a module doc is outside both by construction. It is the one claim
in the tree with no instrument at all.

**Date:** 2026-09-17. **Lane:** C.

**The three, and the shape of each.**

| crate | what the list said | what was true |
|---|---|---|
| `archivemanager` | four capabilities | none of the four backed |
| `torrent` | "Tracker announce/scrape (HTTP)", "Bandwidth throttling", "Peer discovery" | no socket in the crate; a `BandwidthLimiter` held in fields nothing reads |
| `pdfviewer` | "content streams are not interpreted"; 1.5 files "refused by name" | 184519 text runs across 20 of 20 documents |

**The third is the instructive one, because I wrote it.** The first two
*overstated*: claims the code could not back, and I removed them in the
morning. The third *understated*, and I committed it in the afternoon, in my
own crate, hours after fixing the other two — text that was accurate when
written and made false by the work that followed. I then did it a second time
in the same crate with `can_open()`.

So this is not carelessness about someone else's code. It is text you write
and then outgrow, and **the author is the worst-placed person to notice**,
because they remember writing it and not what it said.

**Understating is the worse direction**, which is the same finding
`find-stale-admissions` records for drawn text: an overstatement is caught the
first time somebody tries the feature, and an understatement is believed, so
nobody tries. `apps/whiteboard` told people their work could not be saved
while Ctrl+S was writing an SVG.

**Why it matters more than a stale comment usually would.** A feature list is
what the next reader plans from. The stranded-serialiser roadmap item exists
because `apps/spreadsheet`'s module doc described its own gap *accurately* and
so turned into a task; an inaccurate one misleads in exactly the same
mechanism, and costs the same reading to discover.

**Proper fix, and why it is hard.** A checker cannot know whether "reads
`.torrent` files" is true. But the narrower half is reachable and is where
every instance above sat: a module doc that **denies** a capability the crate
now holds. That is `find-stale-admissions`' predicate, pointed at `//!` blocks
instead of string literals — "not interpreted", "cannot", "no X here",
beside a symbol that now does it. It would have caught the `pdfviewer` and
`torrent` entries and missed `archivemanager`'s, which is a real fraction of
the class rather than all of it.

**Until then**, the rule that would have prevented all three: when a change
gives a crate a capability, re-read its module doc in the *same* change. Every
one of these was introduced by an edit that added something and left the
header alone.
