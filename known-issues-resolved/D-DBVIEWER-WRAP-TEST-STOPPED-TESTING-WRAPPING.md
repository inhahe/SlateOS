### [C] D-DBVIEWER-WRAP-TEST-STOPPED-TESTING-WRAPPING — ✅ FIXED 2026-08-15

**Status:** FIXED 2026-08-15
**Found by a full `cargo test --workspace`,** not by looking for it:
`dbviewer::tests::a_long_query_error_is_wrapped_not_cut_mid_word` failed with
"the error was drawn as 1 command(s)". It was the only failure in the
workspace, and it is **not** a wrapping bug — the wrapping is fine.

The test renders the results pane at a fixed 1200px, which leaves the message
1176px, and asserts a 184-character SQL error takes more than one line. That
held when it was written: the toolkit then measured text with the built-in
8×16 bitmap font, where 184 characters come to ~1470px and overflow. Later
`osfont::system::SystemFont` began resolving a real proportional host face,
the same string measured **988px**, and it fit on one line. So the test
started reporting a wrapping bug that did not exist, for the sole reason that
the font got narrower.

**The sharper form of the problem is the one that did not fail.** For as long
as the measured width sat between "wraps" and "fits", this test passed while
testing progressively less: a string that only just overflows exercises the
wrap loop once. There was no signal at all until it crossed the line
completely. A threshold test whose threshold is a constant and whose input is
measured by the environment degrades silently before it fails loudly.

**Fixed** by sizing the pane from the message rather than from a constant:
`results_pane_layout_at(app, width)` is split out of `results_pane_layout`,
and the test asks for half the message's own measured width, which forces an
overflow on any face however narrow it draws. `a_wordy_message_does_not_crowd_out_the_results`
did not need the same treatment — it uses `"word ".repeat(2000)`, which
overflows anything, which is why it kept working.

**Ruled out first, by A/B rather than by argument:** this is not fallout from
the Hangul shaper wiring landed the same day. Restoring `gui/font/src/` to the
pre-Hangul commit and re-running reproduced the failure identically. (It could
not have been: `norm::needs_work` is false for this string, so `pieces` takes
the fast path, and `hangul::preprocess` early-outs on `present`.)

**Where.** `apps/dbviewer/src/main.rs` — `results_pane_layout`,
`results_pane_layout_at`, `TEST_PANE_WIDTH`, and the test itself.

**Worth generalising.** Any other test asserting "this wraps" or "this is
elided" against a hard-coded width has the same failure mode latent in it. The
rule that avoids it: derive the box from a measurement of the content, never
from a literal.

**Follow-up sweep, same day.** Every other `text::wrap` / `text::elide`
assertion in `apps/**` and `gui/**` was checked for the same shape. One more
had it: `logviewer::tests::the_raw_line_is_cut_to_the_box_drawn_behind_it`
asserted `out.ends_with("...")` against a hard-coded `room = 400.0`, and the
lines it uses measure **450px** — a 12% margin, one font-metric change from
the failure dbviewer had already hit. Fixed the same way (`room` derived as
half the line's measured width).

The rest are sound, and the reason is worth naming: they assert a
*postcondition of the function* rather than a fact about the environment.
`regextester::elided_text_fits_the_box_it_is_drawn_in` checks `w <= box_w`,
which holds whether or not elision was needed; `logviewer::a_short_raw_line_is_left_alone`
and `regextester::text_that_fits_is_not_elided` assert the *non*-action at
widths (400px, 500px) nothing plausible could overflow; and
`dbviewer::a_wordy_message_does_not_crowd_out_the_results` uses
`"word ".repeat(2000)`, which overflows any face. Only an assertion that
something *must* overflow a literal is fragile, because only that direction
depends on the font staying at least as wide as it was the day the number was
written.
