"""Which apps answer keys they never name on screen?

Reports *candidates*, not verdicts. Three things it cannot know:

  * a key name in a string literal is not proof the string is drawn
    (`apps/netscan`'s `wol_note` was written and drawn by nothing);
  * a `Key::` match in live code is not proof the arm is a user shortcut
    -- it may be a helper, a test fixture builder, or a widget's own
    internal handling;
  * an app may name its keys in prose the user reads elsewhere.

So this ranks apps by how many of the keys they match are never spelled
anywhere in their own strings, and the top of that list gets read by hand.

Every `.rs` file in the crate is read, not just `main.rs`: 12 of 141 apps
have more than one source file, and a sweep over `src/main.rs` alone is
the eighth way a search says nothing.
"""

import io
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import rustlex  # noqa: E402
import selftestflag  # noqa: E402

# How a `Key::` variant is spelled where a human would read it. Several
# spellings each, because apps differ and any one of them counts as named.
NAMES: dict[str, tuple[str, ...]] = {
    "Left": ("Left", "Arrow", "arrows"),
    "Right": ("Right", "Arrow", "arrows"),
    "Up": ("Up", "Arrow", "arrows"),
    "Down": ("Down", "Arrow", "arrows"),
    "Home": ("Home",),
    "End": ("End",),
    "PageUp": ("PageUp", "Page Up", "PgUp"),
    "PageDown": ("PageDown", "Page Down", "PgDn", "PgDown"),
    "Backspace": ("Backspace", "Bksp"),
    "Delete": ("Delete", "Del"),
    "Insert": ("Insert", "Ins"),
    "Enter": ("Enter", "Return"),
    "Tab": ("Tab",),
    "Escape": ("Esc",),
    "Space": ("Space",),
    "Comma": (",", "Comma"),
    "Period": (".", "Period"),
    "Semicolon": (";", "Semicolon"),
    "Colon": (":", "Colon"),
    "Slash": ("/", "?", "Slash"),
    "Backslash": ("\\", "Backslash"),
    "LeftBracket": ("[",),
    "RightBracket": ("]",),
    "Minus": ("-", "Minus"),
    "Equals": ("=", "+", "Equals"),
    "Apostrophe": ("'",),
    "Grave": ("`",),
}
for _letter in "ABCDEFGHIJKLMNOPQRSTUVWXYZ":
    NAMES[_letter] = (_letter,)
for _digit in range(10):
    NAMES[f"Num{_digit}"] = (str(_digit),)
for _fn in range(1, 13):
    NAMES[f"F{_fn}"] = (f"F{_fn}",)

# Keys whose meaning a user does not have to be told.
#
# This is the filter that makes the survey mean anything. Without it the
# report is led by `apps/terminal` with fifty-six "unnamed" keys -- a
# terminal *forwards* keys to a shell, and `Backspace` in a text field is
# not a discoverability problem in any program ever written. What a user
# cannot guess is that `B` hides the sidebar. Arrows, Home/End, paging,
# Enter, Escape, Tab, Backspace and Delete carry their meaning on the cap;
# letters, digits and function keys carry none.
CONVENTIONAL = {
    "Left",
    "Right",
    "Up",
    "Down",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Backspace",
    "Delete",
    "Insert",
    "Enter",
    "Tab",
    "Escape",
    "Space",
    "VolumeUp",
    "VolumeDown",
    "VolumeMute",
    "MediaPlayPause",
    "MediaNextTrack",
    "MediaPrevTrack",
    "MediaStop",
}

# Variants that are never a shortcut a user presses deliberately.
IGNORE = {
    "Unknown",
    "LeftShift",
    "RightShift",
    "LeftCtrl",
    "RightCtrl",
    "LeftAlt",
    "RightAlt",
    "LeftSuper",
    "RightSuper",
    "CapsLock",
    "NumLock",
    "ScrollLock",
    "PrintScreen",
    "Pause",
}

KEY_RE = re.compile(r"\bKey::([A-Za-z][A-Za-z0-9]*)")

