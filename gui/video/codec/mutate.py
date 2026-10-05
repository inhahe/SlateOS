"""Mutation test for videocodec's subtitles: Blu-ray's PGS and DVD's VobSub
pictures, and the reader that gives their cues.

Each row puts back one way of not showing what FFmpeg shows -- a rule of its
`pgssub` decoder dropped, a colour rounded otherwise, a crop made as FFmpeg
makes it -- or of not giving the cues a player needs, and names the tests
that have to notice: the fixtures' (`tests/subtitles.rs`, held to FFmpeg's
sub2video pictures by `tests/data/generate_subtitle_fixtures.py`) and the
module's own.

Breaks one piece of production code at a time and checks that the tests
which claim to cover it are the ones that fail.  A test that passes against
a broken program is not testing the program.

The sweep runs the crate's unit tests and `tests/subtitles.rs` alone: its
`tests/frames.rs` decodes pictures for ten minutes, and names nothing here.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"
TARGETS = ("--lib", "--test", "subtitles")

# pgs.rs's own.
SHOWS = "a_display_set_shows_its_composition"
CODES = "the_run_length_codes_read_as_ffmpeg_reads_them"
COLOURS = "colours_are_ffmpegs"
SD = "a_short_canvas_converts_by_bt601"
NOTHING = "nothing_composed_clears_and_a_missing_palette_changes_nothing"
EPOCH = "an_epoch_start_or_acquisition_point_forgets_objects_and_palettes"
CAPS = "two_objects_are_shown_and_an_epoch_holds_eight_palettes_and_64_objects"
CROP = "a_crop_shows_its_part_and_forced_is_kept"
COUNTED = "an_epoch_s_beginning_is_known_and_damage_is_counted"
CANVAS = "a_canvas_past_8192_is_damage"
HELD = "an_epoch_holds_32_mib_of_pixels"
SEGMENTS = "an_object_in_two_segments_is_one_and_damage_takes_the_old_one_with_it"
# The fixtures'.
PGS = "pgs"
PGS_SD = "pgs_on_a_standard_definition_canvas"
PGS_CROPPED = "pgs_cropped_as_a_blu_ray_player_crops"
PGS_DAMAGE = "pgs_damage_taken_as_ffmpeg_takes_it"
SEEK = "a_seek_in_pictures_lands_on_the_picture_showing_then"
SEEK_BACK = "a_seek_back_forgets_what_was_read_after_it"
OPENED = "text_is_opened_before_pictures_and_pictures_read_before_those_not"
# vobsub.rs's own.
SETUP = "a_setup_gives_the_canvas_and_the_palette"
STARTED = "a_picture_shows_from_its_start_until_its_stop"
DVD_CODES = "the_run_length_codes_read_as_a_dvd_codes_them"
EDGES = "transparent_edges_are_cut_and_a_transparent_inside_is_kept"
SEQUENCES = "each_sequence_takes_effect_at_its_date"
LENIENT = "forced_no_start_and_no_stop_are_ffmpegs"
GREYS = "without_a_palette_the_greys_are_ffmpegs"
DVD_DAMAGE = "damage_changes_nothing_and_a_transparent_picture_clears"
# The VobSub fixtures'.
VOBSUB = "vobsub"
VOBSUB_DVD = "vobsub_as_a_dvd_player_shows_it"
VOBSUB_GREY = "vobsub_without_a_palette"
VOBSUB_PAL = "vobsub_without_a_size"
VOBSUB_DAMAGE = "vobsub_damage_taken_as_ffmpeg_takes_it"
VOB_SEEK = "a_seek_in_dvd_pictures_finds_one_still_showing"
# dvb.rs's own.
DVB_SHOWS = "a_display_set_shows_its_regions_through_their_clut"
DVB_VERSION = "a_page_of_its_own_version_changes_nothing_and_afresh_forgets"
DVB_ORDER = "the_first_region_listed_is_on_top_and_a_repeat_ends_the_list"
DVB_DRAWN = "an_object_is_drawn_where_its_region_places_it_when_its_data_comes"
DVB_KNOWN = "an_object_is_known_from_its_naming_until_no_region_places_it"
DVB_NON_MOD = "the_non_modifying_colour_leaves_its_pixels_and_the_next_in_place"
DVB_ENTRIES = "an_entry_for_several_tables_goes_into_each"
DVB_SERVICE = "only_the_services_pages_are_read"
DVB_DAMAGED = "damage_shows_nothing_new_and_is_counted"
DVB_LAST = "of_several_display_sets_in_a_block_the_last_shows"
DVB_EPOCH = "an_epoch_begins_at_an_acquisition_point_or_a_mode_change"
DVB_SHORT = "a_block_of_six_bytes_or_not_of_segments_is_damage"
DVB_BACKGROUND = "a_new_region_is_filled_with_its_background_and_shown"
DVB_END_CODE = "a_line_as_wide_as_its_region_ends_on_its_end_code"
DVB_BOUNDS = "regions_hold_ffmpegs_largest_four_times_and_place_1024_objects"
DVB_RESET = "a_reset_forgets_the_page_held_but_not_the_settled_size"
# The DVB fixtures'.
DVB = "dvb"
DVB_RECEIVER = "dvb_as_a_receiver_shows_it"
DVB_HD = "dvb_on_a_high_definition_display_with_a_window"
DVB_PAGES = "dvb_reads_its_own_services_pages"
DVB_DAMAGE = "dvb_damage_taken_as_ffmpeg_takes_it"
DVB_SEEK = "a_seek_in_dvb_pictures_reads_from_where_they_begin_afresh"
DVB_FORGETS = "a_seek_in_dvb_pictures_forgets_what_was_read"

# (name, old, new, [tests that must fail])
PICTURES = [
    (
        "an acquisition point keeps the epoch",
        "        if state & 0xC0 != 0 {",
        "        if state & 0x80 != 0 {",
        [EPOCH, PGS_DAMAGE],
    ),
    (
        "every composition forgets the epoch",
        "        if state & 0xC0 != 0 {",
        "        if true {",
        [EPOCH, PGS],
    ),
    (
        "a composition shows every object it names",
        "        for _ in 0..usize::from(*count).min(MAX_SHOWN) {",
        "        for _ in 0..usize::from(*count) {",
        [CAPS, PGS_DAMAGE],
    ),
    (
        "an epoch holds a ninth palette",
        "            if self.palettes.len() >= MAX_PALETTES {",
        "            if false {",
        [CAPS, PGS_DAMAGE],
    ),
    (
        "an epoch holds a sixty-fifth object",
        "        if held.is_none() && self.objects.len() >= MAX_OBJECTS {",
        "        if false {",
        [CAPS, PGS_DAMAGE],
    ),
    (
        "a canvas of any size is read",
        "        if width > MAX_CANVAS || height > MAX_CANVAS {",
        "        if false {",
        [CANVAS],
    ),
    (
        "an epoch's objects hold any number of pixels",
        "            && others.saturating_add(size) <= MAX_HELD;",
        "            && others.saturating_add(size) <= usize::MAX;",
        [HELD],
    ),
    (
        "a palette that does not exist clears",
        "            return Shown::Unchanged;",
        "            return Shown::Images(Vec::new());",
        [NOTHING, PGS_DAMAGE],
    ),
    (
        "a composition of nothing changes nothing",
        "            return Shown::Images(Vec::new());",
        "            return Shown::Unchanged;",
        [NOTHING, PGS],
    ),
    (
        "a run past the object's end is cut to fit",
        "filled.checked_add(run).filter(|&e| e <= total)",
        "filled.checked_add(run).map(|e| e.min(total))",
        [CODES, PGS_DAMAGE],
    ),
    (
        "a line's end moves to the next line",
        "            lines = lines.saturating_add(1);\n",
        "            lines = lines.saturating_add(1);\n"
        "            filled = filled.next_multiple_of(usize::from(width).max(1)).min(total);\n",
        [CODES, PGS_DAMAGE],
    ),
    (
        "codes that do not fill an object show what they fill",
        "    (filled >= total).then_some(pixels)",
        "    Some(pixels)",
        [CODES, SEGMENTS, PGS_DAMAGE],
    ),
    (
        "a damaged object leaves the old one of its id",
        "            (None, Some(at)) => {\n                self.objects.remove(at);\n            }",
        "            (None, Some(_)) => {}",
        [SEGMENTS, PGS_DAMAGE],
    ),
    (
        "an object larger than the canvas is held",
        "        let fits = p.width <= self.width\n            && p.height <= self.height\n            && others",
        "        let fits = others",
        # The fixture's is wider than the canvas, and a picture past the
        # canvas's edge is left out by sub2video and by the test's drawing
        # alike: the module's own test sees it held (and the fixture's count
        # of damage one set short).
        [SEGMENTS],
    ),
    (
        "the rest of another object joins the one begun",
        "self.partial.as_mut().filter(|p| p.id == id)",
        "self.partial.as_mut().filter(|_| true)",
        [SEGMENTS],
    ),
    (
        "colours by BT.601 on every canvas",
        "        let sd = self.height > 0 && self.height <= 576;",
        "        let sd = true;",
        [SHOWS, PGS],
    ),
    (
        "colours by BT.709 on every canvas",
        "        let sd = self.height > 0 && self.height <= 576;",
        "        let sd = false;",
        [SD, PGS_SD],
    ),
    (
        "colours rounded down",
        "((y + v + 512) >> 10)",
        "((y + v) >> 10)",
        [COLOURS, PGS],
    ),
    (
        "a crop is ignored, as FFmpeg ignores it",
        "    let [left, top, w, h] = p.crop.unwrap_or([0, 0, o.width, o.height]);",
        "    let [left, top, w, h] = [0, 0, o.width, o.height];",
        [CROP, PGS_CROPPED],
    ),
    (
        "nothing is forced",
        "                forced: flags & 0x40 != 0,",
        "                forced: false,",
        [CROP, PGS],
    ),
    (
        "a crop is read where the forced bit is",
        "            if flags & 0x80 != 0 {",
        "            if flags & 0x40 != 0 {",
        [CROP, PGS],
    ),
    (
        "an acquisition point does not begin an epoch for a seek",
        "            return body.get(7).is_some_and(|state| state & 0xC0 != 0);",
        "            return body.get(7).is_some_and(|state| state & 0x80 != 0);",
        [COUNTED],
    ),
    (
        "a seek forgets the count of damage",
        "        let damaged = self.damaged;",
        "        let damaged = 0;",
        [COUNTED],
    ),
]

READER = [
    (
        "a seek reads pictures from the set it lands on",
        "            Reader::Pgs(_) => self.back_to_epoch_start(ticks, pgs::begins_epoch)?,",
        "            Reader::Pgs(_) => {}",
        [SEEK],
    ),
    (
        "any set begins an epoch for a seek",
        "            if begins(&sample.data) {",
        "            if true {",
        [SEEK, DVB_SEEK],
    ),
    (
        "a seek keeps the decoder's epoch",
        "            Reader::Pgs(decoder) => decoder.reset(),",
        "            Reader::Pgs(_) => {}",
        [SEEK_BACK],
    ),
    (
        "pictures replaced at the time they came are given",
        "            if at > cue.start && self.kept(cue.start, at) {",
        "            if self.kept(cue.start, at) {",
        # A PGS display set's picture replaced at its own time never shows
        # (the next set supersedes it before it is made); a subpicture's
        # colour change and stop at one date make one that would.
        [VOBSUB],
    ),
    (
        "the last picture ends where it begins",
        "                end: i64::MAX,",
        "                end: at,",
        [PGS],
    ),
    (
        "damage in pictures is not counted",
        "            Reader::Pgs(decoder) => decoder.damaged(),",
        "            Reader::Pgs(_) => 0,",
        [PGS_DAMAGE],
    ),
    (
        "pictures are opened before text",
        "                    !t.format.is_text(),\n",
        "",
        [OPENED],
    ),
    (
        "pictures not read are opened before pictures read",
        "                    !t.format.is_read(),\n",
        "",
        [OPENED],
    ),
    (
        "a block's changes drop what the last had still to make before them",
        "        self.make_until(first);\n        self.coming.clear();",
        "        self.coming.clear();",
        [PGS, VOBSUB],
    ),
    (
        "a block's changes supersede nothing of the last's",
        "        self.coming.clear();\n        self.coming.extend(changes);",
        "        self.coming.extend(changes);",
        [VOBSUB, VOBSUB_DVD],
    ),
    (
        "a seek in DVD pictures reads from the SPU it lands on",
        "            Reader::VobSub(_) => self.back_one(ticks)?,",
        "            Reader::VobSub(_) => {}",
        [VOB_SEEK],
    ),
    (
        "a seek's step back stays where it is",
        "        if self.demuxer.seek(self.key, at.saturating_sub(1)).is_err() {",
        "        if self.demuxer.seek(self.key, at).is_err() {",
        [VOB_SEEK],
    ),
    (
        "a block's duration ends no picture",
        "let duration = (length > 0).then(|| i64::try_from(length).unwrap_or(i64::MAX));",
        "let duration = (length > 0).then(|| i64::MAX).filter(|_| false);",
        [VOBSUB_DAMAGE],
    ),
    (
        "a DVB page's timeout clears nothing",
        "                    vec![(start, images), (gone, Vec::new())]",
        "                    vec![(start, images)]",
        [DVB],
    ),
    (
        "a seek in DVB pictures reads from the set it lands on",
        "                self.back_to_epoch_start(ticks, |block| dvb::begins_epoch(block, pages))?;",
        "                let _ = (pages, ticks);",
        [DVB_SEEK],
    ),
    (
        "a seek keeps what DVB pictures were read",
        "            Reader::Dvb(decoder) => decoder.reset(),",
        "            Reader::Dvb(_) => {}",
        [DVB_FORGETS],
    ),
    (
        "damage in DVB pictures is not counted",
        "            Reader::Dvb(decoder) => decoder.damaged(),",
        "            Reader::Dvb(_) => 0,",
        [DVB_DAMAGE],
    ),
]

DVD = [
    (
        "a setup with no size is on NTSC's canvas",
        "const DEFAULT_CANVAS: (u32, u32) = (720, 576);",
        "const DEFAULT_CANVAS: (u32, u32) = (720, 480);",
        [SETUP, VOBSUB_PAL],
    ),
    (
        "a picture never started is never shown",
        "    if !sequences.iter().any(|s| s.start) {",
        "    if false {",
        [LENIENT, VOBSUB],
    ),
    (
        "a picture never started shows from its sequence's date",
        "            (first.start, first.ms) = (true, 0);",
        "            first.start = true;",
        [LENIENT],
    ),
    (
        "a later colour or fade changes nothing, as in FFmpeg",
        "            shown.map(|(_, forced)| (s.picture, forced))",
        "            shown",
        [SEQUENCES, VOBSUB_DVD],
    ),
    (
        "changes go back in time with their dates",
        "        let at = s.ms.saturating_mul(1_000_000).max(last);",
        "        let at = s.ms.saturating_mul(1_000_000);",
        [DVD_DAMAGE],
    ),
    (
        "a picture never stopped is not ended by its block",
        "        if let Some(end) = duration.filter(|&d| d > last) {",
        "        if let Some(end) = duration.filter(|_| false) {",
        [LENIENT, VOBSUB_DAMAGE],
    ),
    (
        "a sequence naming an earlier one next is followed",
        "        if next <= at {",
        "        if next == at {",
        [DVD_DAMAGE],
    ),
    (
        "dates are hundredths of a second",
        "    i64::from(u32::from(date) * 1024 / 90)",
        "    i64::from(u32::from(date) * 10)",
        [STARTED, VOBSUB],
    ),
    (
        "a forced picture's transparent edges are cut",
        "    let kept = |&index: &u8| forced || rgba",
        "    let kept = |&index: &u8| rgba",
        [EDGES, VOBSUB],
    ),
    (
        "no picture's transparent edges are cut",
        "    let kept = |&index: &u8| forced || rgba",
        "    let kept = |&index: &u8| true || rgba",
        [EDGES, VOBSUB],
    ),
    (
        "alpha is sixteen times its nibble",
        "a.saturating_mul(17)",
        "a.saturating_mul(16)",
        [STARTED, VOBSUB],
    ),
    (
        "two greys are FFmpeg's ramp of three",
        "        2 => &[0x00, 0xFF],",
        "        2 => &[0x00, 0x80],",
        [GREYS, VOBSUB_GREY],
    ),
    (
        "greys are their levels unscaled",
        "u8::try_from(level.saturating_mul(255) / 256)",
        "u8::try_from(level)",
        [GREYS, VOBSUB_GREY],
    ),
    (
        "a run of no count fills nothing",
        "                0 => left,",
        "                0 => 0,",
        [DVD_CODES, VOBSUB],
    ),
    (
        "a run past the line's end runs on",
        "                n => n.min(left),",
        "                n => n,",
        [DVD_CODES],
    ),
    (
        "a line does not end on a byte",
        "        self.at = self.at.saturating_add(self.at % 2);\n",
        "",
        [DVD_CODES, VOBSUB],
    ),
    (
        "a forced start is not forced",
        "                FORCED_START => (s.start, s.forced) = (true, true),",
        "                FORCED_START => s.start = true,",
        [LENIENT, VOBSUB],
    ),
    (
        "the four colours are read the other way round",
        "let four = [b & 0x0F, b >> 4, a & 0x0F, a >> 4];",
        "let four = [a >> 4, a & 0x0F, b >> 4, b & 0x0F];",
        [STARTED, VOBSUB],
    ),
    (
        "a code of three nibbles reads a fourth",
        "            for floor in [0x4, 0x10, 0x40] {",
        "            for floor in [0x4, 0x10, 0x100] {",
        [DVD_CODES],
    ),
]

LIB = [
    (
        "DVB's pictures are not read",
        "        self.is_text() || matches!(self, Self::Pgs | Self::VobSub | Self::Dvb)",
        "        self.is_text() || matches!(self, Self::Pgs | Self::VobSub)",
        [OPENED],
    ),
]

DVB_ROWS = [
    # Blocks and segments.
    (
        "a block of six bytes is read",
        "        if len <= 6 || data.at(0) != 0x0F {",
        "        if len < 6 || data.at(0) != 0x0F {",
        [DVB_SHORT, DVB_DAMAGE],
    ),
    (
        "a block not starting with the sync byte is read",
        "        if len <= 6 || data.at(0) != 0x0F {",
        "        if len <= 6 {",
        [DVB_SHORT],
    ),
    (
        "another service's segments are read",
        "            if self.pages.is_none_or(|(c, a)| id == c || id == a) {",
        "            if true {",
        [DVB_SERVICE, DVB_PAGES],
    ),
    (
        "a setup does not name the service",
        "        let named = config.len() >= 4 && (config.len().is_multiple_of(5) || config.len() == 4);",
        "        let named = false;",
        [DVB_SERVICE, DVB_PAGES],
    ),
    (
        "of several display sets in a block, the first shows",
        "                    END => {\n                        shown = Some(self.show());",
        "                    END => {\n                        shown = shown.or_else(|| Some(self.show()));",
        [DVB_LAST, DVB_RECEIVER],
    ),
    (
        "a page, a region and an object with no end are not shown",
        "            if !end {\n                shown = Some(self.show());",
        "            if false && !end {\n                shown = Some(self.show());",
        [DVB_DAMAGED, DVB_DAMAGE],
    ),
    (
        "a display set of no display definition does not settle the size",
        "            if !display {\n                self.sized = true;",
        "            if !display {\n                self.sized = false;",
        [DVB_RESET],
    ),
    # Pages.
    (
        "a page of the version held is read again",
        "        if self.page_version == Some(version) {",
        "        if false && self.page_version == Some(version) {",
        [DVB_VERSION, DVB],
    ),
    (
        "an acquisition point does not begin afresh",
        "        if state == 1 || state == 2 {",
        "        if state == 2 {",
        [DVB_VERSION],
    ),
    (
        "a mode change does not begin afresh",
        "        if state == 1 || state == 2 {",
        "        if state == 1 {",
        [DVB, DVB_VERSION],
    ),
    (
        "a page lists a region twice",
        "            if self.shown.iter().any(|&(r, _, _)| r == id) {",
        "            if false && self.shown.iter().any(|&(r, _, _)| r == id) {",
        [DVB_ORDER, DVB],
    ),
    (
        "the last region listed is on top",
        "        for &(id, x, y) in self.shown.iter().rev() {",
        "        for &(id, x, y) in self.shown.iter() {",
        [DVB_ORDER, DVB, DVB_HD],
    ),
    # Regions.
    (
        "a region of no size is held",
        "        if count == 0 || count > MAX_REGION || held - region.pixels.len() + count > MAX_HELD {",
        "        if count > MAX_REGION || held - region.pixels.len() + count > MAX_HELD {",
        [DVB_DAMAGED, DVB_DAMAGE],
    ),
    (
        "a region past FFmpeg's largest is held",
        "        if count == 0 || count > MAX_REGION || held - region.pixels.len() + count > MAX_HELD {",
        "        if count == 0 || held - region.pixels.len() + count > MAX_HELD {",
        [DVB_BOUNDS],
    ),
    (
        "regions past four of FFmpeg's largest are held",
        "        if count == 0 || count > MAX_REGION || held - region.pixels.len() + count > MAX_HELD {",
        "        if count == 0 || count > MAX_REGION {",
        [DVB_BOUNDS],
    ),
    (
        "a new region is not filled",
        "            fill = true;",
        "            fill |= false;",
        [DVB_BACKGROUND],
    ),
    (
        "a region's fill is not made",
        "        if fill {\n            region.pixels.fill(background);",
        "        if false && fill {\n            region.pixels.fill(background);",
        [DVB, DVB_RECEIVER, DVB_BACKGROUND],
    ),
    (
        "an 8-bit region's background is its 4-bit code",
        "            8 => data.at(at + 8),",
        "            8 => data.at(at + 9) >> 4,",
        [DVB],
    ),
    (
        "a 4-bit region's background is its 2-bit code",
        "            4 => data.at(at + 9) >> 4,",
        "            4 => (data.at(at + 9) >> 2) & 3,",
        [DVB, DVB_RECEIVER, DVB_BACKGROUND],
    ),
    (
        "a 2-bit region's background is its 4-bit code",
        "            _ => (data.at(at + 9) >> 2) & 3,",
        "            _ => data.at(at + 9) >> 4,",
        [DVB],
    ),
    (
        "an object placed outside its region is placed",
        "            if x >= width || y >= height || self.placements.len() >= MAX_PLACED {",
        "            if self.placements.len() >= MAX_PLACED {",
        [DVB_DAMAGED, DVB_DAMAGE],
    ),
    (
        "objects past 1024 are placed",
        "            if x >= width || y >= height || self.placements.len() >= MAX_PLACED {",
        "            if x >= width || y >= height {",
        [DVB_BOUNDS],
    ),
    (
        "an object named outside its region is not known",
        "            if !self.objects.contains(&object) {\n                self.objects.push(object);\n            }\n",
        "",
        [DVB_KNOWN],
    ),
    (
        "an object no region places is still known",
        "                self.objects.retain(|&o| o != object);",
        "                let _ = object;",
        [DVB_KNOWN],
    ),
    # CLUTs.
    (
        "a CLUT of the version held is read again",
        "        if clut.version == Some(version) {",
        "        if false && clut.version == Some(version) {",
        [DVB],
    ),
    (
        "a reduced-range entry is read as a full-range one",
        "            let (y, cr, cb, t) = if flags & 1 == 1 {",
        "            let (y, cr, cb, t) = if true {",
        [DVB],
    ),
    (
        "a Y of 0 is not transparent",
        "            let alpha = if y == 0 { 0 } else { 255 - t };",
        "            let alpha = 255 - t;",
        [DVB],
    ),
    (
        "an entry for the 2- and 4-bit tables goes into the 2-bit alone",
        "            if flags & 0x40 != 0",
        "            if flags & 0x40 != 0 && flags & 0x80 == 0",
        [DVB_ENTRIES, DVB_RECEIVER],
    ),
    (
        "the 2-bit table takes no entries",
        "            if flags & 0x80 != 0",
        "            if false && flags & 0x80 != 0",
        [DVB],
    ),
    (
        "the 8-bit table takes no entries",
        "            if flags & 0x20 != 0",
        "            if false && flags & 0x20 != 0",
        [DVB],
    ),
    # Objects.
    (
        "data for an object not known is read",
        "        if !self.objects.contains(&id) {",
        "        if false && !self.objects.contains(&id) {",
        [DVB_DRAWN, DVB_DAMAGE],
    ),
    (
        "characters are taken for pixels",
        "        if coding != 0 {",
        "        if coding > 1 {",
        [DVB_DAMAGE],
    ),
    (
        "field lengths past their segment are read",
        "        if first + top + bottom > at + size {",
        "        if false && first + top + bottom > at + size {",
        [DVB_DAMAGE],
    ),
    (
        "a bottom field of no bytes draws nothing",
        "            let (start, len) = if bottom > 0 {",
        "            let (start, len) = if true {",
        [DVB],
    ),
    (
        "the bottom field draws on the top field's lines",
        "        let (mut x, mut y) = (usize::from(p.x), usize::from(p.y) + field);",
        "        let (mut x, mut y) = (usize::from(p.x), usize::from(p.y) + field * 0);",
        [DVB, DVB_SHOWS],
    ),
    (
        "a field's 2-to-4 map is passed over",
        "                    map24 = [a >> 4, a & 0xF, b >> 4, b & 0xF];",
        "                    let _ = (a, b);",
        [DVB],
    ),
    (
        "a field's 2-to-8 map is passed over",
        "                    for (i, m) in map28.iter_mut().enumerate() {\n                        *m = data.at(at + i);",
        "                    for (i, m) in map28.iter_mut().enumerate() {\n                        let _ = (i, m);",
        [DVB],
    ),
    (
        "a field's 4-to-8 map is passed over",
        "                    for (i, m) in map48.iter_mut().enumerate() {\n                        *m = data.at(at + i);",
        "                    for (i, m) in map48.iter_mut().enumerate() {\n                        let _ = (i, m);",
        [DVB],
    ),
    (
        "2-bit data in a 4-bit region is not mapped",
        "                        4 => Some(&map24[..]),",
        "                        4 => None,",
        [DVB],
    ),
    (
        "2-bit data in an 8-bit region is not mapped",
        "                        8 => Some(&map28[..]),",
        "                        8 => None,",
        [DVB],
    ),
    (
        "4-bit data in an 8-bit region is not mapped",
        "                    let map = (region.depth == 8).then_some(&map48[..]);",
        "                    let map: Option<&[u8]> = None;",
        [DVB],
    ),
    (
        "4-bit data is drawn in a 2-bit region",
        "                    if region.depth < 4 {",
        "                    if region.depth < 2 {",
        [DVB],
    ),
    (
        "8-bit data is drawn in a 4-bit region",
        "                    if region.depth < 8 {",
        "                    if region.depth < 4 {",
        [DVB],
    ),
    (
        "a line's end code after the row's end is read as the next line's",
        "    if p < end && data.at(p) == 0 {",
        "    if false && p < end && data.at(p) == 0 {",
        [DVB_END_CODE, DVB],
    ),
    (
        "the non-modifying colour is drawn",
        "        if self.non_mod && code == 1 {",
        "        if false && self.non_mod && code == 1 {",
        [DVB_NON_MOD, DVB_RECEIVER],
    ),
    (
        "a pixel of the non-modifying colour holds no place",
        "        if self.non_mod && code == 1 {\n            self.x += n;",
        "        if self.non_mod && code == 1 {\n            let _ = n;",
        [DVB_NON_MOD, DVB_RECEIVER],
    ),
    # Display definitions.
    (
        "a display definition of the version held is read again",
        "        if self.display.version == Some(version) {",
        "        if false && self.display.version == Some(version) {",
        [DVB_HD],
    ),
    (
        "the window does not move the regions across",
        "            self.display.x = data.be16(at + 5);",
        "            let _ = data.be16(at + 5);",
        [DVB_HD],
    ),
    (
        "the window does not move the regions down",
        "            self.display.y = data.be16(at + 9);",
        "            let _ = data.be16(at + 9);",
        [DVB_HD],
    ),
    (
        "a first display definition too large is taken",
        "            if u64::from(width + 128) * 8 * u64::from(height + 128)",
        "            if false && u64::from(width + 128) * 8 * u64::from(height + 128)",
        [DVB_RESET],
    ),
    # Seeking.
    (
        "an acquisition point begins no epoch",
        "            return matches!((data.at(at + 1) >> 2) & 3, 1 | 2);",
        "            return matches!((data.at(at + 1) >> 2) & 3, 2);",
        [DVB_EPOCH],
    ),
]

if __name__ == "__main__":
    # A filter goes to the tables it names a row of, and only those: the
    # harness refuses a filter that selects nothing.
    only = sys.argv[1:]
    tables = [
        (SRC / "subtitle" / "pgs.rs", PICTURES),
        (SRC / "subtitle" / "vobsub.rs", DVD),
        (SRC / "subtitle" / "dvb.rs", DVB_ROWS),
        (SRC / "subtitle.rs", READER),
        (SRC / "lib.rs", LIB),
    ]
    names = [name for _, rows in tables for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    results = [0]
    for src, rows in tables:
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        results.append(sweep(src, rows, "videocodec", timeout=600, only=mine or None, targets=TARGETS))
    raise SystemExit(max(results))
