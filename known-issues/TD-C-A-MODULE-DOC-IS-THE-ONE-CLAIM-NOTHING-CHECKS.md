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

### Tried 2026-10-07: the pairs pointed at `//!`, and what they found

A throwaway scan (not in the tree) ran `find-stale-admissions.py`'s three
pairs over every crate's `//!` blocks, cut into sentences, in `gui/`, `apps/`,
`userspace/` and `services/`.

- **With the drawn-text vocabulary as it is: 17 sentences in `gui/` and
  `apps/`, 16 of them true.** Module docs say "cannot read" and "cannot be
  read" to describe how unreadable *input* is handled -- "a rule this cannot
  read is not applied at all" (`windowrules`), "a declaration that cannot be
  read ... is dropped" (the toolkit's CSS) -- which is the opposite of
  denying a capability. In a banner those words are a claim; in a module doc
  they are a contract.
- **Dropping that family -- keeping "no filesystem access", "no network
  access", "never fetched", "no `std::net`", "no socket", "no way to run":
  11 sentences over all four trees, 9 of them true.** What remains is history
  ("until then it had no socket", `apps/ircclient`, `apps/netscan`) and two
  error-handling sentences in `userspace/coreutils` ("a file that cannot be
  opened does not stop the run").
- **The one finding, both times: `apps/torrent`'s header**, two sentences --
  "no `std::net` here and no socket of any kind" and "the announce URL ... is
  never fetched" -- in a crate that has opened peer connections over
  `TcpStream` since 3844f184e and downloaded from its window since
  bc56d4e02. Lane E's: `requests/c-e-the-torrent-apps-header-says-it-cannot-transfer-anything.md`.

So the pairs *do* work on module docs, at one real finding in eleven, and
what makes the rest noise is recognisable: past tense, and a subject that is
input ("a file", "a line", "a rule") rather than the program. A version worth
keeping in the tree would skip sentences in the past tense and keep the
eleven-word vocabulary above, and list what it still reports, as its siblings
do. Not built: one finding in four trees does not yet pay for a fourth list of
known exceptions to keep current.