# A key enum that carries the key in the variant's *payload*.
#
# `apps/markdowneditor` defines its own `Key` -- `Char(char)`, `Function(u8)`
# -- and bridges the toolkit's into it. `KEY_RE` reads the variant name, so
# the survey saw two keys called `Char` and `Function` and none of the twelve
# letters the app answers. It reported a text editor with 56 `Key::Char` sites
# as having two unnamed keys: the instrument was blind, and blindness here
# reads as *clean*, which is the worst direction for a survey to fail in.
#
# Only a **pattern** counts, and that distinction is load-bearing rather than
# fussy. The bridge is `GKey::F1 => Key::Function(1)` for all twelve function
# keys, so reading payloads without it would have swapped a blind spot for
# twelve keys this app does not bind -- `markdowneditor` binds no function key
# at all. A pattern stands to the left of its arm's `=>`; a construction
# stands to the right of one, which is a fact about the text on the line.
#
# The limitation, stated here rather than discovered later: a pattern split
# across lines (`Key::Char('a')` on one line, `| Key::Char('b') => {` on the
# next) loses the first alternative, because the `=>` is not on its line.
# Nothing in this tree is written that way; if something ever is, this
# undercounts silently, so the self-test pins the shapes that do occur.
PAYLOAD_RE = re.compile(r"(?<![A-Za-z0-9_])Key::(Char|Function)\s*\(([^)]*)\)(.*)")
PAYLOAD_CHAR = re.compile(r"'([A-Za-z0-9])'")
PAYLOAD_NUM = re.compile(r"(?<![A-Za-z0-9_])([0-9]{1,2})(?![0-9])")

# Keys an app dispatches from a *typed character* rather than from a `Key`.
#
# `apps/whiteboard` answers `g` for the grid and `0` for zoom-to-fit inside
# `handle_typed(&mut self, event: &KeyEvent)`, matching on the char that
# `event.typed()` produced. `apps/tmux` answers eighteen that way, through
# `process_prefix_key(&mut self, key: char)`. There is no `Key::` anywhere in
# either arm, so this survey could not see one of them -- the fifth flaw it
# has had, and the second in the direction that reads as clean.
#
# THE HARD PART IS TELLING A SHORTCUT FROM A PARSER. A match on `'B'` is just
# as likely to be a unit suffix (`apps/backup`), a terminal escape
# (`apps/terminal`) or a size abbreviation (`apps/pdfviewer`) as a key. A
# plain scan for char-literal arms finds 136 across 20 crates and most are
# noise.
#
# Two rules were tried and one thrown away. Matching function *names*
# -- `handle_key`, `handle_typed`, `on_key` -- accepted whiteboard and paint
# and missed tmux entirely, whose dispatcher is called `process_prefix_key`.
# That is the same defect as searching for the identifier `SHORTCUTS`: a name
# is a thing an author chooses. What survives is **reachability**: an arm
# counts when the function holding it takes a `KeyEvent`, or takes a `char`
# and is called by something that does. No list of names, and tmux is found
# through a caller two hops away.
CHAR_LIT = re.compile(r"'([A-Za-z0-9])'")
FN_DECL = re.compile(
    r"^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?"
    r"(?:async[ \t]+)?fn[ \t]+([A-Za-z_]\w*)([^\n]*)",
    re.MULTILINE,
)
CALLED = re.compile(r"(?<![A-Za-z0-9_])([a-z_]\w*)\s*\(")
TAKES_CHAR = re.compile(r":\s*char(?![A-Za-z0-9_])")

# Whether a char literal is a match arm is decided by walking a short window
# after it rather than by a regex.
#
# The obvious pattern -- the literal, optional spaces, a starred group of
# further alternatives each with its own optional spaces, then the arrow --
# puts a variable-width gap inside a starred group and another outside it.
# On the char literals that are *not* arms it backtracks through every way
# of splitting the run, and this tree has thousands of those: every
# `rest.find` of a quote character is one. It took this survey from two
# seconds to five minutes fifty, and the runtime is what gave it away --
# the report it produced looked entirely reasonable.
_ARM_GAP = frozenset(" \t|'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789")


def _is_arm(code: str, after: int) -> bool:
    """Does an arrow follow this char literal, with only alternatives between?"""
    limit = min(len(code), after + 80)
    i = after
    while i < limit:
        if code.startswith("=>", i):
            return True
        if code[i] not in _ARM_GAP:
            return False
        i += 1
    return False


