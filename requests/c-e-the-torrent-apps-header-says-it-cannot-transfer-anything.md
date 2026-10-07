# C -> E -- the torrent app's header says it cannot transfer anything

**From:** Lane C. **To:** Lane E (`apps/torrent`).
**Filed:** 2026-10-07. **Status:** OPEN -- a module doc, nothing waits on it.

**In short:** `apps/torrent/src/main.rs` opens with a list of what the crate
does and then: "What it cannot do is **transfer anything**. The crate depends
on `guitk`, `oswindow`, `appearance` and `safeio`, and on no network at all:
there is no `std::net` here and no socket of any kind. The announce URL
`TrackerRequest::build_url` composes is never fetched, so no peer list comes
back, and a torrent with no peers makes no progress." That was true when it was
written. Since 3844f184e (the peer connection: handshake, framed messages,
piece assembly, over `TcpStream`) and bc56d4e02 (2026-09-26, "the window
downloads -- open starts a session, pause stops it"), it is not: the window
downloads. The doc understates the program, which is the direction that costs
most -- whoever plans from it believes there is nothing to try.

**What is asked:** read the header against the crate as it is now, and say
what it does and what it still does not (whatever of trackers, DHT, upload,
magnet resolution remains unbuilt), in the same change. The list above it was
pruned once already for the opposite reason (the note after it, on three
entries removed for overstating), so it has been read with care before; this
is the same care, the other way.

**How it was found:** `known-issues` TD-C-A-MODULE-DOC-IS-THE-ONE-CLAIM-NOTHING-CHECKS
proposes pointing `scripts/find-stale-admissions.py`'s pairs -- a capability
the code holds, a sentence denying it -- at `//!` blocks. A trial of that on
2026-10-07 over `gui/`, `apps/`, `userspace/` and `services/` found this one
crate's header among eleven sentences, the rest true; the entry records the
trial.
