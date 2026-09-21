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
    literals: list[str] = []
    has_list = False

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
                break
        # Comments and literals blanked before the key scan: a key named in
        # a comment is the fourth way a search says nothing, and my own
        # comment about Command::new once poisoned a grep for it.
        code = rustlex.strip_noise(live, keep_literals=False)
        matched.update(KEY_RE.findall(code))
        # The payload scan needs the literals `code` has just blanked, so it
        # reads a second pass with comments stripped and literals kept.
        matched.update(payload_keys(rustlex.strip_noise(live, keep_literals=True)))
        literals.extend(l for l in rustlex.string_literals(live) if is_label(l))

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
    bad = 0
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
    print(f"self-test ok -- {len(cases) + len(named) + len(payloads) + len(labels)} case(s)")
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
