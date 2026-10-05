## TD-C-FOUR-CLIENTS-FOR-SERVICES-THEY-COULD-NOT-REACH -- FIXED 2026-09-15

**In short:** a remote desktop client that reported connections, a feed reader
whose articles never came from a feed, a podcast app that marked episodes
downloaded and counted the disk space they used, and a video player that played
a two-hour film from a path with no file. None of the four could reach the
thing it was a client for.

**Date:** 2026-09-15. **Lane:** C. Applications fifteen through eighteen.

### `apps/remotedesktop` -- and an outcome recorded before the attempt

`new` called `load_sample_data`, so the window opened on profiles for machines
nobody had configured and a session already in `SessionState::Connected`.
Pressing Connect created a session that `handle_tick` walked through
`Authenticating` to `Connected`.

**The second fabrication is the one worth separating out, because it would be
a defect even with a working network.** `connect_profile` wrote a history entry
with `success: true` **at the moment Connect was pressed**, before any outcome
was known.

An outcome written at the start of an attempt is not a record of what happened;
it is a record of what was *intended*, filed where somebody will later read it
as what happened. A connection log exists to be consulted after the fact, by
someone asking whether a machine was reachable last Tuesday -- and this one
answered yes to every question it was ever asked.

`advance_session_state` is `#[cfg(test)]` now. Its own doc comment already said
"simulated for UI development", and `handle_tick` called it on every tick, so
**the simulation was the shipping behaviour and the comment saying so was read
only by somebody already looking at that line.**

### `apps/rssreader` -- where the split fell, and which half to keep

`new` called `populate_sample_data`, which built folders, feeds and articles out
of `SAMPLE_RSS` -- a constant XML document **fed through the real parser**.

The split is unusually clean and it decides the fix: **everything downstream of
the parse was genuine, and everything upstream of it was invented.**
`parse_feed`, the de-duplication, the article merge and the unread and starred
counters are all real and stay.

So `ingest_feed_xml` became `pub` rather than private, and deliberately **not**
`#[cfg(test)]`. It is the door a real fetcher comes through -- the same shape as
`speedtest`'s `record_sample` and `soundrecorder`'s `process_samples`. It has no
caller in production, which is the honest state of the app: the parser is
written and tested, and the thing that would hand it bytes is not built.
Retiring it into the test build would hide that, and would make whoever writes
the fetcher reconstruct an entry point that already exists and already works.

It also advertises **F5 and Shift+F5** in its own key list as "Refresh current
feed" and "Refresh all feeds", and nothing dispatches either. A key that is
documented and does nothing reads as a broken key, so the banner names it.

### `apps/podcast` -- two claims that fail in different places

The tick called `simulate_download_tick` every frame. It moved each active item
on by 10%, and on reaching 1.0 marked the episode `Downloaded` **and added the
episode's file size to `used_disk_bytes`**.

* **The disk figure outlives the session.** Someone checking why their storage
  is full finds gigabytes attributed to podcasts, and deleting them frees
  nothing.
* **The `Downloaded` mark fails somewhere more specific.** Offline availability
  is the entire reason anyone downloads a podcast, so a wrong mark surfaces on
  a plane or a train -- **at the exact moment there is no connection to fall
  back on.** That is the same structure as `weather`'s alert channel: the
  failure is deferred to the situation the feature existed for.

### `apps/videoplayer` -- and the fourth author to reach the same wrong conclusion

`main` called `seeded_player`, so every launch opened on a two-hour "Sample
Movie" at `/home/user/Videos/sample.mkv`, with chapters, external subtitles and
a playlist. Pressing play ran the clock, advanced the chapter markers and put
subtitles on screen on cue, over a black rectangle.

**The playlist is the part that outlives the window.** An entry naming a path is
a claim that a file is at that path, and a playlist is the kind of thing
somebody reads later to find out what they have.

Its doc comment said the sample content existed *"so the first window is not an
empty black rectangle"*. **That is the fourth appearance of this reasoning in
the sweep, in four different authors' words:**

| app | the words |
|---|---|
| `videoplayer` | "so the first window is not an empty black rectangle" |
| `torrent` | "a client that opens on an empty list looks broken rather than idle" |
| `photomanager` | "so the first window is not an empty grid" |
| `filesearch` | "until a real index exists this is what there is to search" |

**All four were right about the symptom and wrong about the remedy.** An empty
window does look broken. The answer is to **say why it is empty**, not to fill
it. Four people reached the first half independently and none reached the
second, which suggests the missing step is not obvious and is worth stating
plainly wherever it can be seen.

### The fixture-placement correlation is now exact

Eighteen applications. **Sixteen wired their fixture into the constructor and
broke between 6 and 66 tests each. Two called it from `main()` instead --
`apps/email` and `apps/videoplayer` -- and both broke zero.**

The fix is otherwise identical in all eighteen. So the cost of removing a
fabrication is almost entirely in **where the call sat**, not in what it
produced: a fixture in the constructor is handed to every test whether it wants
it or not, and a fixture in `main` has to be asked for.