def typed_keys(sources: list[str]) -> set[str]:
    """Keys reached from a key handler and matched as a character literal."""
    fns: list[tuple[str, str, str, int, int]] = []
    for code in sources:
        found = list(FN_DECL.finditer(code))
        for i, m in enumerate(found):
            stop = found[i + 1].start() if i + 1 < len(found) else len(code)
            fns.append((m.group(1), m.group(0), code, m.start(), stop))

    by_name: dict[str, list[int]] = {}
    for i, (name, _sig, _c, _a, _b) in enumerate(fns):
        by_name.setdefault(name, []).append(i)

    calls = [set(CALLED.findall(code[a:b])) for _n, _s, code, a, b in fns]
    charish = {
        i for i, (_n, sig, _c, _a, _b) in enumerate(fns) if TAKES_CHAR.search(sig)
    }

    # Breadth-first from the functions that take a keystroke, through the
    # call graph. The nested form -- for each candidate, scan every reached
    # function body -- is quadratic in the number of functions, and
    # `apps/paint` has hundreds of them.
    reached = {
        i for i, (_n, sig, _c, _a, _b) in enumerate(fns) if "KeyEvent" in sig
    }
    frontier = list(reached)
    while frontier:
        for callee in calls[frontier.pop()]:
            for j in by_name.get(callee, ()):
                if j in charish and j not in reached:
                    reached.add(j)
                    frontier.append(j)

    out: set[str] = set()
    for i in reached:
        _name, _sig, code, a, b = fns[i]
        for m in CHAR_LIT.finditer(code, a, b):
            if _is_arm(code, m.end()):
                ch = m.group(1)
                out.add(ch.upper() if ch.isalpha() else f"Num{ch}")
    return out

# The variant names themselves are never keys -- the key is what they carry.
PAYLOAD_VARIANTS = frozenset({"Char", "Function"})


def payload_keys(code: str) -> set[str]:
    """Keys named inside `Key::Char('x')` / `Key::Function(5)` patterns."""
    out: set[str] = set()
    for variant, payload, rest in PAYLOAD_RE.findall(code):
        if "=>" not in rest:
            continue
        if variant == "Char":
            for ch in PAYLOAD_CHAR.findall(payload):
                out.add(ch.upper() if ch.isalpha() else f"Num{ch}")
        else:
            for num in PAYLOAD_NUM.findall(payload):
                if 1 <= int(num) <= 12:
                    out.add(f"F{int(num)}")
    return out


# A printed key list, found by its *shape* rather than its name.
#
# This looked for the identifiers `SHORTCUTS` and `ALL_KEY_ACTIONS`, which is
# how the first run of this survey reported ten apps with a key list when the
# real number is far higher: `apps/magnifier` calls its `HELP_ROWS`, and a
# dozen more spell it a dozen other ways. A name is a thing an author chooses;
# a `const NAME: ... (&str, &str)` is a thing the type system fixes, and that
# is what a list of keys and their descriptions looks like whatever it is
# called.
#
# The first version of this tool was the very defect the tool exists to find:
# a search for the spelling somebody happened to use. Lane A hit the same
# shape the same day in `scripts/check-variant-lists.py`, whose population is
# every list *named* ALL -- so a list that should be total and is called
# PRIMARY_COMMANDS is invisible to it, and nothing says so.
LIST_RE = re.compile(
    r"(?:const|static)\s+[A-Z_][A-Z_0-9]*\s*:[^=;]*"
    r"\(\s*&(?:'static\s+)?str\s*,\s*&(?:'static\s+)?str\s*\)"
)

# ...and then the first column has to look like keys.
#
# The shape alone is not enough, and finding that out is the second correction
# this detector has needed. `(&str, &str)` is what a key list is made of and
# also what every other table of string pairs is made of: `apps/explorer` has
# `SIDEBAR_ITEMS` mapping a label to a path, `apps/gomoku` has `PANEL_LINES`
# mapping a label to a sample value for measuring. Both matched, and the count
# went from a wrong 10 (by name) to a wrong 29 (by type) before this.
#
# **There is no purely structural signal for "a list of keys".** The type says
# "pairs of strings"; only the contents say what kind. So this is openly a
# heuristic -- the first column of at least half the rows has to parse as a
# key label -- and the number it produces is reported as one.
KEYISH = re.compile(
    r"""^\s*
    (?: (?:Ctrl|Control|Alt|Option|Shift|Super|Cmd|Win|Meta) \+ )*
    (?:
        F[1-9][0-2]?                      # function keys
      | PageUp|PageDown|PgUp|PgDn         # named keys
      | Home|End|Left|Right|Up|Down
      | Enter|Return|Esc|Escape|Tab|Space|Spacebar
      | Delete|Del|Backspace|Bksp|Insert|Ins
      | Arrows
      | [A-Za-z0-9]\s*-\s*[A-Za-z0-9]     # a range, 0-9 or A-F
      | .                                 # a single key cap
    )
    \s*$""",
    re.VERBOSE,
)

# The first element of each row of a list literal: ("Ctrl+N", "New slide").
ROW_KEY_RE = re.compile(r'\(\s*"((?:[^"\\]|\\.)*)"\s*,')


