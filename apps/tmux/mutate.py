"""Mutation test for the terminal multiplexer: shells in its panes, the keys
and the pointer, and tmux's own meanings for its commands.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

Every pane held a banner saying the system had no PTY layer, typing went
nowhere, nothing answered the pointer, `:split-window -h` and the
`even-horizontal` layout did the opposite of tmux's, `prefix +` shrank half
the panes it was asked to grow, and `}` did not swap anything (known-issues,
TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED).

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src" / "main.rs"

# (name, old, new, [tests that must fail])
MUTATIONS = [
    # -- shells --
    (
        "a pane's shell is never started",
        "            pane.term.start_with(spawn);",
        "            let _ = spawn;",
        ["every_pane_gets_a_shell_of_its_own"],
    ),
    (
        "a key never reaches the shell",
        "            Some(pane) => {\n"
        "                pane.term.handle_event(&Event::Key(key.clone()));\n"
        "                EventResult::Consumed",
        "            Some(pane) => {\n"
        "                let _ = pane;\n"
        "                EventResult::Consumed",
        ["typing_reaches_the_active_panes_shell"],
    ),
    (
        "only the panes on screen are read",
        "        for pane in &mut self.panes {\n"
        "            match pane.term.on_event(&Event::Tick { elapsed_ms }) {",
        "        for pane in self.panes.iter_mut().filter(|p| visible.contains(&p.id)) {\n"
        "            match pane.term.on_event(&Event::Tick { elapsed_ms }) {",
        ["a_shell_in_a_window_not_showing_is_read_too"],
    ),
    (
        "a shell that exits leaves its pane",
        "            match pane.term.on_event(&Event::Tick { elapsed_ms }) {\n"
        "                Response::Exit => finished.push(pane.id),",
        "            match pane.term.on_event(&Event::Tick { elapsed_ms }) {\n"
        "                Response::Exit => {}",
        ["a_shell_that_exits_takes_its_pane_with_it", "the_last_shell_to_exit_closes_the_window"],
    ),
    (
        "a closed pane's shell is not hung up",
        "            let mut pane = self.panes.remove(at);\n            pane.term.hang_up();",
        "            let pane = self.panes.remove(at);\n            drop(pane);",
        ["closing_a_pane_asks_first_and_hangs_up_its_shell"],
    ),
    (
        "closing the window leaves the shells running",
        "            for pane in &mut self.panes {\n"
        "                pane.term.hang_up();\n"
        "            }\n"
        "            return Response::Exit;",
        "            return Response::Exit;",
        ["closing_the_window_hangs_up_every_shell"],
    ),
    (
        "the last session ending keeps the window open",
        "            self.quit = true;",
        "            self.quit = false;",
        [
            "the_last_shell_to_exit_closes_the_window",
            "killing_a_session_ends_its_shells_and_the_last_one_closes_the_window",
        ],
    ),
    (
        "the prefix twice sends nothing",
        "        if key.modifiers.ctrl && key.key == Key::B {\n"
        "            if let Some(pane) = self.active_pane_mut() {",
        "        if key.modifiers.ctrl && key.key == Key::B {\n"
        "            if let Some(pane) = None::<&mut Pane> {",
        ["the_prefix_twice_sends_the_program_a_ctrl_b"],
    ),
    (
        "a paste goes onto the screen",
        "            pane.term.paste(&text);",
        "            pane.term.feed(text.as_bytes());",
        ["a_paste_goes_to_the_program_not_onto_the_screen"],
    ),
    (
        "a pane is called Terminal",
        "        term.title.clear();",
        "",
        ["a_pane_is_named_by_what_runs_in_it"],
    ),
    # -- asking first --
    (
        "x closes without asking",
        "            'x' => self.ask_close_pane(),",
        "            'x' => {\n"
        "                if let Some(id) = self.active_pane_id() {\n"
        "                    self.remove_pane(id);\n"
        "                }\n"
        "            }",
        ["closing_a_pane_asks_first_and_hangs_up_its_shell"],
    ),
    (
        "any key is a yes",
        "                .is_some_and(|c| c.eq_ignore_ascii_case(&'y'))",
        "                .is_some()",
        ["closing_a_pane_asks_first_and_hangs_up_its_shell"],
    ),
    # -- limits --
    (
        "a refused window is refused after its pane is made",
        "        if session.windows.len() >= MAX_WINDOWS {",
        "        if false {",
        ["a_refused_window_leaves_no_pane_or_shell_behind"],
    ),
    (
        "a pane too small is split anyway",
        "        if room < MIN_PANE_SIZE * 2.0 + PANE_BORDER_WIDTH {",
        "        if false {",
        ["a_pane_too_small_to_halve_is_not_split"],
    ),
    # -- sizes and focus --
    (
        "a terminal is sized to its whole pane",
        "                pane.term.resize_to_window(content.w, content.h);",
        "                pane.term.resize_to_window(rect.w, rect.h);",
        ["each_terminal_is_as_big_as_the_space_it_is_drawn_in"],
    ),
    (
        "only the first window is sized",
        "            .flat_map(|w| w.bounds(width, height))",
        "            .take(1)\n            .flat_map(|w| w.bounds(width, height))",
        ["each_terminal_is_as_big_as_the_space_it_is_drawn_in"],
    ),
    (
        "every pane has the keyboard",
        "            let want = Some(pane.id) == keyboard;",
        "            let want = keyboard.is_some();",
        ["only_the_active_pane_has_the_keyboard"],
    ),
    (
        "the prompt leaves the pane the keyboard",
        "        let modal = self.show_help\n            || self.command_mode\n",
        "        let modal = self.show_help\n",
        ["only_the_active_pane_has_the_keyboard"],
    ),
    # -- tmux's meanings --
    (
        "split-window -h splits one above the other",
        '                if flags.clone().any(|f| f == "-h") {',
        '                if !flags.clone().any(|f| f == "-h") {',
        ["split_window_h_splits_side_by_side_as_tmux_does"],
    ),
    (
        "even-horizontal stacks the panes",
        "            Self::EvenHorizontal => Self::build_even(panes, SplitDir::SideBySide)?,",
        "            Self::EvenHorizontal => Self::build_even(panes, SplitDir::Stacked)?,",
        ["the_layouts_mean_what_tmux_means_by_them"],
    ),
    (
        "growing favours the first child",
        "            -delta\n",
        "            delta\n",
        ["growing_a_pane_grows_it_whichever_side_it_is_on"],
    ),
    (
        "a swap only moves the focus",
        "                window.layout.swap(here, other);",
        "                window.select_pane(other);",
        ["swapping_moves_the_pane_and_keeps_it_active"],
    ),
    (
        "the last pane is not remembered",
        "            self.last_pane = Some(self.active_pane);\n",
        "",
        ["semicolon_goes_back_to_the_pane_you_were_in"],
    ),
    (
        "a digit chooses by position",
        "            .and_then(|s| s.windows.iter().position(|w| w.index == number));",
        "            .and_then(|s| (number < s.windows.len()).then_some(number));",
        ["a_window_is_reached_by_the_number_on_its_tab"],
    ),
    (
        "a window's number is never reused",
        "            .find(|i| !self.windows.iter().any(|w| w.index == *i))",
        "            .find(|_| false)",
        ["a_window_is_reached_by_the_number_on_its_tab"],
    ),
    (
        "attach needs a name",
        "            (self.active_session < self.sessions.len()).then_some(self.active_session)",
        "            None",
        ["attach_and_kill_session_take_a_name_a_number_or_nothing"],
    ),
    (
        "two sessions may share a name",
        "        if self.sessions.iter().any(|s| s.name == name) {",
        "        if false {",
        ["two_sessions_cannot_share_a_name"],
    ),
    # -- copy mode --
    (
        "copy mode's keys go to the shell",
        "        if self.active_pane_mut().is_some_and(|p| p.copy_mode) {",
        "        if false {",
        ["copy_mode_selects_whole_lines_and_copies_them"],
    ),
    (
        "v marks nothing",
        "            'v' => pane.set_mark(),",
        "            'v' => {}",
        ["copy_mode_selects_whole_lines_and_copies_them"],
    ),
    (
        "the selection does not follow the view",
        "            sel.end_row = here;",
        "",
        ["copy_mode_selects_whole_lines_and_copies_them"],
    ),
    # -- the pointer --
    (
        "a click in a grid does not focus its pane",
        "            Target::Pane(id, part) => {\n                self.select_pane(id);",
        "            Target::Pane(id, part) => {",
        ["every_control_answers_the_pointer"],
    ),
    (
        "a drag is not followed",
        "                    self.drag = Some(id);",
        "",
        ["what_the_pointer_selected_can_be_copied"],
    ),
    (
        "a pane is sent window coordinates",
        "                x: event.x - content.x,\n                y: event.y - content.y,",
        "                x: event.x,\n                y: event.y,",
        ["what_the_pointer_selected_can_be_copied"],
    ),
    (
        "the wheel scrolls nothing",
        "            Some(Target::Pane(id, _)) => {\n                self.forward_mouse(id, event);",
        "            Some(Target::Pane(id, _)) => {\n                let _ = id;",
        ["the_wheel_scrolls_the_pane_under_the_pointer"],
    ),
    (
        "a click between a chooser's rows closes it",
        "            Target::ChooserBox => {}",
        "            Target::ChooserBox => self.session_chooser = false,",
        ["every_control_answers_the_pointer"],
    ),
    (
        "the scrim is one box under the whole window",
        "            f.hit(Target::Scrim, band);",
        "            let _ = band;\n            f.hit(Target::Scrim, Rect::new(0.0, 0.0, size.0, size.1));",
        ["every_control_answers_the_pointer"],
    ),
    (
        "every chooser row is drawn from the first",
        "        let first = selected.saturating_add(1).saturating_sub(fits);",
        "        let first = 0_usize;",
        ["a_long_list_of_sessions_stays_inside_its_box"],
    ),
    (
        "the tabs do not slide",
        "        let slide = (active_end - room).max(0.0);",
        "        let slide = 0.0_f32;",
        ["the_active_window_is_always_within_reach"],
    ),
    (
        "the window list runs under the clock",
        "            if x + entry_w > end {",
        "            if false {",
        ["the_active_window_is_always_within_reach"],
    ),
    # -- the clock and the detached screen --
    (
        "the clock counts the window's age",
        "        self.wall_ms.map_or_else(String::new, clock_text)",
        "        clock_text(self.uptime_ms)",
        ["the_status_bar_reads_the_wall_clock_in_utc"],
    ),
    (
        "the tick is a flat second",
        "        Some(panes.map_or(status, |p| p.min(status)))",
        "        Some(Duration::from_secs(1))",
        ["the_clock_is_asked_for_only_as_often_as_something_moves"],
    ),
    (
        "the detached screen names a tmux command",
        '                "Closing this window ends every session.".to_string(),',
        '                "Use :attach or tmux attach to reconnect".to_string(),',
        ["the_detached_screen_says_what_runs_on_and_how_to_return"],
    ),
    # -- woken by the shells (2026-09-25) --
    (
        "a new pane's shell cannot wake the window",
        "            // Before its shell starts, so the link is woken for its first word.\n            App::attach_waker(&mut pane.term, waker.clone());",
        "",
        ["every_pane_is_woken_by_its_shell_and_none_is_asked_on_a_clock"],
    ),
    (
        "the panes already open are not given the waker",
        "        for pane in &mut self.panes {\n            App::attach_waker(&mut pane.term, waker.clone());\n        }",
        "",
        ["every_pane_is_woken_by_its_shell_and_none_is_asked_on_a_clock"],
    ),
    (
        "a wake reads no shell",
        "            match App::on_wake(&mut pane.term) {",
        "            match Response::Idle {",
        ["a_wake_reads_every_shell_and_closes_a_pane_whose_shell_is_done"],
    ),
    (
        "a shell done on a wake leaves its pane",
        "            match App::on_wake(&mut pane.term) {\n                Response::Exit => finished.push(pane.id),",
        "            match App::on_wake(&mut pane.term) {\n                Response::Exit => {}",
        ["a_wake_reads_every_shell_and_closes_a_pane_whose_shell_is_done"],
    ),
]

if __name__ == "__main__":
    only = sys.argv[1:] or None
    raise SystemExit(sweep(SRC, MUTATIONS, "tmux-app", timeout=600, only=only))
