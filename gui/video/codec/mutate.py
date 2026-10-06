"""Mutation test for videocodec's subtitles: Blu-ray's PGS, DVD's VobSub and
DVB's pictures; WebVTT, TTML and CEA-608 captions in MP4, with the XML TTML
is read from and the joining of their cues' pieces; and the reader that
gives every format's cues.

Each row puts back one way of not showing what the reference shows -- a rule
of FFmpeg's `pgssub` decoder dropped, a colour rounded otherwise, a crop made
as FFmpeg makes it, a TTML time read otherwise than ttconv reads it, a
caption command done otherwise than the FCC's rules say -- or of
not giving the cues a player needs, and names the tests that have to notice:
the fixtures' (`tests/subtitles.rs`, held to their references' answers by
`tests/data/generate_subtitle_fixtures.py`) and the module's own.

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

# WebVTT in MP4: each sample the cues showing through it, joined again
# (isovtt.rs).
VTT_SAMPLE = "a_sample_is_its_cues_in_order_and_an_empty_one_none"
VTT_SIZES = "a_box_of_size_zero_runs_to_the_end_and_one_of_size_one_is_long"
VTT_WHOLE = "a_cue_split_across_samples_is_whole_again"
VTT_CONTIGUOUS = "a_cue_goes_on_only_into_the_sample_its_last_one_ended_at"
VTT_TWICE = "two_cues_alike_in_one_sample_are_two_cues"
VTT_ORDER = "cues_come_in_the_order_they_began"
VTT_TOGETHER = "cues_that_began_together_come_in_their_samples_order"

WEBVTT_MP4 = "webvtt_in_mp4"
WEBVTT_JOINED = "webvtt_in_mp4_cut_into_samples_is_joined_again"
WEBVTT_MP4_FORGETS = "a_seek_in_webvtt_in_mp4_forgets_the_cue_showing_before_it"
WEBVTT_MP4_DAMAGE = "a_damaged_webvtt_sample_in_mp4_shows_nothing_and_is_counted"

# Pieces of cues joined again (joined.rs), for WebVTT's samples and TTML's.
J_ONE = "a_cue_in_samples_one_after_another_is_one_cue"
J_GAP = "a_cue_after_a_gap_is_another_cue"
J_WAITS = "an_ended_cue_waits_for_one_begun_before_it"
J_ALIKE = "two_cues_alike_are_two_cues_each_going_on_once"
J_INSIDE = "pieces_inside_a_sample_join_where_they_meet"
J_REFERENCE = "whole_samples_join_as_the_first_joining_did"

# TTML in MP4 (ttml.rs, xml.rs): the module's own.
TT_TIMES = "time_expressions_are_read_exactly"
TT_NEAREST = "nanoseconds_are_the_nearest"
TT_EXACT_NS = "nanoseconds_are_exact_however_large_the_terms"
TT_ORDER = "times_order_exactly_however_large_the_terms"
TT_STRETCHES = "a_documents_stretches_are_what_it_shows_at_every_moment"
TT_SLOW = "a_paragraph_made_to_be_slow_is_given_up_on"
TT_WHITE = "white_space_is_ttconvs"
TT_REFERENCED = "styles_inline_over_referenced_and_the_last_reference_over_the_first"
TT_SPAN = "a_span_shows_from_its_own_begin"
TT_SEQ = "text_lasts_as_long_as_its_parent_and_a_seq_puts_children_in_turn"
TT_LATE = "a_container_beginning_late_lasts_until_its_last_child_ends"
TT_OWN_REGION = "a_paragraph_naming_another_region_than_its_body_shows_in_none"
TT_HIDDEN = "hidden_text_and_display_none_show_nothing"
TT_THIRDS = "a_region_s_place_is_said_in_thirds"
TT_NOT_ALLOWED = "an_element_where_ttml_allows_none_is_passed_over_alone"
XML_ROOT = "a_document_is_its_root_names_resolved"
XML_REFERENCES = "references_cdata_and_line_endings_are_read"
XML_SCOPED = "a_default_namespace_is_scoped_to_its_element"
XML_LAUGHS = "a_doctype_is_passed_over_and_its_entities_never_read"
XML_REFUSED = "what_is_not_well_formed_is_refused"
XML_DEEP = "elements_nest_only_so_deep"
XML_SCOPE_ENDS = "a_declaration_is_in_scope_until_its_element_ends"
XML_EXPAT_LINES = "line_endings_and_white_space_are_read_as_expat_reads_them"
XML_EDGES = "what_is_well_formed_at_the_edges_is_read"
XML_CHARS = "the_characters_xml_allows_are_expats"
XML_ONE_NAMESPACE = "a_namespace_is_one_string_however_many_names_are_in_it"
XML_BORROWED = "what_is_as_written_is_borrowed"
# The TTML fixtures'.
TTML_MP4 = "ttml_in_mp4"
TTML_SPLIT = "ttml_in_samples_of_two_seconds_is_joined_again"
TTML_TIMING = "ttml_timing"
TTML_TIMING_SPLIT = "ttml_timing_cut_into_samples"
TTML_OWN_STRETCH = "ttml_shows_each_sample_only_in_its_own_stretch"
TTML_REGIONS = "ttml_regions"
TTML_DEFAULT = "ttml_without_regions"
TTML_SEEK = "a_seek_in_ttml_gives_the_cues_showing_then"
TTML_FORGETS = "a_seek_in_ttml_forgets_what_was_read"
FORMATS = "which_formats_are_text_which_pictures_and_which_read"

# CEA-608 captions (cea608.rs): the module's own.
C_POPON = "a_pop_on_caption_shows_from_end_of_caption_to_the_erase"
C_REPLACED = "a_caption_replaced_ends_where_the_next_begins"
C_SAME = "the_same_caption_shown_again_ends_nothing"
C_EXTENDED = "an_extended_character_takes_the_place_of_the_one_before"
C_PARITY = "a_character_failing_parity_is_a_solid_block"
C_PARITY_CODE = "a_control_code_failing_parity_is_ignored"
C_REDUNDANT = "a_control_code_is_redundant_only_in_the_very_next_pair"
C_CHANNEL = "another_channel_and_text_mode_show_nothing"
C_FIRST_COLUMN = "backspace_in_the_first_column_is_nothing_and_a_full_row_overwrites_its_last"
C_MIDROW = "mid_row_codes_are_spaces_and_a_colour_turns_italics_off"
C_ROLL = "roll_up_rolls_its_window_and_a_line_shows_from_its_first_character"
C_ROLL_AGAIN = "roll_up_sent_again_before_each_line_ends_nothing"
C_ROLL_ERASES = "roll_up_erases_a_pop_on_caption"
C_WINDOW = "a_pac_naming_another_base_row_moves_the_window_intact"
C_BLANKED = "typing_that_leaves_the_screen_blank_ends_a_stretch"
C_PAINT = "paint_on_shows_from_its_first_character_to_the_erase"
C_COLUMN = "a_row_keeps_its_column_and_the_caption_its_third"
C_ATOMS = "a_samples_pairs_are_its_field_1_atoms"
# The CEA-608 fixtures'.
CEA_POPON = "cea608_pop_on"
CEA_STYLES = "cea608_styles"
CEA_CHARS = "cea608_characters"
CEA_ROLLUP = "cea608_roll_up"
CEA_MODES = "cea608_switching_style_and_a_window_moved"
CEA_PAINT = "cea608_paint_on"
CEA_RULES = "cea608_the_rules_no_reader_keeps"
CEA_SEEK = "a_seek_in_cea608_gives_the_screen_showing_then"
CEA_FORGETS = "a_seek_in_cea608_forgets_the_screen_read_before"

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
    # WebVTT in MP4.
    (
        "MP4's WebVTT is read as Matroska's",
        '            SubtitleFormat::WebVtt if chosen.codec_id == b"wvtt" => {',
        "            SubtitleFormat::WebVtt if false => {",
        [WEBVTT_MP4, WEBVTT_JOINED],
    ),
    (
        "MP4's WebVTT gives no cue still showing at the track's end",
        "            let whole = joined.finish();\n            self.give(whole);",
        "            let whole: Vec<isovtt::Whole> = {\n                let _ = joined.finish();\n                Vec::new()\n            };\n            self.give(whole);",
        [WEBVTT_MP4, WEBVTT_JOINED],
    ),
    (
        "damage in MP4's WebVTT is not counted",
        "        if !readable {",
        "        if false && !readable {",
        [WEBVTT_MP4_DAMAGE],
    ),
    (
        "a seek in MP4's WebVTT keeps the cues showing before it",
        "            Reader::IsoVtt(joined) => joined.clear(),",
        "            Reader::IsoVtt(_) => {}",
        [WEBVTT_MP4_FORGETS],
    ),
    # TTML in MP4.
    (
        "MP4's TTML gives no cue still showing at the track's end",
        "            let whole = joined.finish();\n            self.give_shown(whole);",
        "            let _ = joined.finish();\n            self.give_shown(Vec::new());",
        [TTML_MP4, TTML_SPLIT, TTML_TIMING],
    ),
    (
        "a TTML sample that is no document is not counted",
        "        if shown.is_none() {",
        "        if false && shown.is_none() {",
        [TTML_MP4],
    ),
    (
        "a seek in MP4's TTML keeps what was read before it",
        "            Reader::Ttml(joined) => joined.clear(),",
        "            Reader::Ttml(_) => {}",
        [TTML_FORGETS],
    ),
    # CEA-608 captions in MP4.
    (
        "CEA-608 samples are not read as captions",
        "                || self.captions(&sample, ticks)",
        "                || false",
        [CEA_POPON],
    ),
    (
        "the screen still showing at the track's end is lost",
        "            let last = decoder.finish(*end);\n            self.give_caption(last);",
        "            let _ = decoder.finish(*end);\n            self.give_caption(None);",
        [CEA_MODES],
    ),
    (
        "a seek in CEA-608 reads from the time, not before it",
        "            ticks = time::to_ticks(time.saturating_sub(CAPTION_LOOKBACK), self.time_base);",
        "            ticks = time::to_ticks(time, self.time_base);",
        [CEA_SEEK],
    ),
    (
        "a seek in CEA-608 keeps the screen read before it",
        "                **decoder = cea608::Decoder::default();",
        "                let _ = &decoder;",
        [CEA_FORGETS],
    ),
    (
        "TTML's cues are given without a seek's time",
        "            let (start, end) = (w.start.to_ns(), w.end.to_ns());\n            if self.kept(start, end) {",
        "            let (start, end) = (w.start.to_ns(), w.end.to_ns());\n            if true {",
        [TTML_SEEK],
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

# WebVTT in MP4's samples. The check of a box's size before its body is
# taken has no row, its mutant doing nothing (`get` refuses the same sizes).
# The joining of their cues is JOINED's.
ISOVTT = [
    (
        "a cue's identifier is not read",
        '                b"iden" => &mut cue.id,',
        '                b"iden" => continue,',
        [VTT_SAMPLE, WEBVTT_JOINED],
    ),
    (
        "a cue's settings are not read",
        '                b"sttg" => &mut cue.settings,',
        '                b"sttg" => continue,',
        [VTT_SAMPLE, WEBVTT_MP4, WEBVTT_JOINED],
    ),
    (
        "a box of size zero is damage",
        "            0 => (8, rest.len()),",
        "            0 => return None,",
        [VTT_SIZES],
    ),
    (
        "a box of size one keeps its size in its body",
        "                (16, usize::try_from(large).ok()?)",
        "                (8, usize::try_from(large).ok()?)",
        [VTT_SIZES],
    ),
]

# Pieces of cues joined again: WebVTT's samples' (through isovtt.rs, whose
# tests are WebVTT's rules for it) and TTML's paragraphs'. Three pieces have
# no row, their mutants changing only when a cue is given and not which or
# in what order: the cues reaching a time before a piece's ended there, the
# cues reaching a time no piece goes on from ended at it, and those reaching
# a time before the sample's end ended at the end -- each would instead end
# at the next sample, or at the track's end, where they are given the same.
JOINED = [
    (
        "each piece of a cue is a cue of its own",
        "                let (order, start) = match reaching.get_mut(&cue).and_then(BTreeSet::pop_first) {",
        "                let (order, start) = match None::<(u64, T)> {",
        [J_ONE, J_INSIDE, J_REFERENCE, VTT_WHOLE, WEBVTT_JOINED, TTML_SPLIT, TTML_TIMING_SPLIT],
    ),
    (
        "a cue goes on across a gap",
        "            self.end_before(from);\n            let mut reaching = self.open.remove(&from).unwrap_or_default();",
        "            let mut reaching = self.open.pop_first().map(|(_, r)| r).unwrap_or_default();",
        [J_GAP, VTT_CONTIGUOUS],
    ),
    (
        "two cues alike go on as one",
        "reaching.get_mut(&cue).and_then(BTreeSet::pop_first)",
        "reaching.get(&cue).and_then(|s| s.first().copied())",
        [J_ALIKE, VTT_TWICE],
    ),
    (
        "an ended cue is given before one that began earlier",
        "            if first_open.is_some_and(|first| (next.start, next.order) >= first) {",
        "            if false && first_open.is_some_and(|first| (next.start, next.order) >= first) {",
        [J_WAITS, J_REFERENCE, VTT_WHOLE, VTT_ORDER, WEBVTT_JOINED],
    ),
    (
        "cues that began together come in the order they ended",
        "        (&self.start, self.order).cmp(&(&other.start, other.order))",
        "        (&self.start, &self.end).cmp(&(&other.start, &other.end))",
        [VTT_TOGETHER, J_REFERENCE],
    ),
]

# TTML: its times, its documents worked out paragraph by paragraph, its
# styles, regions and white space, and how it is said. The stretches test
# (TT_STRETCHES) holds the paragraph-by-paragraph working to the
# whole-document one over the same parsed document, so it names only the
# rows of the former: a document read wrongly is read wrongly by both.
TTML = [
    # Times.
    (
        "a clock time's frames are not counted",
        "                total = total.add(Ratio::whole(f).div(self.frame_rate)?)?;",
        "                let _ = f;",
        [TT_TIMES, TTML_TIMING, TTML_TIMING_SPLIT],
    ),
    (
        "a tick is a second",
        '            "t" => Ratio::whole(1).div(self.tick_rate)?,',
        '            "t" => Ratio::whole(1),',
        [TT_TIMES, TTML_TIMING, TTML_TIMING_SPLIT],
    ),
    (
        "a fraction's comparison is not turned round",
        "                        reversed = !reversed;",
        "                        reversed = reversed;",
        [TT_ORDER],
    ),
    (
        "half a nanosecond rounds down",
        "        if rest.saturating_mul(2) >= den {",
        "        if rest.saturating_mul(2) > den {",
        [TT_NEAREST, TT_EXACT_NS],
    ),
    (
        "the long multiplication adds half",
        "            rest = rest.saturating_add(a);",
        "            rest = rest.saturating_add(a / 2);",
        [TT_NEAREST, TT_EXACT_NS],
    ),
    # Timing, as ttconv's and TTML's.
    (
        "a sequence's children all begin with it",
        "    let implicit_begin = if parent.seq {",
        "    let implicit_begin = if false && parent.seq {",
        [TT_SEQ, TTML_TIMING, TTML_TIMING_SPLIT],
    ),
    (
        "a parallel container ends with its first child to end",
        "                        (Some(a), Some(b)) => Some(a.max(b)),",
        "                        (Some(a), Some(b)) => Some(a.min(b)),",
        [TT_LATE, TTML_TIMING],
    ),
    (
        "a container beginning late ends early, as ttconv's",
        "                let end = desired_end.and_then(|end| end.add(timing.desired_begin));",
        "                let end = desired_end;",
        [TT_LATE],
    ),
    (
        "an element not allowed where it is takes every one after it",
        "                if *child.name.namespace != *TT || !allowed.contains(&child.name.local) {\n                    continue;",
        "                if *child.name.namespace != *TT || !allowed.contains(&child.name.local) {\n                    break;",
        [TT_NOT_ALLOWED],
    ),
    # Paragraph by paragraph.
    (
        "a paragraph is worked out only where its stretch begins",
        "                changes_inside(child, f.begin, f.end, (start, stop), &mut points, &mut cost);",
        "                let _ = (child, &mut points, &mut cost);",
        [TT_STRETCHES, TTML_TIMING],
    ),
    (
        "a paragraph associated with no region shows in none",
        "        regions_inside(&f.p.children, &mut named);",
        "        let _ = &f.p.children;",
        [TT_STRETCHES, TTML_REGIONS],
    ),
    (
        "an element naming another region than its ancestors' shows in its own",
        "        && own != theirs",
        "        && own != theirs && false",
        [TT_STRETCHES, TTML_REGIONS],
    ),
    (
        "a paragraph not displayed is found",
        "    if c.styles.display_none == Some(true) {\n        return;\n    }\n    let here = Ancestors {",
        "    if false && c.styles.display_none == Some(true) {\n        return;\n    }\n    let here = Ancestors {",
        [TT_STRETCHES, TTML_REGIONS],
    ),
    (
        "a paragraph's stretches of showing the same are not one",
        "                        (Some((_, end, shown)), Some(text)) if *shown == text => *end = until,",
        "                        (Some((_, end, shown)), Some(text)) if false && *shown == text => *end = until,",
        [TT_STRETCHES],
    ),
    (
        "a document made to be slow is read through",
        "                    budget = budget.checked_sub(cost.saturating_add(1))?;",
        "                    budget = budget.saturating_sub(cost.saturating_add(1));",
        [TT_SLOW],
    ),
    # Styles, white space, and how it is said.
    (
        "a referenced style over the element's own",
        "            s = s.under(r);",
        "            s = r.under(&s);",
        [TT_REFERENCED, TTML_MP4, TTML_SPLIT],
    ),
    (
        "a leading space is kept after white space",
        "        if after_space && collapsed.starts_with(' ') {",
        "        if false && after_space && collapsed.starts_with(' ') {",
        [TT_WHITE, TTML_MP4, TTML_SPLIT],
    ),
    (
        "hidden text is shown",
        "            Piece::Text { style, .. } if style.hidden => {}",
        "            Piece::Text { style, .. } if false && style.hidden => {}",
        [TT_HIDDEN, TTML_MP4, TTML_SPLIT],
    ),
    (
        "white is written as a colour",
        "                    writer.op(Op::Colour((want != rgb(WHITE)).then_some(want)));",
        "                    writer.op(Op::Colour(Some(want)));",
        [TTML_MP4, TTML_SPLIT],
    ),
    (
        "an anchor at a third goes in the third before",
        "        if v < one {",
        "        if v <= one {",
        [TT_THIRDS],
    ),
]

# CEA-608 captions: the FCC's rules for each command, the characters, what a
# cue is, and how it is said.
CEA608 = [
    # Pairs.
    (
        "a character failing parity is shown as sent",
        "            if !odd(b) {\n                self.put('\\u{2588}');",
        "            if false && !odd(b) {\n                self.put('\\u{2588}');",
        [C_PARITY, CEA_RULES],
    ),
    (
        "a control code failing parity is acted on",
        "            if !odd(b1) || !odd(b2) || self.last == Some([b1, b2]) {",
        "            if self.last == Some([b1, b2]) {",
        [C_PARITY_CODE, CEA_RULES],
    ),
    (
        "every control code acted on twice",
        "            if !odd(b1) || !odd(b2) || self.last == Some([b1, b2]) {",
        "            if !odd(b1) || !odd(b2) {",
        [C_POPON, C_REDUNDANT, CEA_POPON],
    ),
    (
        "a control code repeated pairs apart is ignored",
        "        if [b1, b2] == [0x80, 0x80] {\n            self.last = None;",
        "        if [b1, b2] == [0x80, 0x80] {\n            let _ = self.last;",
        [C_REDUNDANT, CEA_RULES],
    ),
    (
        "another channel's captions are shown",
        "            self.ours = c1 < 0x18;",
        "            self.ours = true;",
        [C_CHANNEL, CEA_RULES],
    ),
    (
        "text mode's characters are captions",
        "        if !self.ours || matches!(self.mode, Mode::Unset | Mode::Text) {",
        "        if !self.ours || matches!(self.mode, Mode::Unset) {",
        [C_CHANNEL, CEA_RULES],
    ),
    (
        "a pop-on caption is loaded on the screen",
        "        if self.mode == Mode::PopOn {",
        "        if false {",
        [C_POPON, CEA_POPON],
    ),
    (
        "the samples' second field read as the first",
        '            if kind == b"cdat" {',
        '            if kind == b"cdat" || kind == b"cdt2" {',
        [C_ATOMS],
    ),
    # Characters.
    (
        "a mid-row code is no space",
        "                };\n                self.put(' ');\n                false",
        "                };\n                false",
        [C_MIDROW, CEA_STYLES],
    ),
    (
        "a colour mid-row code keeps italics",
        "                    Style {\n                        colour: code,\n                        italic: false,\n                        underline,\n                    }\n                };\n                self.put(' ');",
        "                    Style {\n                        colour: code,\n                        italic: self.style.italic,\n                        underline,\n                    }\n                };\n                self.put(' ');",
        [C_MIDROW, CEA_STYLES],
    ),
    (
        "a special character is the next one along",
        "                if let Some(&ch) = SPECIAL.get(usize::from(c2 & 0x0F)) {",
        "                if let Some(&ch) = SPECIAL.get(usize::from(c2.wrapping_add(1) & 0x0F)) {",
        [CEA_CHARS],
    ),
    (
        "an extended character backs over nothing",
        "                self.col = self.col.saturating_sub(1);\n                let table",
        "                let table",
        [C_EXTENDED, CEA_CHARS],
    ),
    (
        "the two extended sets swapped",
        "                let table = EXTENDED.get(usize::from(c1 & 1));",
        "                let table = EXTENDED.get(usize::from(!c1 & 1));",
        [C_EXTENDED, CEA_CHARS],
    ),
    # Commands.
    (
        "a PAC's indent is not kept",
        "            self.col = usize::from(code & 7).saturating_mul(4);",
        "            self.col = 0;",
        [C_COLUMN, CEA_POPON, CEA_PAINT],
    ),
    (
        "a tab offset is one column short",
        "                    .saturating_add(usize::from(c2 & 0x03))",
        "                    .saturating_add(usize::from(c2 & 0x02))",
        [CEA_PAINT],
    ),
    (
        "a PAC in roll-up leaves the window where it was",
        "        let moved = self.mode == Mode::RollUp && row != self.row;",
        "        let moved = false;",
        [C_WINDOW, CEA_MODES],
    ),
    (
        "Backspace in the first column erases it",
        "                if self.col > 0 {\n                    self.col = self.col.saturating_sub(1);",
        "                if true {\n                    self.col = self.col.saturating_sub(1);",
        [C_FIRST_COLUMN],
    ),
    (
        "Delete to End of Row spares the cursor's column",
        ".and_then(|r| r.get_mut(col..))",
        ".and_then(|r| r.get_mut(col.saturating_add(1)..))",
        [CEA_PAINT],
    ),
    (
        "Roll-Up keeps a pop-on caption",
        "                if self.mode != Mode::RollUp {\n                    *self.displayed = BLANK;",
        "                if self.mode != Mode::RollUp {\n                    let _ = BLANK;",
        [C_ROLL_ERASES, CEA_MODES],
    ),
    (
        "a Carriage Return brings the top line back to the base row",
        "                    rows.rotate_left(1);\n                    if let Some(last) = rows.last_mut() {\n                        *last = BLANK_ROW;\n                    }",
        "                    rows.rotate_left(1);",
        [C_ROLL, CEA_ROLLUP],
    ),
    (
        "End of Caption shows nothing",
        "                core::mem::swap(&mut self.displayed, &mut self.hidden);",
        "                let _ = (&self.displayed, &self.hidden);",
        [C_POPON, CEA_POPON],
    ),
    (
        "Erase Displayed Memory erases nothing",
        "            0x2C => {\n                *self.displayed = BLANK;",
        "            0x2C => {\n                let _ = BLANK;",
        [C_POPON, CEA_POPON],
    ),
    (
        "Erase Non-Displayed Memory erases nothing",
        "            0x2E => *self.hidden = BLANK,",
        "            0x2E => {}",
        [CEA_POPON],
    ),
    # What a cue is.
    (
        "a command that changes nothing ends the stretch",
        "        if after == self.screen {",
        "        if false {",
        [C_SAME, C_ROLL_AGAIN, CEA_ROLLUP],
    ),
    (
        "typing that blanks the screen ends nothing",
        "        if flushed || self.screen.is_empty() {",
        "        if flushed {",
        [C_BLANKED],
    ),
    (
        "a stretch begins at the command, the screen still blank",
        "            if !self.screen.is_empty() {\n                self.since = Some(t);\n            }",
        "            self.since = Some(t);",
        [CEA_ROLLUP],
    ),
    # How it is said.
    (
        "an empty cell before a character is a space SRT may drop",
        "cell.map_or(('\\u{a0}', WHITE)",
        "cell.map_or((' ', WHITE)",
        [C_COLUMN, CEA_POPON],
    ),
    (
        "the middle third is the bottom's",
        "        } else if twice < 20 {\n            4",
        "        } else if twice < 20 {\n            1",
        [C_WINDOW, CEA_POPON, CEA_MODES],
    ),
    (
        "the grid's face is not named",
        '        w.op(Op::Face(Some("Monospace".to_owned())));',
        "",
        [C_COLUMN, CEA_POPON],
    ),
]

# What TTML needs of XML.
XML = [
    # What a run of the document reads as.
    (
        "line endings are kept as written",
        "        '\\r' => true,",
        "        '\\r' => false,",
        [XML_REFERENCES, XML_EXPAT_LINES, XML_BORROWED],
    ),
    (
        "a tab or a line feed in a value is kept",
        "        '\\n' | '\\t' => run == Run::Value,",
        "        '\\n' | '\\t' => false,",
        [XML_REFERENCES, XML_EXPAT_LINES, XML_BORROWED],
    ),
    (
        "a carriage return in a value is a line feed",
        "            out.push(if run == Run::Value { ' ' } else { '\\n' });",
        "            out.push('\\n');",
        [XML_EXPAT_LINES],
    ),
    (
        "a reference in a CDATA section is read",
        "        '&' => run != Run::Cdata,",
        "        '&' => true,",
        [XML_REFERENCES, XML_BORROWED],
    ),
    (
        "what is as written is copied",
        "        return Ok(Cow::Borrowed(s));",
        "        return Ok(Cow::Owned(s.to_owned()));",
        [XML_BORROWED],
    ),
    (
        "an ampersand's reference is another character",
        "        \"amp\" => '&',",
        "        \"amp\" => '+',",
        [XML_REFERENCES, TTML_SPLIT],
    ),
    # The characters XML allows.
    (
        "a character XML does not allow is read",
        "    if !text.chars().all(is_xml_char) {",
        "    if false {",
        [XML_REFUSED, XML_CHARS],
    ),
    (
        "a reference to a character XML does not allow is read",
        "                .filter(|&c| is_xml_char(c))",
        "                .filter(|&c| c != '\\0')",
        [XML_REFUSED, XML_CHARS],
    ),
    (
        "a backspace is a character XML allows",
        "'\\0'..='\\u{8}'",
        "'\\0'..='\\u{7}'",
        [XML_CHARS],
    ),
    (
        "a tab is a character XML refuses",
        "'\\0'..='\\u{8}'",
        "'\\0'..='\\u{9}'",
        [XML_CHARS, XML_REFERENCES, XML_EXPAT_LINES],
    ),
    (
        "a form feed is a character XML allows",
        "'\\u{b}' | '\\u{c}'",
        "'\\u{b}'",
        [XML_CHARS],
    ),
    (
        "U+000E is a character XML allows",
        "'\\u{e}'..='\\u{1f}'",
        "'\\u{f}'..='\\u{1f}'",
        [XML_CHARS],
    ),
    (
        "U+001F is a character XML allows",
        "'\\u{e}'..='\\u{1f}'",
        "'\\u{e}'..='\\u{1e}'",
        [XML_CHARS],
    ),
    (
        "U+FFFE is a character XML allows",
        "'\\u{fffe}' | '\\u{ffff}'",
        "'\\u{ffff}'",
        [XML_CHARS, XML_REFUSED],
    ),
    (
        "U+FFFF is a character XML allows",
        "'\\u{fffe}' | '\\u{ffff}'",
        "'\\u{fffe}'",
        [XML_CHARS, XML_REFUSED],
    ),
    # Elements, and a DOCTYPE.
    (
        "nesting goes one past the bound",
        "        if self.open.len() >= MAX_DEPTH {",
        "        if self.open.len() > MAX_DEPTH {",
        [XML_DEEP],
    ),
    (
        "a DOCTYPE ends at its first '>'",
        "                (None, '>') if depth == 0 => {",
        "                (None, '>') => {",
        [XML_LAUGHS],
    ),
    # An element's attributes, each once.
    (
        "an attribute written twice is read",
        "            .written\n            .windows(2)\n            .any(|w| matches!(w, [a, b] if a == b))",
        "            .written\n            .windows(2)\n            .any(|w| matches!(w, [a, b] if a == b) && false)",
        [XML_REFUSED],
    ),
    (
        "attributes written are told apart unsorted",
        "        self.written.sort_unstable();",
        "",
        [XML_REFUSED],
    ),
    (
        "two attributes resolving alike are read",
        "            .expanded\n            .windows(2)\n            .any(|w| matches!(w, [a, b] if a == b))",
        "            .expanded\n            .windows(2)\n            .any(|w| matches!(w, [a, b] if a == b) && false)",
        [XML_REFUSED],
    ),
    (
        "attributes resolved are told apart unsorted",
        "        self.expanded.sort_unstable();",
        "",
        [XML_REFUSED],
    ),
    # Namespaces: in scope, and each one string.
    (
        "a declaration stays in scope after its element",
        "                namespaces.pop();",
        "                let _ = &namespaces;",
        [XML_SCOPE_ENDS, XML_SCOPED],
    ),
    (
        "an outer declaration hides an inner one",
        "                .and_then(|namespaces| namespaces.last())",
        "                .and_then(|namespaces| namespaces.first())",
        [XML_SCOPE_ENDS],
    ),
    (
        "a prefix bound by nothing is in no namespace",
        "                None => return Err(XmlError::UnboundPrefix),",
        "                None => &self.none,",
        [XML_REFUSED, XML_SCOPE_ENDS],
    ),
    (
        "each name its namespace's own copy",
        "            namespace: Rc::clone(namespace),",
        "            namespace: Rc::from(&**namespace),",
        [XML_ONE_NAMESPACE, XML_REFUSED],
    ),
    (
        "a namespace declared twice is two strings",
        "        if let Some(known) = self.namespaces.get(uri) {\n            return Rc::clone(known);\n        }\n",
        "",
        [XML_ONE_NAMESPACE, XML_REFUSED],
    ),
    # The reserved prefixes and namespaces, and declarations' names.
    (
        "the xmlns prefix may be declared",
        '        if prefix == "xmlns" {',
        "        if false {",
        [XML_REFUSED],
    ),
    (
        "the xml prefix may be bound to another namespace",
        "            if uri != XML_NAMESPACE {",
        "            if false {",
        [XML_REFUSED],
    ),
    (
        "the xml prefix's own declaration is refused",
        '        if prefix == "xml" {\n            if uri',
        "        if false {\n            if uri",
        [XML_EDGES, XML_REFUSED],
    ),
    (
        "a prefix may be bound to a reserved namespace",
        "        } else if uri == XML_NAMESPACE || uri == XMLNS_NAMESPACE {",
        "        } else if false {",
        [XML_REFUSED],
    ),
    (
        "a prefix may be bound to no namespace",
        "        } else if uri.is_empty() && !prefix.is_empty() {",
        "        } else if false {",
        [XML_REFUSED],
    ),
    (
        "a declaration's prefix may be no name",
        "        Some(prefix) if prefix.is_empty() || prefix.contains(':') => {",
        "        Some(prefix) if false => {",
        [XML_REFUSED],
    ),
]

CONTAINER = [
    (
        "MP4's WebVTT is not offered as subtitles",
        '                        || (t.kind == mp4::TrackKind::Data && t.codec_tag == *b"wvtt")',
        "                        || false",
        [WEBVTT_MP4, WEBVTT_JOINED],
    ),
    (
        "MP4's WebVTT is offered as a format not read",
        '                        } else if t.codec_tag == *b"wvtt" {',
        "                        } else if false {",
        [WEBVTT_MP4],
    ),
    (
        "MP4's CEA-608 is offered as a format not read",
        "                        } else if t.codec == mp4::Codec::Cea608 {",
        "                        } else if false {",
        [CEA_POPON],
    ),
    (
        "MP4's TTML is offered as a format not read",
        "                        } else if t.codec == mp4::Codec::Ttml {",
        "                        } else if false {",
        [TTML_MP4, TTML_DEFAULT],
    ),
]

LIB = [
    (
        "DVB's pictures are not read",
        "        self.is_text() || matches!(self, Self::Pgs | Self::VobSub | Self::Dvb)",
        "        self.is_text() || matches!(self, Self::Pgs | Self::VobSub)",
        [OPENED],
    ),
    (
        "CEA-608 is not text",
        "                | Self::Cea608\n",
        "",
        [FORMATS],
    ),
    (
        "TTML is not text",
        "                | Self::Ttml\n",
        "",
        [FORMATS],
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
        (SRC / "subtitle" / "isovtt.rs", ISOVTT),
        (SRC / "subtitle" / "joined.rs", JOINED),
        (SRC / "subtitle" / "ttml.rs", TTML),
        (SRC / "subtitle" / "xml.rs", XML),
        (SRC / "subtitle" / "cea608.rs", CEA608),
        (SRC / "subtitle.rs", READER),
        (SRC / "container.rs", CONTAINER),
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
