"""Mutation test for the torrent client: its window (the pointer layer, the
magnet dialog, the search, the labels, the file priorities, and since
2026-09-26 the downloads it drives) and its transport -- the trackers, the
peer connection, the storage and the download session.

A table per source file, swept one file at a time.  Rows are deliberately
absent where no test on this Windows host can see the mutant: the refusal to
write through a symbolic link (its test is unix-only), the deadline that stops
a trickling tracker (the tests' silent tracker times out by the read timeout
alone), the refusal of non-loopback trackers in the tests (test-only code),
and the three-strikes rule and the stop flag in a peer thread, whose mutants
either race or hang for the harness's whole timeout.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Nothing answered the pointer (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED); the notice
saying this client cannot transfer was painted over; the list did not scroll;
labels, the search and magnet links could not be reached; a skipped file was
downloaded all the same; and a transfer set going with no network ticked seven
times a second for good.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

WINDOW = "opening_a_torrent_downloads_it"
LISTS = "opening_a_torrent_file_lists_what_is_in_it"
SPACE = "space_starts_pauses_and_resumes_a_download"
LOOKING = "a_download_with_no_peers_says_it_is_looking"
SKIPPED = "a_skipped_file_is_not_downloaded"
CTRL_P = "ctrl_p_and_ctrl_r_stop_and_start_everything"

FORGED = "a_udp_reply_that_is_not_ours_is_ignored"
UDP_ASKED = "a_udp_tracker_is_asked_as_bep_15_says"
HTTP_ASKED = "an_http_tracker_is_asked_and_answers_with_peers"
CHUNKED = "a_chunked_answer_with_ipv6_peers"
REFUSAL = "a_refusal_is_said_as_the_tracker_said_it"
REDIRECT = "a_redirect_is_followed"
TOO_BIG = "an_answer_too_big_or_too_slow_is_given_up"
ADDRESS = "an_address_this_client_cannot_use_says_why"
PARSE = "an_http_address_is_taken_apart"

HANDSHAKE = "a_handshake_for_another_torrent_is_refused"
SPLIT = "a_message_split_across_a_timeout_arrives_whole"
OVERSIZED = "an_oversized_message_or_a_close_ends_the_connection"
ASSEMBLY = "a_piece_is_assembled_from_the_blocks_asked_for"

SPANS = "a_piece_spans_the_files_it_covers"
WRITTEN = "pieces_written_make_the_files"
SHORT = "a_missing_or_short_file_is_not_had"
MISSING_SAVE = "a_missing_save_folder_is_made"

WHOLE = "a_torrent_is_downloaded_from_a_peer_the_tracker_names"
BAD = "a_bad_piece_is_fetched_again"
RESUME = "a_download_started_again_fetches_only_what_is_missing"
BLOCKED = "files_that_cannot_be_written_end_the_download"
QUIET = "a_stopped_download_goes_quiet_before_its_trackers_are_told"
BITFIELD = "a_bitfield_must_fit_the_torrent"
PICKER = "the_picker_hands_out_the_rarest_piece_nobody_is_fetching"

# (name, old, new, [tests that must fail])
MAIN = [
    # -- the file priorities ---------------------------------------------------------------------------------
    (
        "a file's priority reaches no piece",
        "            self.pieces.set_priority(piece, wanted);",
        "            let _ = (piece, wanted);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    (
        "a shared piece takes the lower priority",
        "                .max()\n"
        "                .unwrap_or(5);",
        "                .min()\n"
        "                .unwrap_or(5);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    (
        "a press on a priority changes nothing",
        "                t.set_file_priority(index, next);",
        "                let _ = (index, next);",
        ["a_skipped_file_is_not_downloaded"],
    ),
    # -- the clock ---------------------------------------------------------------------------------------------
    (
        "a stalled download says Downloading",
        "            TorrentState::Downloading if torrent.peers.is_empty() => {",
        "            TorrentState::Downloading if false => {",
        [LOOKING],
    ),
    # -- the notice ---------------------------------------------------------------------------------------------
    (
        "the notice is painted over again",
        "                y: y + 6.0 + i as f32 * 14.0,",
        "                y: 4.0 + i as f32 * 14.0,",
        ["the_notice_is_drawn_where_it_can_be_read"],
    ),
    # -- the keys ------------------------------------------------------------------------------------------------
    (
        "the magnet dialog lets keys through",
        "        if self.show_add_dialog {\n"
        "            return self.handle_dialog_key(key);",
        "        if false {\n"
        "            return self.handle_dialog_key(key);",
        ["a_press_behind_the_magnet_dialog_reaches_nothing", "a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "the search box lets keys through",
        "        if self.search_active {\n"
        "            return self.handle_search_key(key);",
        "        if false {\n"
        "            return self.handle_search_key(key);",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    (
        "Ctrl+U opens nothing",
        "                Key::U => {\n"
        "                    self.open_magnet_dialog();",
        "                Key::U => {\n"
        "                    let _ = 0;",
        ["a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "L labels nothing",
        "            Key::L => self.cycle_label(),",
        "            Key::L => EventResult::Ignored,",
        ["a_label_can_be_given_and_chosen_by", "every_advertised_key_does_something"],
    ),
    (
        "a search finds nothing by name",
        "                    t.name.to_lowercase().contains(&q) || t.label.to_lowercase().contains(&q)",
        "                    t.label.to_lowercase().contains(&q)",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    (
        "Down walks the order things were added in",
        "        let ids: Vec<u32> = self.filtered_torrents().iter().map(|t| t.id).collect();\n"
        "        if ids.is_empty() {",
        "        let ids: Vec<u32> = self.torrents.iter().map(|t| t.id).collect();\n"
        "        if ids.is_empty() {",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    # -- the pointer ----------------------------------------------------------------------------------------------
    (
        "Pause pauses nothing",
        "            Target::Pause => {\n"
        "                let Some(id) = self.selected_torrent else {\n"
        "                    return EventResult::Ignored;\n"
        "                };\n"
        "                self.pause_torrent(id);",
        "            Target::Pause => {\n"
        "                let Some(id) = self.selected_torrent else {\n"
        "                    return EventResult::Ignored;\n"
        "                };\n"
        "                let _ = id;",
        ["the_toolbar_answers_the_pointer"],
    ),
    (
        "a filter press does nothing",
        "            Target::Filter(filter) => return self.set_filter(filter),",
        "            Target::Filter(_) => return EventResult::Ignored,",
        ["the_sidebar_filters_and_labels_answer_the_pointer"],
    ),
    (
        "a second label press keeps the label",
        "                self.selected_label = if self.selected_label.as_ref() == Some(&label) {",
        "                self.selected_label = if false {",
        ["the_sidebar_filters_and_labels_answer_the_pointer"],
    ),
    (
        "a second press on a column head does not reverse it",
        "                if column == self.sort_column {\n"
        "                    self.sort_ascending = !self.sort_ascending;",
        "                if false {\n"
        "                    self.sort_ascending = !self.sort_ascending;",
        ["a_column_head_sorts_and_a_second_press_reverses"],
    ),
    (
        "a second press on a row does not show its details",
        "                if self.selected_torrent == Some(id) {\n"
        "                    self.active_tab = Tab::Details;",
        "                if false {\n"
        "                    self.active_tab = Tab::Details;",
        ["a_row_press_chooses_and_a_second_shows_its_details"],
    ),
    (
        "the details' label does nothing",
        "            Target::CycleLabel => return self.cycle_label(),",
        "            Target::CycleLabel => return EventResult::Ignored,",
        ["a_label_can_be_given_and_chosen_by"],
    ),
    (
        "Add does not add the typed magnet",
        "            Target::MagnetAdd => self.add_typed_magnet(),",
        "            Target::MagnetAdd => self.close_magnet_dialog(),",
        ["a_magnet_link_is_added_from_the_dialog"],
    ),
    (
        "a press behind the magnet dialog reaches the row",
        "        f.hit(Target::DialogBackdrop, Rect::new(0.0, 0.0, width, height));",
        "",
        ["a_press_behind_the_magnet_dialog_reaches_nothing"],
    ),
    (
        "a press goes through the list of keys",
        "            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, width, height));",
        "",
        ["the_list_of_keys_is_modal_to_the_pointer"],
    ),
    (
        "a press elsewhere leaves the keys in the search box",
        "        if target != Target::Search {\n"
        "            self.search_active = false;",
        "        if false {\n"
        "            self.search_active = false;",
        ["the_search_box_filters_and_escape_clears_it"],
    ),
    # -- scrolling ------------------------------------------------------------------------------------------------
    (
        "the list does not scroll",
        "        self.transfer_scroll = next;\n"
        "        EventResult::Consumed",
        "        let _ = next;\n"
        "        EventResult::Consumed",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    (
        "the list scrolls past its end",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        }\n"
        "        .min(last);",
        "            now.saturating_add(rows.unsigned_abs())\n"
        "        };\n"
        "        let _ = last;",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    (
        "the chosen transfer scrolls off the bottom",
        "            } else if at >= self.transfer_scroll.saturating_add(visible) {",
        "            } else if false {",
        ["the_transfer_list_scrolls_and_follows_the_selection"],
    ),
    # -- the downloads the window drives (2026-09-26) -----------------------
    (
        "opening a torrent does not start it",
        "                let id = self.add_torrent(meta, None);\n                self.start_torrent(id);",
        "                let id = self.add_torrent(meta, None);",
        [WINDOW],
    ),
    (
        "pausing leaves the download running",
        "        if let Some(session) = self.sessions.remove(&id) {\n            session.stop();\n        }\n    }",
        "    }",
        [SPACE],
    ),
    (
        "a finished download is said to be seeding",
        "                self.state = TorrentState::Complete;",
        "                self.state = TorrentState::Seeding;",
        [WINDOW],
    ),
    (
        "a piece had is not counted as downloaded",
        "                self.pieces.set_piece(index);\n                self.downloaded = self.have_bytes();",
        "                self.pieces.set_piece(index);",
        [WINDOW],
    ),
    (
        "a finished download keeps its session",
        "                // Dropping it stops whatever of it is still running.\n                self.sessions.remove(&id);",
        "",
        [WINDOW],
    ),
    (
        "a skipped file's pieces are wanted",
        "            .map(|i| self.pieces.priority(i).is_some_and(|p| p > 0))",
        "            .map(|_| true)",
        [SKIPPED],
    ),
    (
        "the clock runs with no download",
        "        if self.sessions.is_empty() {\n            return None;\n        }",
        "",
        [WINDOW, CTRL_P],
    ),
    (
        "a download with no peers ticks as fast as one with them",
        "        } else {\n            Duration::from_secs(1)\n        })",
        "        } else {\n            PIECE_STEP\n        })",
        [LOOKING],
    ),
    (
        "a tracker's answer is not shown",
        "                            t.status = TrackerStatus::Working;",
        "                            t.status = TrackerStatus::NotContacted;",
        [WINDOW],
    ),
]

TRACKER = [
    (
        "a UDP reply with another transaction number is believed",
        "            if be_u32(reply, 4) != Some(tid) {\n                continue;\n            }",
        "",
        [FORGED],
    ),
    (
        "a UDP tracker's error is not said",
        "                Some(3) => {",
        "                Some(333) => {",
        [FORGED],
    ),
    (
        "the UDP connection number is not used",
        "    packet.extend_from_slice(&connection_id.to_be_bytes());",
        "    packet.extend_from_slice(&0_u64.to_be_bytes());",
        [UDP_ASKED],
    ),
    (
        "the UDP event is not sent",
        "        TrackerEvent::Started => 2,",
        "        TrackerEvent::Started => 0,",
        [UDP_ASKED],
    ),
    (
        "an HTTP answer past the cap is read on",
        "                if out.len() > cap {",
        "                if out.len() > usize::MAX - 1 {",
        [TOO_BIG],
    ),
    (
        "a chunked answer is not put back together",
        "        let body = if chunked {",
        "        let body = if false {",
        [CHUNKED],
    ),
    (
        "a redirect is not followed",
        "            301 | 302 | 303 | 307 | 308 => {",
        "            399 => {",
        [REDIRECT],
    ),
    (
        "a refusal is taken for an answer",
        "    if let Some(reason) = response.failure_reason {\n        return Err(format!(\"the tracker refused: {reason}\"));\n    }",
        "",
        [REFUSAL],
    ),
    (
        "IPv6 peers are dropped",
        "        peers.extend(v6.chunks_exact(18).filter_map(peer_v6));",
        "",
        [CHUNKED],
    ),
    (
        "an HTTP tracker's interval is not floored",
        "        interval: Duration::from_secs(response.interval).clamp(MIN_INTERVAL, MAX_INTERVAL),",
        "        interval: Duration::from_secs(response.interval).min(MAX_INTERVAL),",
        [CHUNKED],
    ),
    (
        "an https tracker is not refused for its reason",
        '    } else if url.starts_with("https://") {',
        '    } else if url.starts_with("never://") {',
        [ADDRESS],
    ),
    (
        "a user name in the address is let through",
        "    if authority.contains('@') {",
        "    if false {",
        [PARSE],
    ),
    (
        "port 0 is taken",
        "        Some(p) => p.parse::<u16>().ok().filter(|&p| p > 0).ok_or_else(bad)?,",
        "        Some(p) => p.parse::<u16>().ok().ok_or_else(bad)?,",
        [PARSE],
    ),
]

PEER = [
    (
        "a handshake for another torrent is accepted",
        "        if got.info_hash != info_hash {",
        "        if false {",
        [HANDSHAKE],
    ),
    (
        "a message over the cap is waited for",
        "        if len > MAX_MESSAGE {",
        "        if len > usize::MAX - 1 {",
        [OVERSIZED],
    ),
    (
        "half a message is thrown away at a timeout",
        "                    ) =>\n                {\n                    return Ok(None);\n                }",
        "                    ) =>\n                {\n                    self.pending.clear();\n                    return Ok(None);\n                }",
        [SPLIT],
    ),
    (
        "a block not asked for is taken",
        "        if !self.asked.get(b).copied().unwrap_or(false) || bytes.len() != len as usize {",
        "        if bytes.len() != len as usize {",
        [ASSEMBLY],
    ),
    (
        "a block off a block's edge is taken",
        "        if !begin.is_multiple_of(BLOCK) {",
        "        if false {",
        [ASSEMBLY],
    ),
    (
        "requests are not held to the pipeline",
        "        let room = outstanding.saturating_sub(in_flight);",
        "        let room = usize::MAX;",
        [ASSEMBLY],
    ),
    (
        "a choke does not forget what was asked",
        "        self.asked.iter_mut().for_each(|a| *a = false);",
        "",
        [ASSEMBLY],
    ),
    (
        "the last block is asked for whole",
        "        let len = self.data.len().saturating_sub(start).min(BLOCK as usize);",
        "        let len = BLOCK as usize;",
        [ASSEMBLY],
    ),
]

STORAGE = [
    (
        "a piece is cut at the wrong place",
        "                    offset: from - f.offset,",
        "                    offset: from,",
        [SPANS],
    ),
    (
        "a piece's hash is not checked",
        "            .is_some_and(|h| data.len() as u64 == self.piece_len(index) && sha1::sha1(data) == *h)",
        "            .is_some_and(|_| data.len() as u64 == self.piece_len(index))",
        [WRITTEN],
    ),
    (
        "a short file reads as had",
        "                Ok(_) => return Ok(None),",
        "                Ok(_) => {}",
        [SHORT],
    ),
    (
        "a missing file is an error rather than not had",
        "                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),\n",
        "",
        [WRITTEN, SHORT],
    ),
    (
        "the save folder is not made",
        "        fs::create_dir_all(&self.root)?;",
        "",
        [MISSING_SAVE],
    ),
]

SESSION = [
    (
        "a bad piece is written",
        "                    if self.storage.matches(i, &bytes) {",
        "                    if true {",
        [BAD],
    ),
    (
        "what is on disk is fetched again",
        "    let picker = Arc::new(Mutex::new(Picker {\n        have,",
        "    let picker = Arc::new(Mutex::new(Picker {\n        have: vec![false; count],",
        [RESUME],
    ),
    (
        "a bitfield of the wrong length is taken",
        "    if bits.len() != count.div_ceil(8) {\n        return None;\n    }",
        "",
        [BITFIELD],
    ),
    (
        "a bitfield's spare bits are ignored",
        "    if all.iter().skip(count).any(|&b| b) {\n        return None;\n    }",
        "",
        [BITFIELD],
    ),
    (
        "the rarest piece is not preferred",
        "            .min_by_key(|&i| self.seen.get(i).copied().unwrap_or(0))?;",
        "            .min_by_key(|&i| i)?;",
        [PICKER],
    ),
    (
        "a piece being fetched is handed out again",
        "                    && !self.busy.get(i).copied().unwrap_or(true)",
        "                    && true",
        [PICKER],
    ),
    (
        "an unwanted piece is handed out",
        "                    && self.wanted.get(i).copied().unwrap_or(false)\n                    && !self.have.get(i).copied().unwrap_or(true)\n                    && !self.busy",
        "                    && !self.have.get(i).copied().unwrap_or(true)\n                    && !self.busy",
        [PICKER],
    ),
    (
        "the trackers are not told it finished",
        "                        finish(&urls, &request(TrackerEvent::Completed, 0, downloaded));",
        "",
        [WHOLE],
    ),
    (
        "a write that fails is not said",
        "                            let _ = self.reports.send(Report::Fatal(e.clone())); // As above.",
        "",
        [BLOCKED],
    ),
    (
        "a stopped download goes quiet only after its trackers are told",
        "    }\n    hush();\n    if !lock(&picker).complete() && !first {",
        "    }\n    if !lock(&picker).complete() && !first {",
        [QUIET],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "tracker.rs": TRACKER,
    "peer.rs": PEER,
    "storage.rs": STORAGE,
    "session.rs": SESSION,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "torrent", timeout=900, only=mine))
    raise SystemExit(worst)
