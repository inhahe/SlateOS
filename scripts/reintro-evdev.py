#!/usr/bin/env python3
"""Prove the evdev input tests are regression tests, one defect at a time.

The fourth of these harnesses in the input area (after `reintro-mouse-page.py`
and `reintro-reload-input.py`, which covered the Settings page and the reload
verb), and the one that covers the other end of the same wire:
`known-issues.md` -> `TD-COMPOSITOR-HAS-NO-LOCAL-INPUT`, closed by
`gui/compositor/src/present/evdev.rs` and the `Paired` adapter in
`gui/compositor/src/present.rs`.

Why this module in particular needs proving rather than merely testing: none of
it can be exercised on the machine this tree is compiled on. The four syscalls
are behind a trait and every test drives a fake device, which is what makes a
suite here *look* thorough while being satisfiable by almost anything. A fake
that scripts bytes and an implementation that decodes them can agree on a
mistake with nobody to contradict them -- there is no real keyboard in the loop
to notice that Left arrow came out as keypad 4, that vertical scroll went the
wrong way, or that a Shift held across a dropped packet stayed down for ever.
Each defect below is one of those, put back, and the test that names it has to
name it back.

The defects are also chosen to be the shape the real bugs would be: an inverted
sign, a fallback consulted before the authority instead of after it, a clamp
deleted, a `take()` that became a read, a per-device check dropped so one
device's drop releases another's keys. None of them stop the module compiling
and none of them would be visible in a screenshot.

Restore discipline as in the companions: byte snapshots taken up front, written
back unconditionally in a `finally`, verified by SHA-256. A reverse
search-and-replace is not good enough -- if a patch half-applied, or a formatter
ran, or the process died between the write and the undo, a reverse replace
silently leaves the tree modified while claiming success.

Two modes:

- `--check` matches every defect's pattern against the snapshot and builds
  nothing. Seconds, no toolchain, and it answers the only question that rots on
  its own: has a rename or a rustfmt pass stopped this defect applying?
- No flag: the real sweep. Apply, run the tests, restore, report.

Filter either with defect letters: `reintro-evdev.py T U V`.
"""

import hashlib
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-gnu"

EVDEV = "gui/compositor/src/present/evdev.rs"
UAPI = "gui/compositor/src/present/evdev/uapi.rs"
PRESENT = "gui/compositor/src/present.rs"

# Stand-in for a test name, in the "expected to fail" slot, meaning "this one
# is defended by a `const _: () = assert!(..)` and the catch is a build break".
COMPILE = "<compile>"

