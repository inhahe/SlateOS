"""Mutation test for the Remote Desktop's VNC client.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-26, when a VNC profile began to
connect: the password asked for first and RDP/SSH refused by name, the session
becoming Connected on the server's handshake and filing its outcome then, the
remote screen kept opaque and uploaded, CopyRect reading the old pixels, and
keys and the pointer going to the remote machine -- the escape hotkey not
(`main.rs`); and the protocol itself (`rfb.rs`): the handshake's messages, the
bound on a rectangle, and the frame asked for next.

Deliberately absent: `on_wake` and `attach_waker`.  The window's event loop
calls them, which no test runs; the tests call `pump` directly, which is all
`on_wake` does.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

SHOWS = "a_vnc_session_shows_the_remote_screen"
INPUT = "keys_and_the_pointer_reach_the_remote_machine"
REFUSED = "a_refused_password_is_a_failed_attempt"
ASKS = "connect_asks_for_the_password_and_refuses_what_is_not_vnc"
OVERLAP = "an_overlapping_copy_reads_the_old_pixels"
DURATION = "a_sessions_duration_runs_from_its_handshake"
RECONNECT = "test_reconnect_session"
RECONNECT_ASKS = "reconnect_asks_for_the_password_again"
HANDSHAKE = "a_session_shakes_hands_and_shows_the_screen"
PASSWORD = "a_password_is_proven_by_the_challenge"
OUTSIDE = "a_rectangle_outside_the_desktop_is_refused"
OTHERS = "every_other_message_arrives"
RFB_KEYS = "keys_and_the_pointer_are_sent_as_rfb_says"
HEXTILE_TEST = "hextile_tiles_are_drawn"
ZRLE_TEST = "zrle_rectangles_are_drawn_through_one_stream"
ZRLE_REFUSED = "a_zrle_run_past_its_tile_is_refused"

MAIN = [
    (
        "RDP is let through to VNC",
        "        if profile.protocol != Protocol::Vnc {",
        "        if profile.protocol == Protocol::Ssh {",
        [ASKS],
    ),
    (
        "the handshake does not make the session Connected",
        "                        session.state = SessionState::Connected;",
        "                        session.state = SessionState::Connecting;",
        [SHOWS, INPUT, OVERLAP],
    ),
    (
        "a connection that worked is not filed",
        "                    self.record_attempt(id, true);\n",
        "",
        [SHOWS],
    ),
    (
        "an attempt that failed is not filed",
        "                    if !was_ready {",
        "                    if was_ready {",
        [REFUSED],
    ),
    (
        "an ended session's half is kept",
        "                    self.live.retain(|l| l.session_id != id);\n",
        "",
        [REFUSED],
    ),
    (
        "the screen is never uploaded",
        "            if live.ready && live.dirty {",
        "            if live.ready && live.dirty && false {",
        [SHOWS],
    ),
    (
        "the remote screen is drawn see-through",
        "                *d = s | 0xFF00_0000;",
        "                *d = *s;",
        [SHOWS, OVERLAP],
    ),
    (
        "CopyRect copies as it reads",
        "    if block.len() != area {",
        "    if block.len() != area || true {",
        [OVERLAP],
    ),
    (
        "the escape hotkey goes to the remote machine",
        "        if key.key == self.escape_hotkey || self.screen_rect().is_none() {",
        "        if self.screen_rect().is_none() {",
        [INPUT],
    ),
    (
        "a release forgets the keysym of its press",
        "            live.held.push((key.key, sym));",
        "            live.held.push((key.key, 0));",
        [INPUT],
    ),
    (
        "a pressed button is not sent",
        "            MouseEventKind::Press(b) => live.buttons |= bit(b),",
        "            MouseEventKind::Press(_) => {}",
        [INPUT],
    ),
    (
        "Enter in the password prompt does not connect",
        "                    let _id = self.connect_vnc(prompt.profile_index, &prompt.text);",
        "                    let _ = prompt;",
        [ASKS],
    ),
    (
        "reconnect claims an attempt again",
        "        let _asks = self.connect_profile(profile_index);",
        "        let _ = profile_index;\n"
        "        if let Some(s) = self.sessions.get_mut(index) {\n"
        "            s.state = SessionState::Reconnecting;\n"
        "        }",
        [RECONNECT, RECONNECT_ASKS],
    ),
    (
        "a live session is reconnected",
        "        if session.state != SessionState::Disconnected {",
        "        if false {",
        [RECONNECT],
    ),
    (
        "a connected session's duration does not run",
        "            (true, Some(since)) => now.saturating_sub(since),",
        "            (true, Some(_)) => 0,",
        [DURATION],
    ),
]

RFB = [
    (
        "ZRLE is not asked for first",
        "        for e in [ZRLE, HEXTILE, COPY_RECT, RAW, DESKTOP_SIZE] {",
        "        for e in [HEXTILE, ZRLE, COPY_RECT, RAW, DESKTOP_SIZE] {",
        [HANDSHAKE],
    ),
    (
        "each ZRLE rectangle starts a new zlib stream",
        "                    let raw = self\n                        .zrle\n",
        "                    let raw = deflate::PiecewiseInflater::zlib()\n",
        [ZRLE_TEST],
    ),
    (
        "a two-colour palette's indices are read two bits wide",
        "                        2 => (1, 0b1),",
        "                        2 => (1, 0b11),",
        [ZRLE_TEST],
    ),
    (
        "a run is one short",
        "        let mut run = 1_usize;",
        "        let mut run = 0_usize;",
        [ZRLE_TEST],
    ),
    (
        "a run past its tile is written",
        "                        let run = at.run()?;\n"
        "                        if run > area.saturating_sub(tile.len()) {",
        "                        let run = at.run()?;\n"
        "                        if false {",
        [ZRLE_REFUSED],
    ),
    (
        "palette runs ignore the run flag",
        "                        let run = if index & 128 != 0 { at.run()? } else { 1 };",
        "                        let run = 1;",
        [ZRLE_TEST],
    ),
    (
        "a tile's background is read and dropped",
        "                    bg = self.pixel()?;",
        "                    let _ = self.pixel()?;",
        [HEXTILE_TEST],
    ),
    (
        "a subrectangle is put at its tile's corner",
        "                        (tx.saturating_add(sx), ty.saturating_add(sy)),",
        "                        (tx, ty),",
        [HEXTILE_TEST],
    ),
    (
        "a raw tile is read column by column",
        "                                (tx.saturating_add(col), ty.saturating_add(row)),",
        "                                (tx.saturating_add(row), ty.saturating_add(col)),",
        [HEXTILE_TEST],
    ),
    (
        "subrectangles are never read",
        "                if kind & ANY_SUBRECTS == 0 {",
        "                if true {",
        [HEXTILE_TEST],
    ),
    (
        "a rectangle past the desktop's edge is read",
        "        || u32::from(x).saturating_add(u32::from(w)) > u32::from(width)",
        "        || false",
        [OUTSIDE],
    ),
    (
        "the pixels' red and blue are swapped",
        "                            [b, g, r, _] => u32::from_le_bytes([*b, *g, *r, 0]),",
        "                            [b, g, r, _] => u32::from_le_bytes([*r, *g, *b, 0]),",
        [HANDSHAKE, SHOWS],
    ),
]

TABLES = {
    "main.rs": MAIN,
    "rfb.rs": RFB,
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
        worst = max(worst, sweep(SRC / file, rows, "remotedesktop", timeout=900, only=mine))
    raise SystemExit(worst)
