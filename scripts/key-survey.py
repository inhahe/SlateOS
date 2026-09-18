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
        if "SHORTCUTS" in live or "ALL_KEY_ACTIONS" in live:
            has_list = True
        # Comments and literals blanked before the key scan: a key named in
        # a comment is the fourth way a search says nothing, and my own
        # comment about Command::new once poisoned a grep for it.
        code = rustlex.strip_noise(live, keep_literals=False)
        matched.update(KEY_RE.findall(code))
        literals.extend(rustlex.string_literals(live))

    matched -= IGNORE
    matched -= CONVENTIONAL
    if not matched:
        return None

    haystack = " | ".join(literals)
    unnamed = sorted(
        k
        for k in matched
        if not any(n in haystack for n in NAMES.get(k, (k,)))
    )
    return len(matched), len(unnamed), unnamed, has_list


def main() -> int:
    rows = []
    apps = Path(__file__).resolve().parent.parent / "apps"
    for crate in sorted(apps.iterdir()):
        if not crate.is_dir():
            continue
        result = survey(crate)
        if result is None:
            continue
        total, missing, unnamed, has_list = result
        rows.append((missing, total, crate.name, unnamed, has_list))

    rows.sort(key=lambda r: (-r[0], -r[1], r[2]))
    listed = sum(1 for r in rows if r[4])
    print(f"{len(rows)} apps bind a letter, digit or function key; {listed} carry a key list\n")
    print(f"{'app':<22}{'keys':>5}{'unnamed':>9}  list  unnamed keys")
    for missing, total, name, unnamed, has_list in rows:
        if missing == 0:
            continue
        shown = ", ".join(unnamed[:10]) + ("..." if len(unnamed) > 10 else "")
        print(f"{name:<22}{total:>5}{missing:>9}  {'yes ' if has_list else '--  '}  {shown}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