def is_key_list(body: str) -> bool:
    """Does the first column of this list literal read as key labels?"""
    keys = ROW_KEY_RE.findall(body)
    if not keys:
        return False
    keyish = sum(
        1
        for k in keys
        if k and all(KEYISH.match(part) for part in re.split(r"\s*[/,]\s*", k) if part)
    )
    return keyish * 2 >= len(keys)



# A run of keys written the way a person writes one, plus the two words that
# stand for four keys each.
#
# WHY THIS EXISTS. Without it this survey reports keys as unnamed that the app
# names perfectly well, and it did -- five times in one afternoon, each with a
# different shape:
#
#   * `apps/klotski` draws "1-7: puzzle" and was reported as never naming
#     `Num2`..`Num6`, because the literal text "Num2" is not in the string.
#   * `apps/rush` the same, with "1-8".
#   * `apps/compass` draws "1-0: select waypoint" -- which means 1 through 9
#     and *then* 0, so an ascending expander produces an empty range and the
#     row looks like it names nothing. Ten keys reported unnamed; none were.
#   * `apps/sokoban` and `apps/snake` write "Arrows/WASD: move", which names
#     eight keys in two words.
#
# Each false alarm cost a read of an app that turned out to be correct, and one
# of them nearly cost an edit to an app that was already right.
RANGE_RE = re.compile(r"(?<![\w-])([A-Za-z0-9])\s*-\s*([A-Za-z0-9])(?![\w-])")


def spelled_out(haystack: str) -> set[str]:
    """Every `Key::` variant the text names through a range or a group word."""
    out: set[str] = set()
    if re.search(r"\barrows?\b", haystack, re.IGNORECASE):
        out |= {"Left", "Right", "Up", "Down"}
    if re.search(r"\bwasd\b", haystack, re.IGNORECASE):
        out |= {"W", "A", "S", "D"}
    for lo, hi in RANGE_RE.findall(haystack):
        if lo.isdigit() and hi.isdigit():
            a, b = int(lo), int(hi)
            # `1-0` is how a person writes "1 through 9 and then 0", and it is
            # the spelling `apps/compass` uses. Read ascending it is empty.
            digits = range(a, b + 1) if a <= b else list(range(a, 10)) + [0]
            out |= {f"Num{d}" for d in digits}
        elif lo.isalpha() and hi.isalpha():
            a, b = ord(lo.upper()), ord(hi.upper())
            if a <= b:
                out |= {chr(c) for c in range(a, b + 1)}
    return out


# A one-character name has to stand alone to count as naming a key.
#
# WHY. `NAMES["A"]` is `("A",)` and the test was `"A" in haystack`, a plain
# substring match -- so `apps/worldclock`'s button label "Add City" counted as
# naming the `A` key, and "Grid" named `G`, and "Pin" named `P`. That app draws
# no hint line and carries no key list: it names *none* of its eleven keys, and
# the survey reported two.
#
# Measured over the whole tree the day this was fixed: the substring rule
# reported 25 apps and 63 unnamed keys, and requiring a standalone token
# reported 53 apps and 220. The substring rule was hiding about 157 keys, which
# is two and a half times the gap it was reporting.
#
# Case-sensitive on purpose. Matching case-insensitively drops it to 173, and
# the difference is almost entirely the English article: a lowercase standalone
# `a` appears in ordinary prose everywhere, so `A` would read as named in any
# app with a sentence in it. Key hints in this tree capitalise the key, which
# is also how a person writes one.
_STANDALONE = "(?<![A-Za-z0-9]){}(?![A-Za-z0-9])"


def names_the_key(name: str, haystack: str) -> bool:
    """Does `haystack` name a key called `name`?"""
    if len(name) == 1 and name.isalnum():
        return re.search(_STANDALONE.format(re.escape(name)), haystack) is not None
    return name in haystack


# A whole string literal read as the keys column of a printed key list.
#
# Case-sensitivity for one-letter keys stays, and the reason recorded for it
# -- that "a" is an English article -- turned out to be the smaller half. A
# standalone lowercase letter is also every format placeholder (`{e}`,
# `{d} days ago`), every unit (`{}h {}m`), every aperture (`f/{ap:.1}`) and
# every doc fixture (`add(a: u32, b: u32)`). Accepting lowercase anywhere in
# the haystack moved 26 keys off this queue and every one I sampled was one of
# those.
#
# What is safe is much narrower: a *whole, short* literal that reads as a
# column of key caps. `apps/tmux` prints `("c", "New window")` and
# `("n / p", "Next or previous window")`, and its prefix answers 'c' and not
# 'C', so the card is right to be lowercase and the survey was wrong to be
# blind to it.
LABEL_SPLIT = re.compile(r"\s*(?:/|,| or )\s*")


