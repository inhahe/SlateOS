## BUG-C-THE-KEYBOARD-LAYOUT-TEST-FAILS-ABOUT-ONE-WORKSPACE-RUN-IN-TWO -- FIXED 2026-09-13

**In short:** one test in the desktop shell failed now and then, and only when
the whole project's tests ran at once. It checks that the Super+Space shortcut
writes the new keyboard layout to the file the compositor reads. The cause was
its neighbour: a second test fires the same shortcut, the shortcut *writes a
file*, and that test was not given a scratch directory to write it in. So it
wrote into the developer's real configuration directory normally -- and into
this test's scratch directory whenever the two ran at the same moment.

**Why that made the assertion fail.** `persist_input_layout` returns early
when the file already holds the layout being switched to. Both tests start
from the same builtin list and both switch to the same second layout. If the
unscoped one wrote `de` first, the scoped one read `de` as its *before*, found
the file already correct, skipped its own save, and read `de` again as its
*after*. `assert_ne!(before, after)` then failed with nothing actually broken
in the feature.

**The fix:** `gui/desktop/src/lib.rs`,
`super_space_switches_to_the_next_keyboard_layout` now runs inside
`settingsfile::testing::with_scratch_config`, which both redirects the write
and takes the process-wide lock that serialises it against the other test.

**The general lesson, which is the reason this entry stays:** a test needs a
scratch config if it *writes* settings, not only if it reads them. This one
asserted purely on the shell's in-memory field and looked like a pure
in-memory test; the write was two calls down, inside the action handler. The
way to find the rest of them is to look at what the code under test *calls*,
not at what the test asserts.

**Evidence:** failed in the workspace run of 2026-09-13 (one failure in 579
binaries); passed in the identical run immediately after; passed 8/8 in
isolation, because in isolation the two tests do not overlap.
