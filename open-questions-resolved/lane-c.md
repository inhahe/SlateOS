## Resolved — lane C
- **Under the optional Filled look, should the grey boxes be paler?** (C-Q15)
  -- answered 2026-09-27: the boxes stay as they are; instead the accent and the
  other interface colours are kept separately for each look, so a choice made
  under one never lands on the other. `design-decisions.md` §1421.

- **Should the games follow the desktop theme?** (C-Q16) -- answered
  2026-09-27: every game's menus and panels do; each board is decided by
  whether its colours mean something to a player (Claude's calls, sent to lane
  E); and every game made as polished as possible. §1422.

- **Five finished features nobody can reach: wire them up or delete them?**
  (C-Q17) -- answered 2026-09-27: wire them up; where a feature also exists in
  reachable form, the one kept first takes everything both could do. §1423.

- **An event coloured like the accent vanishes on today's date: whose colour
  wins?** (C-Q19) -- answered 2026-09-27: neither colour is changed; a warning,
  both ways round, with a way straight to changing the event's colour. §1424.

- **Four lists of the installed programs: which is the real one?** (C-Q20) --
  answered 2026-09-27: one library in userspace; the kernel's list goes; and
  every program, category, file type and default any of the four held is
  carried over before anything is deleted. §1425.

- **What runs a daily backup?** (C-Q21) -- answered 2026-09-27: a background
  service started at boot, whether or not anyone signs in; a backup missed while
  the machine was off runs as soon as it is on again, without asking. §1426.

- **Automatic sign-in: how does someone reach a different account?** (C-Q22)
  -- answered 2026-09-27: no pause at start-up; a key held while starting shows
  the chooser, and the screen says so from the first moment; starting for
  repair skips automatic sign-in. §1427.

- **The feature list is often wrong: re-check it?** (C-Q23) -- answered
  2026-09-27: section by section as work is picked from it, each check dated.
  §1428.

- **Which keyboard shortcuts are on by default?** (C-Q24) — answered
  2026-09-27: Alt+F4, Alt+Tab, Super, Super+R, Print Screen and its variants
  (including two that save to a file), the dedicated volume and brightness
  keys, and inside programs Ctrl+C/X/V/Z, Ctrl+Shift+Z and Ctrl+F4; everything
  else available but off; Super+Tab folded into a setting for Alt+Tab. The
  operator also asked for a redo tree. `design-decisions.md` §1416.

- **How do passwords leave the password manager?** (C-Q25) — answered
  2026-09-27: both a plain-text export, made unmistakably clear it is insecure,
  and an encrypted backup; plus a program may ask for a password only with a
  capability for it and the user's consent in a prompt. §1417.

- **Where do a program's settings live?** (C-Q26) — answered 2026-09-27: one
  YAML file per program under the user's settings folder, and a settings
  service beside it that tells open windows about changes -- not the function
  that saves. §1418.

- **Should something build every crate before a merge?** (C-Q11) — answered
  2026-09-27 by delegation: the operator left it to Claude, asking that the
  check's cost be measured while the machine carries its normal load and set
  against the time it has saved. Measured and decided 2026-09-27
  (`design-decisions.md` §1430): the boot test already builds and lints every
  crate before a merge, at about 2.4% of its time, and has caught real breaks;
  nothing is added. The
  operator's two testing ideas that came with the answer went to lane A:
  `requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`.

- **The Open and Save windows should be the file explorer: which way?**
  (C-Q30) — answered 2026-09-27: the explorer shows the window for every
  program, and the program is handed only the file chosen (option A, which
  Claude recommended). Written up as `design-decisions.md` §1415, with the
  work by lane.

- **Nothing draws the mouse pointer; what happens over fullscreen?** (C-Q18)
  — answered 2026-09-27: the pointer is always shown, drawn on the
  presenter's copy today and by the display's hardware cursor plane once a
  screen is shown without copying; the light/dark request is met by the
  existing Default and Inverted outlined schemes. Written up by lane F as
  `design-decisions.md` §1334. Before that it was
  deferred 2026-09-25 to `deferred-questions.md` DQ3, at lane F's request
  (`requests/f-c-c-q18s-premise-changed-the-pointer-is-drawn-over-fullscreen-at-no-cost.md`).
  Lane F built the pointer as a layer laid over the picture as it is shown, the
  way a graphics chip's cursor plane is, and every presenter that exists copies
  a fullscreen picture anyway, so the pointer costs fullscreen nothing -- the
  trade the question asked about does not exist yet. The operator's answer
  settled the later case too, so it does not come back.

- **What does "selected" look like, and what happens to a toolbar?** (C-Q13,
  C-Q14) — both answered 2026-09-12. Selection takes the accent everywhere, at
  the *same* one-pixel thickness rather than a thicker line — the code already
  did that and only the explorer mock drew it heavier. Full-width strips keep
  their fill by default, with the hairline-separator treatment offered as a
  setting beside it. Written up as `design-decisions.md` §834 and §835; together
  they unblock the last 369 draw sites of the border conversion.


