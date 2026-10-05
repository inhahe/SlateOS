### [E] The torrent client transfers nothing: it has no tracker or peer transport -- 2026-09-25
**Status:** FIXED for downloading, 2026-09-26 (lane E) -- `apps/torrent/src/`
`tracker.rs`, `peer.rs`, `storage.rs`, `session.rs`, wired in `main.rs`. OPEN,
lane E's: uploading, incoming connections, the end of a download (no endgame),
magnet links (BEP 9/10), DHT -- listed below. On SlateOS itself the whole of it
waits on `std::net` reaching lane D's sockets, which nothing here has tried.

**In short:** the torrent client downloads now. Opening a `.torrent` starts it:
the trackers are asked for peers (UDP or plain HTTP), each peer gets a thread,
every piece is checked against its SHA-1 before a byte of it is written, and the
window shows the pieces, peers, trackers and speed as they come. Pause stops it,
Resume carries on from what is on disk. It does not share what it fetches.

**What was built, in the order it was tested:**
1. `.torrent` read as it is (`b8f43aba2`): the info hash over the file's own
   bytes (it was the re-encoded dictionary's -- a torrent written non-canonically
   got a hash no tracker knows); paths kept as bytes and checked a part at a
   time, so none can climb out of the save folder (a non-UTF-8 part used to be
   dropped silently); a file entry without a length an error (it used to be
   skipped, shifting every later byte into the wrong file); bencode nesting
   bounded. The tree's `sha1` crate replaces the crate's own copy.
2. `storage.rs`: pieces mapped onto the files they span; only whole, checked
   pieces written; a symbolic link below the save folder refused; what is
   already on disk and whole found (`have`), so a restart fetches only the rest.
3. `tracker.rs`: HTTP (deadline, 1 MiB cap, chunked, redirects to `http://`
   only, BEP 7 IPv6 peers) and UDP (BEP 15; a reply with another transaction
   number ignored). `https://` refused with the reason.
4. `peer.rs`: the handshake, framed messages that survive a read timeout
   mid-message, and a piece assembled from only the blocks asked for.
5. `session.rs`: a coordinator thread, a thread a peer, a shared rarest-first
   picker; a bad piece fetched again and a peer dropped after three; a snubbing
   peer dropped after a minute; events to the window.
6. The window: open starts, Space/Pause/Resume stop and start a session,
   removing stops it (and, when asked, deletes the torrent's own files), ticks
   apply the events, the speed is sampled a second at a time, a finished
   download is Complete -- not Seeding, since nothing is uploaded -- and the
   notice says what the client does not do. The simulated swarm the window used
   to download from, and the helpers only it used, are gone.

Tested byte for byte against a tracker and seeding peers run in threads on
loopback (a download, one with a lying peer beside an honest one, a restart
with pieces already on disk, a stop, an unwritable folder, the whole thing
through the window). The tests cannot reach the network: in them a tracker
anywhere but loopback is refused before its name is looked up, and the default
save folder is a temporary one.

**Still to do:**
- **Upload.** Requests are unanswered and no peer is unchoked. A client that
  only takes is tolerated by swarms but is poor manners, and some private
  trackers drop it. Needs a choking algorithm and a listener.
- **Incoming connections.** Nothing listens on `listen_port`, which is still
  announced; peers that try it fail. Belongs with upload.
- **Endgame.** The last piece waits for the peer holding it, or for its silence
  to time out after a minute; clients ask a second peer for the same blocks.
- **Magnet links** (BEP 9/10 metadata exchange): the files are not known until
  a peer sends the info dictionary. `PeerConn::extensions` records who could.
- **DHT, PEX, encryption, µTP, proxies, bandwidth limits**: the settings panel
  says only the port and connections per torrent are read.

The window polls on a tick while a download runs (150 ms with peers, a second
without); `requests/e-f-wake-an-application-for-its-own-descriptor.md` would
let it wait instead.