def label_keys(literal: str) -> set[str]:
    """Keys named by a literal that is itself a key-list label."""
    text = literal.strip()
    if not text or len(text) > 12:
        return set()
    parts = [p.strip() for p in LABEL_SPLIT.split(text) if p.strip()]
    if not parts or len(parts) > 4:
        return set()
    out: set[str] = set()
    for part in parts:
        if len(part) != 1 or not part.isalnum():
            # One non-cap part and this is prose, not a key column.
            return set()
        out.add(part.upper() if part.isalpha() else f"Num{part}")
    return out


# A string with an escape character in it is not a label a reader sees.
#
# `apps/terminal` writes the sequences it sends to the program inside as
# string literals: "\x1b[Z" is back-tab and "\x1b[{};{}R" is a cursor
# position report. Those contain a standalone Z and R, so the survey read
# them as the terminal naming those keys -- and a terminal names no key,
# it forwards them. Two of the keys it forwards therefore read as *named*.
#
# That is the same failure direction as shape 23 and it was found the same
# way: not by the report looking wrong, but by an answer file line going
# stale, which asked why a key was no longer being reported.
def is_label(literal: str) -> bool:
    """Is this literal text somebody reads, rather than bytes sent somewhere?"""
    return not any(mark in literal for mark in (r"\x1b", r"\u{1b}", chr(27)))


def crate_sources(crate: Path) -> list[Path]:
    return sorted(p for p in (crate / "src").rglob("*.rs"))


def survey(crate: Path) -> tuple[int, int, list[str], bool] | None:
    files = crate_sources(crate)
    if not files:
        return None

    matched: set[str] = set()
    from_labels: set[str] = set()
    literals: list[str] = []
    has_list = False
    kept_sources: list[str] = []

    for path in files:
        try:
            src = io.open(path, encoding="utf-8").read()
        except (OSError, UnicodeDecodeError):
            continue
        live, _test = rustlex.live_code(src)
        for m in LIST_RE.finditer(live):
            # The literal that follows the declaration, up to its closing
            # bracket -- enough of it to read the first column.
            body = live[m.end() : m.end() + 4000]
            if is_key_list(body):
                has_list = True
                # The keys column of a list this app really prints. Read here
                # and nowhere else: a bare `"v"` anywhere in a crate is not a
                # key cap -- `apps/procexplorer` has `"v".repeat(72)` in a
                # wrapping fixture -- but the first column of a table whose
                # rows parse as key labels is exactly that.
                for column in ROW_KEY_RE.findall(body):
                    from_labels |= label_keys(column)
                break
        # Comments and literals blanked before the key scan: a key named in
        # a comment is the fourth way a search says nothing, and my own
        # comment about Command::new once poisoned a grep for it.
        code = rustlex.strip_noise(live, keep_literals=False)
        matched.update(KEY_RE.findall(code))
        # The payload scan needs the literals `code` has just blanked, so it
        # reads a second pass with comments stripped and literals kept.
        kept = rustlex.strip_noise(live, keep_literals=True)
        matched.update(payload_keys(kept))
        # Collected rather than scanned per file: a key handler in one file
        # can dispatch to a helper in another, and `typed_keys` walks calls.
        kept_sources.append(kept)
        literals.extend(l for l in rustlex.string_literals(live) if is_label(l))

    matched.update(typed_keys(kept_sources))
    matched -= PAYLOAD_VARIANTS
    matched -= IGNORE
    matched -= CONVENTIONAL
    if not matched:
        return None

    haystack = " | ".join(literals)
    spelled = spelled_out(haystack)
    unnamed = sorted(
        k
        for k in matched
        if k not in spelled
        and k not in from_labels
        and not any(names_the_key(n, haystack) for n in NAMES.get(k, (k,)))
    )
    return len(matched), len(unnamed), unnamed, has_list