- **Should cards be shaded at all, and what colour?** (C-Q10) — answered
  2026-09-11. **Borders, with shaded cards kept as an optional theme.** The
  operator also specified the colours: black border and black headings,
  off-white background, and one blue-green doing three jobs — selected border,
  secondary text, and a switch that is on. Written up as `design-decisions.md`
  §829, with the heading/description split as §830.

  Two consequences the answer forced, both recorded in §829 because they change
  the palette beyond what was asked: the blue-green **replaces** the blue accent
  rather than joining it (every blue-green that clears 4.5 lands 1.19–1.74 from
  `#0036A3`, which is not a second colour), and `subtext0` takes the same value
  as `subtext1` (they were 1.10 apart — one colour — but `subtext0` has 1,087
  uses to `subtext1`'s 161, so it is the one to revisit if they should differ).

  **Still open, and deliberately not closed with it:** how to colour the card
  theme so every combination clears 4.5. The operator's own words — "I guess we
  still have to figure out how to color them". Scoped down from "the default
  look" to "an optional theme", which lowers the urgency without removing it.
  Tracked as `TD-C-THIRTEEN-LIGHT-ACCENTS-STILL-FAIL-ON-CARDS` and revisited
  when the border conversion is done.

- C-Q1 Should normalization consult font coverage? — resolved 2026-08-15
  (§428): **no** — normalization stays font-blind, and the font-fitting stage
  decomposes what the face cannot draw. This was the last 339 sweep
  disagreements, all one question.

- C-Q3 Should all three lanes keep publishing finished work through the one
  shared `os` worktree, after two collided in it? — answered 2026-08-21 by the
  operator, **b**; written up 2026-08-24 (§538): no. A lane publishes with
  `git push origin lane-<x>:main`, a fast-forward that needs no working
  directory and is *refused* rather than tangled if another lane got there
  first. `os` becomes a read-only window on the result.

- C-Q5 Should this OS keep writing its own cryptography by hand? — answered
  2026-08-21 by the operator, **c**; written up 2026-08-24 (§539): the
  primitives (hash, cipher, password hash) are ported from vetted
  implementations; the vault format and the service plumbing on top stay ours.
  The line falls where testing stops reaching — a cipher can compute the right
  answer and still leak the secret through its timing, and no test we write
  sees that, whereas a file format that loses a record is an ordinary bug. The
  eleven hand-written SHA-256 copies collapse to one ported one.

- C-Q4 Nothing can print, and two disconnected halves of a printing system
  exist — which should applications talk to? — answered 2026-08-21 by the
  operator, **c**; written up 2026-08-24 (§540): neither. Printing becomes a
  background service applications submit jobs to, so a job outlives the
  application that started it. Lane C had recommended the cheaper shared
  library (b); the operator overruled it as a stop-gap that would only be
  rewritten, since a library and a service differ in *who owns the job*, and
  every caller written against the library is a caller to migrate.

- C-Q2 On a line mixing Hebrew or Arabic with English, should the Right arrow
  key move the caret one character later in the sentence, or one step right on
  the screen? — answered 2026-08-21 by the operator, **b (visual)**; written up
  2026-08-24 (§541): the screen. A key named for a screen direction follows the
  screen; Home/End and word-motion stay logical, because those name positions
  in the sentence. Caveat carried into the implementation: a widget that does
  not also remember which side of a direction boundary the caret is on will
  **skip a whole right-to-left word** in one press — worse than the old
  behaviour, so a half-switched widget is a regression, not a partial win.

- C-Q6 We have written the Settings screens twice — which copy is the real
  one? — answered 2026-09-07 by the operator, **C**; written up §815: split by
  kind. What the desktop *shows* you (volume overlay, login screen) stays in
  the shell and gets wired up; screens you *open* move to the Settings app and
  the shell's copies go. The operator added a styling mandate that was not part
  of the question: both follow `Aero Desktop (offline).html`, themeable parts
  read from current settings, and the demo's look is the default theme —
  recorded in `roadmap-detailed.md` as instructed.

- C-Q7 The high-contrast scheme's highlight is three times dimmer than the
  others — change it? — answered 2026-09-07; written up §816: **white**, and
  the highlight colour becomes user-configurable in every scheme. The
  configurability is the operator's requirement and binding; the white-over-cyan
  default was delegated to lane C. The operator's colour-vision reasoning was
  correct, but the stronger point was their own first sentence — a highlight
  need not carry meaning in hue at all, and luminance contrast is read
  identically by every form of colour vision.

- C-Q8 The world's timezone data cannot be written because the lane map hands
  the job to a directory that does not exist — who does it? — answered
  2026-09-07 by the operator, **B**; written up §817: lane B, which already
  owns the package manager, with the map corrected in the same change. The
  map's error was the cause of the stall, not a missing decision.

- An account with no password: should the lock screen let it through? —
  answered 2026-09-07 by the operator, **C**; written up §818: such an account
  is never locked at all, so nothing appears that pretends to be protecting
  anything. Setting a password is what turns locking on.

- Which cipher, and who owns it? — answered 2026-09-07; written up §819:
  **ChaCha20-Poly1305**. The operator's rule was "fastest with AES-NI unless
  the bottleneck is the disk anyway"; for a kilobyte vault dominated by key
  derivation, neither cipher is measurable, so the exception applies. The one
  condition that would have flipped it — this becoming the full-disk cipher —
  does not hold: disk encryption already exists in `kernel/src/fs/diskencrypt.rs`
  with AES-256-XTS, a mode not interchangeable with an authenticated-message
  cipher. The entry's unglossed jargon, which the operator called out, is
  glossed in §819.

- C-Q27 — `CLAUDE.md`'s lane row gave lane C a `pkg/**` that has never existed
  (the package manager is `userspace/pkg/`, lane B's). **Moot 2026-09-22**: the
  operator had `CLAUDE.md`'s lane section rewritten for six lanes, and the new
  table — generated from `scripts/which-lane.py`'s, which was already right —
  names no `pkg/`. No decision was needed, so there is no design-decisions
  entry beyond the mention in §1100.