# (name, file, [(old, new), ...], [packages], [tests expected to fail])
DEFECTS = [
    # -----------------------------------------------------------------------
    # The pointer
    # -----------------------------------------------------------------------
    (
        "A: the pointer starts at the origin instead of the centre",
        EVDEV,
        [("        pointer.x = f64::from(width).mul_add(0.5, 0.0) as f32;\n"
          "        pointer.y = f64::from(height).mul_add(0.5, 0.0) as f32;",
          "        pointer.x = 0.0;\n"
          "        pointer.y = 0.0;")],
        ["compositor"],
        ["the_pointer_starts_at_the_centre_rather_than_under_a_control"],
    ),
    (
        "B: the far edge is the width, so the last column cannot be clicked",
        EVDEV,
        [("        let max_x = self.width.saturating_sub(1) as f32;\n"
          "        let max_y = self.height.saturating_sub(1) as f32;",
          "        let max_x = self.width as f32;\n"
          "        let max_y = self.height as f32;")],
        ["compositor"],
        ["the_pointer_cannot_be_pushed_off_the_desktop"],
    ),
    (
        "C: the sub-pixel remainder is thrown away every packet",
        EVDEV,
        [("        let (dx, dy) = accelerate(dx as f32, dy as f32, config);\n"
          "        self.x += dx;\n"
          "        self.y += dy;",
          "        let (dx, dy) = accelerate(dx as f32, dy as f32, config);\n"
          "        self.x += dx.trunc();\n"
          "        self.y += dy.trunc();")],
        ["compositor"],
        ["a_slow_movement_at_the_lowest_speed_setting_still_moves_the_pointer"],
    ),
    (
        "D: resizing the desktop leaves the pointer where it was",
        EVDEV,
        [("    pub fn set_bounds(&mut self, width: u32, height: u32) {\n"
          "        self.width = width;\n"
          "        self.height = height;\n"
          "        self.clamp();\n"
          "    }",
          "    pub fn set_bounds(&mut self, width: u32, height: u32) {\n"
          "        self.width = width;\n"
          "        self.height = height;\n"
          "    }")],
        ["compositor"],
        # Not `setting_bounds_through_the_trait_reaches_the_pointer`: that one
        # proves the trait method is wired to the inherent one, which this
        # defect leaves intact. It observes the pointer by moving it, and
        # `nudge` clamps on its own, so it stays green with the clamp deleted —
        # correctly, since it is not the claim it makes.
        ["a_click_after_the_desktop_shrinks_lands_inside_it_without_moving_first",
         "shrinking_the_desktop_brings_the_pointer_back_inside_it"],
    ),
    (
        "E: pointer speed is linear rather than geometric",
        EVDEV,
        [("    let steps = speed.clamp(-10, 10) as f32 / 5.0;\n"
          "    let factor = 2.0f32.powf(steps);",
          "    let steps = speed.clamp(-10, 10) as f32 / 5.0;\n"
          "    let factor = 1.0 + steps;")],
        ["compositor"],
        ["speed_is_geometric_so_the_slider_feels_even_end_to_end",
         "the_highest_speed_setting_multiplies_movement_by_four"],
    ),
    (
        "F: acceleration applies below the threshold as well as above it",
        EVDEV,
        [("    let over = (magnitude / threshold).max(1.0) - 1.0;",
          "    let over = magnitude / threshold;")],
        ["compositor"],
        ["careful_movement_below_the_threshold_is_never_accelerated"],
    ),
    (
        "G: an absurd acceleration gain is obeyed rather than clamped",
        EVDEV,
        [("    let factor = gain.mul_add(over, 1.0).clamp(0.0, MAX_ACCELERATION) * base;",
          "    let factor = gain.mul_add(over, 1.0).max(0.0) * base;")],
        ["compositor"],
        ["an_absurd_acceleration_gain_is_clamped_rather_than_obeyed"],
    ),
    (
        "H: a zero acceleration threshold divides by zero",
        EVDEV,
        [("    let threshold = if threshold > 0.0 { threshold } else { 1.0 };",
          "    let threshold = threshold;")],
        ["compositor"],
        ["a_zero_acceleration_threshold_does_not_divide_by_zero"],
    ),

    # -----------------------------------------------------------------------
    # Packet assembly and flush order
    # -----------------------------------------------------------------------
    (
        "I: a click is delivered before the movement in its own packet",
        EVDEV,
        [("        if self.packet.dx != 0 || self.packet.dy != 0 {\n"
          "            pointer.nudge(self.packet.dx, self.packet.dy, &settings.mouse);\n"
          "            let (x, y) = pointer.position();\n"
          "            out.push(InputEvent::MouseMove { x, y });\n"
          "        }\n"
          "        let (x, y) = pointer.position();",
          "        let (x, y) = pointer.position();\n"
          "        if self.packet.dx != 0 || self.packet.dy != 0 {\n"
          "            pointer.nudge(self.packet.dx, self.packet.dy, &settings.mouse);\n"
          "            out.push(InputEvent::MouseMove {\n"
          "                x: pointer.position().0,\n"
          "                y: pointer.position().1,\n"
          "            });\n"
          "        }")],
        ["compositor"],
        ["a_click_is_delivered_at_the_position_the_movement_in_its_packet_reached"],
    ),
    (
        "J: every packet emits a move, whether or not the mouse moved",
        EVDEV,
        [("        if self.packet.dx != 0 || self.packet.dy != 0 {\n"
          "            pointer.nudge(self.packet.dx, self.packet.dy, &settings.mouse);",
          "        {\n"
          "            pointer.nudge(self.packet.dx, self.packet.dy, &settings.mouse);")],
        ["compositor"],
        ["scrolling_does_not_move_the_pointer"],
    ),
    (
        "K: vertical motion counts upwards, as maths does and screens do not",
        EVDEV,
        [("                REL_Y => self.packet.dy = self.packet.dy.saturating_add(record.value),",
          "                REL_Y => self.packet.dy = self.packet.dy.saturating_sub(record.value),")],
        ["compositor"],
        ["vertical_motion_is_positive_downwards_as_the_screen_counts"],
    ),
    (
        "L: the two scroll axes are crossed",
        EVDEV,
        [("            out.push(InputEvent::MouseScroll {\n"
          "                dx: self.packet.hwheel as f32 * speed * sign,\n"
          "                dy: self.packet.wheel as f32 * speed * sign,",
          "            out.push(InputEvent::MouseScroll {\n"
          "                dx: self.packet.wheel as f32 * speed * sign,\n"
          "                dy: self.packet.hwheel as f32 * speed * sign,")],
        ["compositor"],
        ["horizontal_and_vertical_scroll_in_one_packet_are_one_event"],
    ),
    (
        "M: natural scroll is read and then ignored",
        EVDEV,
        [("            let sign = if settings.mouse.natural_scroll {\n"
          "                -1.0\n"
          "            } else {\n"
          "                1.0\n"
          "            };",
          "            let sign = if settings.mouse.natural_scroll {\n"
          "                1.0\n"
          "            } else {\n"
          "                1.0\n"
          "            };")],
        ["compositor"],
        ["natural_scroll_reverses_the_direction"],
    ),
    (
        "N: the user's scroll speed never scales a notch",
        EVDEV,
        [("            let speed = if settings.mouse.scroll_speed.is_finite() {\n"
          "                settings.mouse.scroll_speed\n"
          "            } else {\n"
          "                1.0\n"
          "            };",
          "            let speed = 1.0;")],
        ["compositor"],
        ["the_users_scroll_speed_scales_the_notch"],
    ),
    (
        "O: a nonsense scroll speed is passed straight through",
        EVDEV,
        [("            let speed = if settings.mouse.scroll_speed.is_finite() {\n"
          "                settings.mouse.scroll_speed\n"
          "            } else {\n"
          "                1.0\n"
          "            };",
          "            let speed = settings.mouse.scroll_speed;")],
        ["compositor"],
        ["a_nonsense_scroll_speed_does_not_produce_a_nonsense_scroll"],
    ),

    # -----------------------------------------------------------------------
    # Buttons
    # -----------------------------------------------------------------------
    (
        "P: a left-handed mapping swaps the thumb buttons too",
        EVDEV,
        [("        BTN_SIDE => MouseButton::Back,\n"
          "        BTN_EXTRA => MouseButton::Forward,",
          "        BTN_SIDE if swap => MouseButton::Forward,\n"
          "        BTN_SIDE => MouseButton::Back,\n"
          "        BTN_EXTRA if swap => MouseButton::Back,\n"
          "        BTN_EXTRA => MouseButton::Forward,")],
        ["compositor"],
        ["a_left_handed_mapping_leaves_the_thumb_buttons_where_they_are"],
    ),
    (
        "Q: an unrecognised button is guessed to be the primary one",
        EVDEV,
        [("        BTN_EXTRA => MouseButton::Forward,\n"
          "        _ => return None,",
          "        BTN_EXTRA => MouseButton::Forward,\n"
          "        _ => MouseButton::Left,")],
        ["compositor"],
        ["a_button_the_compositor_has_no_name_for_is_dropped_rather_than_guessed"],
    ),

    # -----------------------------------------------------------------------
    # Naming the key
    # -----------------------------------------------------------------------
    (
        "R: MSC_SCAN is consulted before the keycode table, not after",
        EVDEV,
        [("    uapi::set1_for_keycode(keycode).or(scan)",
          "    scan.or_else(|| uapi::set1_for_keycode(keycode))")],
        ["compositor"],
        ["the_keycode_table_wins_over_a_raw_code_that_disagrees_with_it"],
    ),
    (
        "S: a keycode the table does not name is dropped, raw code and all",
        EVDEV,
        [("    uapi::set1_for_keycode(keycode).or(scan)",
          "    let _ = scan;\n"
          "    uapi::set1_for_keycode(keycode)")],
        ["compositor"],
        ["a_keycode_with_no_scan_code_falls_back_to_the_raw_one_the_device_sent"],
    ),
    (
        "T: a raw code is left in place and claimed by the next key too",
        EVDEV,
        [("                let scan = self.packet.scan.take();",
          "                let scan = self.packet.scan;")],
        ["compositor"],
        ["a_raw_code_belongs_to_the_key_event_that_follows_it_and_not_the_next_one"],
    ),
    (
        "U: an extended key loses the prefix that separates it from the keypad",
        UAPI,
        [("    Some(0xE000 | extended)",
          "    Some(extended)")],
        ["compositor"],
        ["an_extended_key_keeps_the_prefix_that_distinguishes_it_from_the_keypad",
         "an_extended_key_keeps_the_prefix_that_separates_it_from_the_keypad"],
    ),

    # -----------------------------------------------------------------------
    # Key repeat
    # -----------------------------------------------------------------------
    (
        "V: modifiers and latches repeat like any other key",
        EVDEV,
        [("fn repeats(scancode: u32) -> bool {\n"
          "    !matches!(",
          "fn repeats(scancode: u32) -> bool {\n"
          "    let _ = scancode;\n"
          "    true || !matches!(")],
        ["compositor"],
        ["a_held_modifier_never_repeats",
         "no_modifier_or_latch_is_classified_as_repeating"],
    ),
    (
        "W: the key that repeats is the first one held, not the last",
        EVDEV,
        [("        if repeats(scancode) {\n"
          "            self.repeat = Some(Repeat {",
          "        if repeats(scancode) && self.repeat.is_none() {\n"
          "            self.repeat = Some(Repeat {")],
        ["compositor"],
        ["the_key_that_repeats_is_the_one_pressed_last"],
    ),
    (
        "X: releasing the repeating key does not stop it",
        EVDEV,
        [("        if let Some(repeat) = self.repeat\n"
          "            && !self.held.iter().any(|h| h.scancode == repeat.scancode)\n"
          "        {\n"
          "            self.repeat = None;\n"
          "        }",
          "        // (the repeat is left running)")],
        ["compositor"],
        ["releasing_a_held_key_stops_it_repeating"],
    ),
    (
        "Y: turning repeat off in the settings does not turn it off",
        EVDEV,
        [("        if !config.enabled {\n"
          "            return out;\n"
          "        }",
          "        // (the setting is not consulted)")],
        ["compositor"],
        ["turning_repeat_off_in_the_settings_turns_it_off"],
    ),
    (
        "Z: the repeat interval is a constant rather than the user's",
        EVDEV,
        [("        let interval = Duration::from_millis(u64::from(config.repeat_interval_ms.max(1)));",
          "        let interval = Duration::from_millis(30);")],
        ["compositor"],
        ["the_users_own_delay_and_interval_are_the_ones_used"],
    ),
    (
        "AA: the repeat delay is a constant rather than the user's",
        EVDEV,
        [("                        Duration::from_millis(u64::from(settings.keyboard.repeat_delay_ms)),",
          "                        Duration::from_millis(500),")],
        ["compositor"],
        ["the_users_own_delay_and_interval_are_the_ones_used"],
    ),
    (
        "AB: a stall pays out every repeat it missed while nothing ran",
        EVDEV,
        [("        while repeat.due <= now && emitted < MAX_REPEATS_PER_TICK {",
          "        while repeat.due <= now && emitted < 10_000 {")],
        ["compositor"],
        ["a_stall_does_not_pay_out_the_repeats_it_missed"],
    ),
    (
        "AC: new settings are accepted and then not used",
        EVDEV,
        [("    pub fn set_settings(&mut self, settings: InputSettings) {\n"
          "        self.settings = settings;\n"
          "    }",
          "    pub fn set_settings(&mut self, settings: InputSettings) {\n"
          "        let _ = settings;\n"
          "    }")],
        ["compositor"],
        ["changing_the_settings_while_running_takes_effect_without_a_restart"],
    ),

    # -----------------------------------------------------------------------
    # Re-synchronisation after a dropped packet
    # -----------------------------------------------------------------------
    (
        "AD: a drop reconciles one way only, so a key held unseen stays unseen",
        EVDEV,
        [("        for keycode in 0..(KEY_BITMAP_BYTES as u16).saturating_mul(8) {\n"
          "            if !uapi::bit_set(bitmap, keycode) {",
          "        for keycode in 0..0u16 {\n"
          "            if !uapi::bit_set(bitmap, keycode) {")],
        ["compositor"],
        ["a_drop_presses_a_key_the_device_is_holding_that_we_never_saw_go_down"],
    ),
    (
        "AE: a drop releases everything, whatever the device says is still down",
        EVDEV,
        [("            if held.device != self.index || uapi::bit_set(bitmap, held.keycode) {\n"
          "                return true;\n"
          "            }",
          "            if held.device != self.index {\n"
          "                return true;\n"
          "            }")],
        ["compositor"],
        ["a_drop_leaves_a_key_that_is_still_held_alone"],
    ),
    (
        "AF: a drop on one device releases the keys held on another",
        EVDEV,
        [("            if held.device != self.index || uapi::bit_set(bitmap, held.keycode) {",
          "            if uapi::bit_set(bitmap, held.keycode) {")],
        ["compositor"],
        ["a_drop_on_one_device_does_not_release_keys_held_on_another"],
    ),
    (
        "AG: a device that cannot say what it holds is taken to hold nothing new",
        EVDEV,
        [("            release_all_from(self.index, keys, out);\n"
          "            return;",
          "            return;")],
        ["compositor"],
        ["a_drop_whose_key_state_cannot_be_read_releases_what_that_device_held"],
    ),
    (
        "AH: a drop synthesises a button press from the key state",
        EVDEV,
        [("            if uapi::is_button(keycode) {\n"
          "                // Buttons are re-derived from the next packet's transitions;\n"
          "                // synthesising a press here would deliver a click nobody made.\n"
          "                continue;\n"
          "            }",
          "            // (buttons are pressed like any other code)")],
        ["compositor"],
        ["a_drop_does_not_synthesise_a_button_press_from_the_key_state"],
    ),
    (
        "AI: a drop keeps the half-built packet it invalidated",
        EVDEV,
        [("                    self.packet.clear();\n"
          "                    self.needs_resync = true;",
          "                    self.needs_resync = true;")],
        ["compositor"],
        ["a_drop_discards_the_half_built_packet_and_nothing_after_it"],
    ),
    (
        "AJ: the key state is re-read before the events in the same read",
        EVDEV,
        # A genuine *move*, not an extra call. The first version of this defect
        # inserted `resync` before `drain` and left the after-drain pass in
        # place, which changes nothing: on entry `needs_resync` is still false,
        # so the early pass is inert and the real one still runs afterwards.
        # It reported "no test failed" and the fault was in the defect.
        [("        for stream in &mut self.streams {\n"
          "            stream.drain(\n"
          "                now,\n"
          "                &self.settings,\n"
          "                &mut self.keys,\n"
          "                &mut self.pointer,\n"
          "                &mut out,\n"
          "            );\n"
          "        }",
          "        for stream in &mut self.streams {\n"
          "            stream.resync(&mut self.keys, &mut out);\n"
          "            stream.drain(\n"
          "                now,\n"
          "                &self.settings,\n"
          "                &mut self.keys,\n"
          "                &mut self.pointer,\n"
          "                &mut out,\n"
          "            );\n"
          "        }"),
         ("        for stream in &mut self.streams {\n"
          "            stream.resync(&mut self.keys, &mut out);\n"
          "        }\n",
          "")],
        ["compositor"],
        ["a_correction_lands_in_the_same_poll_as_the_events_it_corrects"],
    ),
    (
        "AK: a key released by a drop goes on repeating",
        EVDEV,
        [("            out.push(InputEvent::KeyUp {\n"
          "                scancode: held.scancode,\n"
          "            });\n"
          "            if keys.repeat.is_some_and(|r| r.scancode == held.scancode) {\n"
          "                keys.repeat = None;\n"
          "            }\n"
          "        }\n"
          "\n"
          "        // Down in the device's books and not in ours",
          "            out.push(InputEvent::KeyUp {\n"
          "                scancode: held.scancode,\n"
          "            });\n"
          "        }\n"
          "\n"
          "        // Down in the device's books and not in ours")],
        ["compositor"],
        ["a_drop_stops_the_repeat_of_a_key_it_released"],
    ),

    # -----------------------------------------------------------------------
    # Reading
    # -----------------------------------------------------------------------
    (
        "AL: an idle device is treated as a broken one",
        EVDEV,
        [("                Err(EAGAIN) | Err(EINTR) => {",
          "                Err(EINTR) => {")],
        ["compositor"],
        ["an_idle_device_produces_nothing_and_is_not_a_fault"],
    ),
    (
        "AM: a permanently broken device is retried every frame for ever",
        EVDEV,
        [("                    self.buf.truncate(start);\n"
          "                    self.dead = true;\n"
          "                    return;",
          "                    self.buf.truncate(start);\n"
          "                    return;")],
        ["compositor"],
        ["a_device_that_fails_permanently_goes_quiet_rather_than_erroring_every_frame"],
    ),
    (
        "AN: a read cut mid-record loses the tail rather than reassembling it",
        EVDEV,
        [("        self.buf.drain(..offset.min(self.buf.len()));",
          "        self.buf.clear();")],
        ["compositor"],
        ["a_read_cut_in_the_middle_of_a_record_is_reassembled"],
    ),
    (
        "AO: a short read is followed by another read for nothing",
        EVDEV,
        [("            if read < READ_CHUNK {\n"
          "                // A short read means the device had nothing more to give.\n"
          "                break;\n"
          "            }",
          "            // (the device is asked again regardless)")],
        ["compositor"],
        ["a_short_read_ends_the_tick_rather_than_asking_again_for_nothing"],
    ),
    (
        "AP: one tick will read a device until it runs dry, whatever it costs",
        EVDEV,
        [("const MAX_READS_PER_TICK: usize = 8;",
          "const MAX_READS_PER_TICK: usize = 64;")],
        ["compositor"],
        # Not a test: `a_burst_larger_than_one_tick_can_take_is_left_for_the_next_one`
        # feeds `MAX_READS_PER_TICK + 2` chunks and expects `MAX_READS_PER_TICK * 8`
        # events, so it scales with the constant and stays green at any value.
        # The ceiling is a `const _: () = assert!(..)` beside the constant.
        [COMPILE],
    ),
    (
        "AQ: only the first device that opened is actually read",
        EVDEV,
        [("        for stream in &mut self.streams {\n"
          "            stream.drain(",
          "        for stream in self.streams.iter_mut().take(1) {\n"
          "            stream.drain(")],
        ["compositor"],
        ["one_device_failing_does_not_silence_another"],
    ),

    # -----------------------------------------------------------------------
    # Opening the devices
    # -----------------------------------------------------------------------
    (
        "AR: a permission failure is reported as an absent device",
        EVDEV,
        [("                Err(EACCES) => denied = true,",
          "                Err(EACCES) => {}")],
        ["compositor"],
        ["a_permission_failure_is_reported_as_the_capability_it_needs"],
    ),
    (
        "AS: any failure at all is reported as a permission problem",
        EVDEV,
        [("                Err(EACCES) => denied = true,\n"
          "                Err(_) => {}",
          "                Err(_) => denied = true,")],
        ["compositor"],
        ["a_machine_with_no_input_devices_is_not_reported_as_a_permission_problem"],
    ),
    (
        "AT: the search stops at the first index that will not open",
        EVDEV,
        [("                Err(EACCES) => denied = true,\n"
          "                Err(_) => {}\n"
          "            }",
          "                Err(EACCES) => {\n"
          "                    denied = true;\n"
          "                    break;\n"
          "                }\n"
          "                Err(_) => break,\n"
          "            }")],
        ["compositor"],
        ["a_device_that_opens_alongside_a_refused_one_is_still_used"],
    ),
    (
        "AU: only the keyboard and the mouse are looked for",
        EVDEV,
        [("        for index in 0..MAX_DEVICES {",
          "        for index in 0..2 {")],
        ["compositor"],
        ["every_device_that_opens_is_read_not_just_the_first_two"],
    ),
    (
        "AV: a device that will not name itself is skipped",
        EVDEV,
        [("                Ok(mut sys) => {\n"
          "                    let name = device_name(&mut sys);",
          "                Ok(mut sys) => {\n"
          "                    let name = device_name(&mut sys);\n"
          "                    if name == \"unnamed device\" {\n"
          "                        continue;\n"
          "                    }")],
        ["compositor"],
        ["a_device_that_will_not_name_itself_is_still_used"],
    ),

    # -----------------------------------------------------------------------
    # Pairing a screen with an input source
    # -----------------------------------------------------------------------
    (
        "AW: the source is not told the desktop size until the first frame",
        PRESENT,
        [("    pub fn new(screen: S, mut input: I, width: u32, height: u32) -> Self {\n"
          "        input.set_bounds(width, height);",
          "    pub fn new(screen: S, input: I, width: u32, height: u32) -> Self {")],
        ["compositor"],
        ["a_source_learns_the_desktop_size_before_the_first_frame_is_shown"],
    ),
    (
        "AX: every frame tells the source its size again, changed or not",
        PRESENT,
        [("        if self.bounds != (width, height) {\n"
          "            self.bounds = (width, height);\n"
          "            self.input.set_bounds(width, height);\n"
          "        }",
          "        self.bounds = (width, height);\n"
          "        self.input.set_bounds(width, height);")],
        ["compositor"],
        ["a_resized_desktop_is_passed_on_but_an_unchanged_one_is_not"],
    ),
    (
        "AY: a resize is never passed on at all",
        PRESENT,
        [("        if self.bounds != (width, height) {\n"
          "            self.bounds = (width, height);\n"
          "            self.input.set_bounds(width, height);\n"
          "        }\n"
          "        self.screen.show(pixels, width, height);",
          "        self.screen.show(pixels, width, height);")],
        ["compositor"],
        ["a_resized_desktop_is_passed_on_but_an_unchanged_one_is_not"],
    ),
    (
        "AZ: the pair asks the screen for input, as it used to",
        PRESENT,
        [("    fn input(&mut self) -> Vec<InputEvent> {\n"
          "        self.input.poll()\n"
          "    }",
          "    fn input(&mut self) -> Vec<InputEvent> {\n"
          "        self.screen.input()\n"
          "    }")],
        ["compositor"],
        ["a_pair_sends_frames_to_the_screen_and_takes_events_from_the_source"],
    ),
    (
        "BA: the pair hides the screen's monitors",
        PRESENT,
        [("    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {\n"
          "        self.screen.monitors()\n"
          "    }",
          "    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {\n"
          "        None\n"
          "    }")],
        ["compositor"],
        ["the_monitors_are_the_screens_and_the_pairing_does_not_invent_any"],
    ),
    (
        "BB: the pair never notices the screen has gone",
        PRESENT,
        [("    fn is_open(&self) -> bool {\n"
          "        self.screen.is_open()\n"
          "    }\n"
          "\n"
          "    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {\n"
          "        self.screen.monitors()\n"
          "    }",
          "    fn is_open(&self) -> bool {\n"
          "        true\n"
          "    }\n"
          "\n"
          "    fn monitors(&mut self) -> Option<Vec<MonitorInfo>> {\n"
          "        self.screen.monitors()\n"
          "    }")],
        ["compositor"],
        ["the_screen_alone_decides_when_the_session_ends"],
    ),
]