def _self_test() -> int:
    r"""Cases for `spelled_out`, every one of them a false alarm this survey
    actually raised before it could read these shapes.

    The last two are the controls. `the barrows of old` must name nothing --
    without a word boundary the arrow case matches inside any word containing
    "arrow". And `press - to zoom out` must name nothing -- a lone hyphen is
    the minus *key*, not a range, which is the same distinction
    `guitk::shortcut::keystrokes` makes and for the same reason.

    The boundary control earns its place twice over. These patterns were
    written with `\b` through a heredoc, one level of escaping was lost, and
    `\b` -- a *valid* Python escape -- became a literal backspace character
    while `\w` in the neighbouring pattern stayed intact because it is not a
    valid one and merely warned. The corrupted patterns matched nothing at all,
    so every case that expects a match would have caught it -- but silently
    returning "this app names no keys" is precisely the failure this survey
    exists to avoid, and it would have read as a long work queue rather than a
    bug.
    """
    cases: list[tuple[str, set[str], str]] = [
        ("1-7: puzzle", {"Num1", "Num4", "Num7"}, "a digit range, as apps/klotski writes it"),
        ("1-8: level", {"Num1", "Num8"}, "as apps/rush writes it"),
        (
            "1-0: select waypoint",
            {"Num1", "Num9", "Num0"},
            "1 through 9 and then 0 -- apps/compass; ascending it is empty",
        ),
        ("Arrows: move", {"Left", "Right", "Up", "Down"}, "one word, four keys"),
        ("Arrows/WASD: steer", {"Left", "W", "A", "S", "D"}, "eight keys in two words"),
        ("A-Z", {"A", "M", "Z"}, "a letter range"),

        ("the barrows of old", set(), "control: no word boundary, so no match"),
        ("press - to zoom out", set(), "control: a lone hyphen is the minus key"),
    ]
    # `names_the_key` cases, kept beside the others because the two rules
    # together decide every row of the report.
    named: list[tuple[str, str, bool, str]] = [
        ("A", "Add City", False, "a capital inside a word does not name a key"),
        ("A", "A: analog", True, "a capital standing alone does"),
        ("A", "press a to switch", False, "lowercase does not -- `a` is an article"),
        ("C", "press c to close", False, "lowercase in prose still does not"),
        ("G", "Grid view", False, "control: this is the exact pair that hid ten keys"),
        ("F5", "F5: refresh", True, "a multi-character name is a substring match"),
        ("Esc", "Esc: back", True, "ditto"),
    ]
    # `payload_keys` cases. Every one is a line that occurs in
    # `apps/markdowneditor`, and the controls are the three shapes that
    # separate "this app answers this key" from "this key appears here":
    # the bridge that builds one, the catch-all that binds one, and the test
    # that presses one. Without them the payload scan reports 21 keys for an
    # app that binds 9.
    payloads: list[tuple[str, set[str], str]] = [
        ("Key::Char('h' | 'H' | 'f' | 'F') => {", {"H", "F"},
         "one arm, four spellings, two keys"),
        ("Key::Function(5) => app.refresh(),", {"F5"},
         "a function key carried in the payload"),
        ("Key::Char('2') => app.heading(2),", {"Num2"},
         "a digit in a payload is the digit key"),
        ("GKey::F1 => Key::Function(1),", set(),
         "control: the bridge builds a key, it does not answer one"),
        ("_ => Key::Char(printable(ev)?),", set(),
         "control: a catch-all building a key from typed text"),
        ("Key::Char(c) => app.insert(c),", set(),
         "control: a binding, not a literal -- this is text input"),
        ("handle_key(&mut app, Key::Char('q'), mods);", set(),
         "control: a test presses it; no arm in the app answers it"),
    ]
    # `is_label` cases. The terminal's own escape sequences were being read
    # as the terminal naming keys, which made two forwarded keys look named.
    labels: list[tuple[str, bool, str]] = [
        (r"\x1b[Z", False, "an escape sequence is bytes, not a label"),
        (r"\x1b[{};{}R", False, "the cursor report that made R look named"),
        ("Ctrl+R  Refresh", True, "a label that happens to name the same key"),
    ]
    # `typed_keys` cases. Each is the shape of a real crate, because the whole
    # difficulty is telling a shortcut from a parser and both are a match on a
    # character. Built with chr(10) rather than escapes: these are fixtures of
    # Rust source inside Python source, and every layer of quoting between here
    # and the file has cost me an hour at least once today.
    handler = chr(10).join([
        "fn handle_key(&mut self, event: &KeyEvent) -> bool {",
        "    self.handle_typed(event)",
        "}",
        "fn handle_typed(&mut self, event: &KeyEvent) -> bool {",
        "    match ch {",
        "        'g' | 'G' => self.grid(),",
        "        '0' => self.fit(),",
        "    }",
        "}",
    ])
    two_hops = chr(10).join([
        "fn handle_key(&mut self, key: &KeyEvent) -> EventResult {",
        "    self.process_prefix_key(c)",
        "}",
        "fn process_prefix_key(&mut self, key: char) {",
        "    match key {",
        "        'n' => self.next_window(),",
        "    }",
        "}",
    ])
    parser = chr(10).join([
        "fn unit_suffix(c: char) -> u64 {",
        "    match c {",
        "        'B' => 1,",
        "        'K' => 1024,",
        "    }",
        "}",
    ])
    not_an_arm = chr(10).join([
        "fn handle_key(&mut self, event: &KeyEvent) -> bool {",
        "    let tick = name.starts_with('a');",
        "    let end = rest.find(';');",
        "    false",
        "}",
    ])
    typed: list[tuple[list[str], set[str], str]] = [
        ([handler], {"G", "Num0"}, "a char arm in a function taking a KeyEvent"),
        ([two_hops], {"N"}, "reached through a caller, not by the handler name"),
        ([parser], set(), "control: a unit parser takes a char and answers no key"),
        ([not_an_arm], set(), "control: char literals that are not match arms"),
        (
            [two_hops.replace("handle_key", "on_key")],
            {"N"},
            "the rule is reachability, so renaming the handler changes nothing",
        ),
    ]
    # `label_keys` cases. The four controls are the shapes that made the
    # looser rule wrong: a format placeholder, a unit, an aperture and a doc
    # fixture. Every one of them contains a standalone lowercase letter, and
    # accepting those moved 26 keys off this queue for no reason at all.
    labels_read: list[tuple[str, set[str], str]] = [
        ("c", {"C"}, "a whole literal that is one key cap"),
        ("n / p", {"N", "P"}, "two caps in one column, as tmux prints them"),
        ("{v:.0}", set(), "control: a format placeholder"),
        ("{}h {}m", set(), "control: units"),
        ("f/{ap:.1}", set(), "control: an aperture, which even has a slash"),
        ("add(a: u32, b: u32)", set(), "control: a doc fixture"),
    ]
    bad = 0
    for text, want_read, why in labels_read:
        got_read = label_keys(text)
        ok = got_read == want_read
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       label_keys({text!r}) -> {sorted(got_read)}")
            bad += 1
    for sources, want_typed, why in typed:
        got_typed = typed_keys(sources)
        ok = got_typed == want_typed
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       typed_keys -> {sorted(got_typed)}, wanted {sorted(want_typed)}")
            bad += 1
    for text, want_label, why in labels:
        got_label = is_label(text)
        ok = got_label == want_label
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       is_label({text!r}) -> {got_label}")
            bad += 1
    for code, want_keys, why in payloads:
        got_keys = payload_keys(code)
        ok = got_keys == want_keys
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       payload_keys({code!r}) -> {sorted(got_keys)},"
                  f" wanted {sorted(want_keys)}")
            bad += 1
    for name, hay, want_hit, why in named:
        got_hit = names_the_key(name, hay)
        ok = got_hit == want_hit
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       names_the_key({name!r}, {hay!r}) -> {got_hit}")
            bad += 1
    for text, want, why in cases:
        got = spelled_out(text)
        ok = want <= got if want else not got
        print(f"  {'ok  ' if ok else 'FAIL'} {why}")
        if not ok:
            print(f"       {text!r} -> {sorted(got)}, wanted {sorted(want)}")
            bad += 1
    if bad:
        print(f"self-test: {bad} failure(s)")
        return 1
    total = len(cases) + len(named) + len(payloads) + len(labels) + len(typed) + len(labels_read)
    print(f"self-test ok -- {total} case(s)")
    return 0


#: Keys already looked at and found not to be a defect.
#:
#: See `key-survey-answered.txt` for the rules. The one that matters is that a
#: line naming a key this survey no longer reports is an *error*: an answer
#: about code that has since changed reads as a decision somebody made about
#: the code as it is now, and nobody did.
ANSWERED = Path(__file__).resolve().parent / "key-survey-answered.txt"


def answered() -> dict[tuple[str, str], str]:
    """`{(crate, key): reason}` from the answers file."""
    out: dict[tuple[str, str], str] = {}
    try:
        text = io.open(ANSWERED, encoding="utf-8").read()
    except OSError:
        return out
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(chr(9))
        if len(parts) >= 3:
            out[(parts[0], parts[1])] = parts[2]
    return out


#: The queue: keys that are a defect and are not fixed yet.
#:
#: Distinct from `ANSWERED`, and the distinction is the whole point. That file
#: says "looked at, and not a defect". This one says "a defect, not fixed
#: yet". An entry in the wrong file is how a bug quietly becomes a decision.
BASELINE = Path(__file__).resolve().parent / "key-survey-baseline.txt"


def baseline() -> set[tuple[str, str]]:
    """`{(crate, key)}` still queued."""
    out: set[tuple[str, str]] = set()
    try:
        text = io.open(BASELINE, encoding="utf-8").read()
    except OSError:
        return out
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(chr(9))
        if len(parts) >= 2:
            out.add((parts[0], parts[1]))
    return out