# Defects excluded from a sweep, with the reason, because a defect deleted for
# being unapplyable is a defect somebody re-adds next year.
#
#   AH -- removing `Stream::resync`'s `is_button` skip changes nothing
#   observable, so no test can catch it and claiming one should was wrong.
#   Every `BTN_*` code is at or above `BTN_MISC` (0x100) and `set1_for_keycode`
#   stops at 217, so the scancode lookup two lines below the guard already
#   drops every button. The guard is reached and redundant, not unreached.
#   It is kept anyway -- it states the intent where the intent applies, and it
#   is what stops the security property ("a resync never synthesises a click")
#   from resting on the contents of an unrelated table. The leg that *can* be
#   checked is pinned by
#   `uapi::no_button_has_a_scan_code_so_a_resync_can_never_synthesise_a_click`.
NO_OP: set[str] = {"AH"}


def letter(name):
    """The defect's identifier -- everything before the first colon."""
    return name.split(":", 1)[0]


def run_tests(pkg):
    r = subprocess.run(
        ["cargo", "test", "-p", pkg, "--target", TARGET],
        cwd=ROOT, capture_output=True, text=True, errors="replace",
    )
    out = r.stdout + r.stderr
    # "error: test failed" is what a *failing test run* prints, so only
    # "could not compile" distinguishes a build break.
    if "could not compile" in out:
        return None, out
    failed = set()
    collecting = False
    for line in out.splitlines():
        s = line.strip()
        if s == "failures:":
            collecting = True
            continue
        if collecting:
            if "::" not in s:
                collecting = False
                continue
            failed.add(s.rsplit("::", 1)[-1])
    return failed, out


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    check_only = "--check" in sys.argv[1:]

    files = sorted({d[1] for d in DEFECTS})
    snap = {f: (ROOT / f).read_bytes() for f in files}
    digest = {f: hashlib.sha256(b).hexdigest() for f, b in snap.items()}
    print("snapshot:")
    for f in files:
        print(f"  {digest[f][:16]}  {f}")
    print()

    selected = [d for d in DEFECTS
                if letter(d[0]) not in NO_OP
                and (not args or letter(d[0]) in args)]

    if check_only:
        bad = 0
        for name, path, edits, _pkgs, _expect in selected:
            text = snap[path].decode("utf-8")
            problems = []
            for old, new in edits:
                n = text.count(old)
                if n == 0:
                    problems.append("PATTERN NOT FOUND")
                elif n > 1:
                    problems.append(f"AMBIGUOUS ({n} matches)")
                elif old == new:
                    problems.append("NO-OP")
                else:
                    text = text.replace(old, new, 1)
            verdict = "; ".join(problems) if problems else "ok"
            if problems:
                bad += 1
            print(f"{name}\n    {verdict}")
        print(f"\n{len(selected) - bad}/{len(selected)} patterns apply cleanly")
        sys.exit(1 if bad else 0)

    verdicts = []
    try:
        for name, path, edits, pkgs, expect in selected:
            text = snap[path].decode("utf-8")
            ok = True
            for old, new in edits:
                if old not in text:
                    ok = False
                    break
                text = text.replace(old, new, 1)
            if not ok:
                verdicts.append((name, "PATTERN NOT FOUND"))
                print(f"{name}\n    PATTERN NOT FOUND\n", flush=True)
                continue
            (ROOT / path).write_text(text, encoding="utf-8", newline="")

            all_failed, note, broke = set(), "", False
            for pkg in pkgs:
                failed, _out = run_tests(pkg)
                if failed is None:
                    broke, note = True, f"{pkg} did not compile"
                    break
                all_failed |= failed
            (ROOT / path).write_bytes(snap[path])

            # Some invariants cannot be defended by a test, because the test
            # would have to restate the constant it is checking and would then
            # scale with it. Those are asserted at compile time instead, and
            # for them a build break *is* the catch rather than a broken run.
            wants_build_break = expect == [COMPILE]
            if broke:
                verdict = ("caught by the build" if wants_build_break
                           else f"DID NOT COMPILE ({note})")
            elif wants_build_break:
                verdict = "*** THE BUILD ACCEPTED IT ***"
            elif not all_failed:
                verdict = "*** NO TEST FAILED ***"
            else:
                verdict = f"caught by {len(all_failed)}: {sorted(all_failed)}"
                missing = [t for t in expect if t not in all_failed]
                if missing and len(missing) == len(expect):
                    verdict += f"  [MISSING: {missing}]"
            verdicts.append((name, verdict))
            print(f"{name}\n    {verdict}\n", flush=True)
    finally:
        bad = []
        for f in files:
            (ROOT / f).write_bytes(snap[f])
            if hashlib.sha256((ROOT / f).read_bytes()).hexdigest() != digest[f]:
                bad.append(f)
        if bad:
            print(f"!!! NOT RESTORED: {bad}")
            sys.exit(2)
        print("restored: all files match their recorded SHA-256")

    print("\n=== summary ===")
    for name, verdict in verdicts:
        print(f"{name}\n    {verdict}")
    unproved = [n for n, v in verdicts
                if "NO TEST FAILED" in v or "NOT FOUND" in v
                or "DID NOT COMPILE" in v or "BUILD ACCEPTED" in v]
    print(f"\n{len(verdicts) - len(unproved)}/{len(verdicts)} defects caught")
    if unproved:
        print("unproved:")
        for n in unproved:
            print(f"  {n}")


if __name__ == "__main__":
    main()