def main(argv: list[str]) -> int:
    if selftestflag.wants_selftest(argv):
        return _self_test()
    show_answered = "--answered" in argv
    known = answered()
    rows = []
    answered_rows: list[tuple[str, str, str]] = []
    apps = Path(__file__).resolve().parent.parent / "apps"
    for crate in sorted(apps.iterdir()):
        if not crate.is_dir():
            continue
        result = survey(crate)
        if result is None:
            continue
        total, _missing, unnamed, has_list = result
        kept = []
        for key in unnamed:
            reason = known.get((crate.name, key))
            if reason is None:
                kept.append(key)
            else:
                answered_rows.append((crate.name, key, reason))
        rows.append((len(kept), total, crate.name, kept, has_list))

    rows.sort(key=lambda r: (-r[0], -r[1], r[2]))
    listed = sum(1 for r in rows if r[4])
    print(f"{len(rows)} apps bind a letter, digit or function key; {listed} carry a key list\n")
    print(f"{'app':<22}{'keys':>5}{'unnamed':>9}  list  unnamed keys")
    queue_apps = 0
    queue_keys = 0
    for missing, total, name, unnamed, has_list in rows:
        if missing == 0:
            continue
        queue_apps += 1
        queue_keys += missing
        shown = ", ".join(unnamed[:10]) + ("..." if len(unnamed) > 10 else "")
        print(f"{name:<22}{total:>5}{missing:>9}  {'yes ' if has_list else '--  '}  {shown}")
    print(f"\nqueue: {queue_apps} app(s), {queue_keys} key(s)")

    # The gate. A key that is neither answered nor queued is new, and new is
    # the case worth failing on: an app can be added tomorrow that binds eight
    # keys and names none, and without this the total goes from 61 to 69 with
    # nobody looking.
    live = {(name, k) for _m, _t, name, keys, _l in rows for k in keys}
    queued = baseline()
    fresh = sorted(live - queued)
    gone = sorted(queued - live)
    if fresh:
        print(f"\n{len(fresh)} key(s) answered by an app and named nowhere, "
              f"not in {BASELINE.name}:")
        for crate_name, key in fresh:
            print(f"  {crate_name} {key}")
        print("  Name them where the app draws them, or -- if this is one of")
        print("  the shapes that only looks like a defect -- add it to")
        print(f"  {ANSWERED.name} with a reason that says what makes it one.")
    if gone:
        print(f"\n{len(gone)} line(s) in {BASELINE.name} name a key that is "
              f"no longer reported:")
        for crate_name, key in gone:
            print(f"  {crate_name} {key}")
        print("  Fixed, most likely -- delete the lines in the same commit as")
        print("  the fix. While one sits here that exact key cannot be")
        print("  reported again, so a file left to rot suppresses the thing")
        print("  it was written to track.")

    if answered_rows:
        print(f"{len(answered_rows)} already answered (--answered for why)")
        if show_answered:
            for crate_name, key, reason in sorted(answered_rows):
                print(f"  {crate_name} {key}: {reason}")

    # An answer about a key this survey no longer reports is not harmless. It
    # reads as a decision somebody made about the code as it is now, and the
    # code has moved -- the key may have been named, renamed or unbound, and
    # those three want different things done with the line.
    stale = sorted(set(known) - {(c, k) for c, k, _ in answered_rows})
    if stale:
        print(
            f"\n{len(stale)} line(s) in {ANSWERED.name} name a key this "
            f"survey no longer reports:"
        )
        for crate_name, key in stale:
            print(f"  {crate_name} {key}")
        print("  Why it is stale decides what to do, and the three answers")
        print("  differ:")
        print("    named   -- the app names the key now. Delete the line; the")
        print("               reason it carried is in the commit that did it.")
        print("    unbound -- the app no longer answers the key at all.")
        print("               Delete it, and check the reason did not describe")
        print("               something that outlived the binding.")
        print("    moved   -- still unnamed, under another key. Re-point the")
        print("               line rather than deleting it, or the next run")
        print("               re-offers the row with nothing recorded against")
        print("               it.")
        return 1
    return 1 if (fresh or gone) else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
