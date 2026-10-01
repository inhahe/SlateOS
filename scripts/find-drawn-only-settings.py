#!/usr/bin/env python3
r"""Which settings does a window let you change, and then only draw?

`find-echoed-settings.py`'s question, for programs with a window.

That checker asks which settings a program reads only to print back -- the
startup banner that says `idle_timeout=600s` about a timeout nothing applies.
A windowed program has the same defect in a form that checker cannot see:
**a settings row.** The row draws the value, Enter changes it, the row draws
the new value -- and nothing else in the program reads it. The user has done
the only check available to them, and it passed.

`apps/videoplayer` is the case this was written from (2026-09-27). Its
Settings tab had eight rows, every one changeable, and six of them --
Resume Playback, Remember Volume, Hardware Decode, Auto-load Subtitles, On
Finish, Deinterlace -- were read by exactly three things: the function that
turns a row into its text (`on_off(prefs.hardware_decode)`), the function
that flips it (`prefs.hardware_decode = !prefs.hardware_decode`), and, once
the settings were kept, the function that saves them. None of those is the
program *doing* anything with the setting, and the echo checker saw none of
it: the reads are not inside a `println!`, and the struct is called
`PlayerPreferences`, which its name filter does not match.

## What counts as not acting

A read of a setting's field is **not acting** when it is one of:

  * **echoed** -- turned into its own text: interpolated straight into a
    `format!`, handed to an on/off helper (`on_off(x.f)`), or named by a
    method on it (`x.f.label()`), or read anywhere in a function whose job is
    a row's text (`fn value`, `fn label`, `fn summary`, ...);
  * **self-updated** -- read to compute its own next value, in a statement
    that assigns the same field (`x.f = !x.f`, `x.f = x.f.next()`);
  * **kept** -- read to be written to a file, in a function that saves or
    serialises (`save_*`, `write_*`, `to_yaml`, ...).

Everything else is acting -- including a read in a `render` function that
decides *what* is drawn (`if prefs.show_grid { ... }`), because that is what
a display setting correctly does. That errs toward silence, like the echo
checker: it under-reports rather than accuses.

## The per-struct rule, kept

A field is reported only when it is read, never read in a way that acts, and
**at least one sibling in its struct is read in a way that acts**. A struct
nobody acts on at all is either a dump or a module nobody has wired -- the
echo checker's reasoning, which holds here unchanged.

## The fix, and why this still reports it afterwards

The fix is the program acting on the setting -- or, where it cannot yet
(`apps/videoplayer` has no decoder, so Hardware Decode has nothing to decode),
saying so beside the value. The second leaves the field read only into
output, so it keeps reporting here; that is on purpose, for the reason
`find-echoed-settings.py` gives. Answered rows go in `KNOWN`, with why, so
the next reading is a lookup.

This reports and does not gate: see the echo checker's "Why this reports and
does not gate", which applies word for word.

Usage:  python scripts/find-drawn-only-settings.py [--roots=apps,gui]
        python scripts/find-drawn-only-settings.py --self-test
"""

from __future__ import annotations

import argparse
import bisect
import importlib.util
import os
import re
import sys
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rustlex  # noqa: E402
import selftestflag  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent

DEFAULT_ROOTS = ("apps", "gui")


def _echo_checker():
    """`find-echoed-settings.py`, imported rather than copied.

    Its struct and field parsing, its print-macro spans and its "is this
    value interpolated as it is?" test are the same questions asked here; two
    copies of them is the arrangement where a fix lands in one. The hyphen in
    the name is why this goes through `importlib`.
    """
    path = Path(__file__).resolve().parent / "find-echoed-settings.py"
    spec = importlib.util.spec_from_file_location("find_echoed_settings", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


ECHO = _echo_checker()

# The echo checker's names, and `Preferences`, which it does not match and
# which is what a windowed program most often calls its settings.
CONFIG_STRUCT = re.compile(
    r"(?:Config|Settings|Options|Opts|Prefs|Preferences|Params)$"
)

# A function whose whole job is a row's text: every read in it is echoed.
TEXT_FN = re.compile(
    r"^(?:value|label|text|summary|describe|description|display_value|"
    r"value_text|value_label|status_text|caption|shown)$"
)

# A function that keeps settings: reading a field to write it down is not
# acting on it.
KEEP_FN = re.compile(
    r"^(?:save|store|persist|write|serialize|serialise|to_yaml|to_doc|to_json|"
    r"to_toml|export|dump)(?:_|$)|_(?:settings|prefs|preferences|config)_to_"
)

# A helper that turns a flag into its own words, with the field's path as its
# argument: `on_off(prefs.` just before `.hardware_decode`.
HELPER_CALL = re.compile(
    r"(?<![A-Za-z0-9_])(?:on_off|onoff|yes_no|bool_label|bool_text|flag_label|"
    r"enabled_label|check_mark)\s*\(\s*&?\s*[A-Za-z0-9_.]*$"
)

# `&mut prefs.` just before the field: handed over to be changed, which is the
# setting being written, not acted on -- a table of `(key, &mut prefs.flag)`
# pairs that a loader fills and a saver reads is the usual reason.
MUT_BORROW = re.compile(r"&\s*mut\s+[A-Za-z0-9_.]*$")

# A method that names a value rather than uses it, right after the field.
NAMING_METHOD = re.compile(
    r"\s*\.\s*(?:label|name|as_str|to_string|display|shown|title)\s*\(\s*\)"
)

STATEMENT_START = re.compile(r"(?:;|\{|\}|=>)")


class Index:
    """What `classify` asks of a crate's code for every read, found once.

    Asked per read by scanning from the top of the file, the nearest `fn` and
    the start of the statement made the scan quadratic: a few minutes on
    `apps/`, most of it in the largest windows. Sorted positions and a binary
    search answer the same questions.
    """

    def __init__(self, code: str):
        self.code = code
        self.spans = ECHO.print_spans(code)
        decls = list(ECHO.FN_DECL.finditer(code))
        self.fn_at = [m.start() for m in decls]
        self.fn_name = [m.group(1) for m in decls]
        self.statement_ends = [m.end() for m in STATEMENT_START.finditer(code)]

    def enclosing_fn(self, pos: int) -> str | None:
        """The nearest `fn` declared before `pos` -- `ECHO.enclosing_fn`'s rule."""
        i = bisect.bisect_left(self.fn_at, pos) - 1
        return self.fn_name[i] if i >= 0 else None

    def statement_before(self, pos: int) -> str:
        """The statement or match arm `pos` is in, up to `pos`."""
        i = bisect.bisect_right(self.statement_ends, pos) - 1
        return self.code[self.statement_ends[i] if i >= 0 else 0:pos]


def classify(index: Index, field: str, pos: int) -> str:
    """How the read of `field` at `pos` uses it: echo, self, kept or acting.

    `pos` is the `.` of `.field`, as `field_reads` gives it.
    """
    code = index.code
    fn = index.enclosing_fn(pos)
    if fn and TEXT_FN.match(fn):
        return "echo"
    if fn and KEEP_FN.search(fn):
        return "kept"
    if re.search(rf"\.\s*{re.escape(field)}\s*=(?!=)", index.statement_before(pos)):
        return "self"
    before = code[max(0, pos - 80):pos]
    if MUT_BORROW.search(before):
        return "self"
    if HELPER_CALL.search(before):
        return "echo"
    name = re.compile(rf"\.\s*{re.escape(field)}").match(code, pos)
    if name and NAMING_METHOD.match(code, name.end()):
        return "echo"
    if ECHO.directly_interpolated(code, pos, index.spans):
        return "echo"
    return "acting"


def analyse(
    code: str, stranded: list | None = None
) -> list[tuple[str, str, dict[str, int]]]:
    """(struct, field, uses) for fields read, never acting, with an acting sibling.

    A field every read of which sits in a function nothing calls goes to
    `stranded` instead: nothing shows it either, so it confirms nothing to
    anybody -- that is `find-stranded-serialisers.py`'s finding, a function
    to wire up or delete, and two tools reporting one defect reads as two.
    """
    index = None
    found = []
    for struct, fields in ECHO.struct_fields(code).items():
        if not CONFIG_STRUCT.search(struct) or len(fields) < 2:
            continue
        index = index or Index(code)
        idle, acting = [], False
        for field in dict.fromkeys(fields):
            reads = ECHO.field_reads(code, field)
            if not reads:
                continue
            uses: dict[str, int] = {}
            for pos in reads:
                kind = classify(index, field, pos)
                uses[kind] = uses.get(kind, 0) + 1
            if "acting" in uses:
                acting = True
            elif all(
                (fn := index.enclosing_fn(pos)) is not None and not ECHO.is_called(code, fn)
                for pos in reads
            ):
                if stranded is not None:
                    stranded.append((struct, field))
            else:
                idle.append((field, uses))
        if acting:
            found.extend((struct, f, uses) for f, uses in idle)
    return found


def crate_code(crate: Path) -> str:
    """A crate's live code, every file of it, tests and comments removed."""
    parts = []
    for path in sorted((crate / "src").rglob("*.rs")):
        try:
            src = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        live, _ = rustlex.live_code(src)
        parts.append(rustlex.strip_noise(live))
    return "\n".join(parts)


#: Rows read and answered, with why -- so nobody reads them twice. A row here
#: the scan no longer reports is printed as stale.
_TORRENT = "the settings panel opens with SETTINGS_NOT_APPLIED: nothing here downloads"
_NO_SHOT = "NO_SCREENSHOTS: the block says no screenshot can be taken -- no frame is decoded"
_REMOTE_RS = ("remote.rs is reached from nowhere in the settings app; wiring it up or "
              "deleting it is the operator's C-Q17")

KNOWN: dict[tuple[str, str, str], str] = {
    ("apps/diskimager", "CreateOptions", "output_path"): "a record, not a control: kept so "
    "'Image created: ...' names the file the copy really wrote",
    ("apps/fontmanager", "RenderSettings", "default_size_pt"): "SETTINGS_NOT_CARRIED: the "
    "panel says nothing carries it to the toolkit",
    ("apps/netmanager", "VpnConfig", "server_address"): "the VPN list is empty in the "
    "shipping program -- only tests fill it; tunnels are vpnmanager's",
    ("apps/netmanager", "VpnConfig", "protocol"): "as server_address",
    ("apps/netmanager", "VpnConfig", "auto_connect"): "as server_address",
    ("apps/netscan", "ScanConfig", "discovery_method"): "a label: it names the method the "
    "scan uses, TCP connect, and nothing changes it (its doc comment says so)",
    ("apps/pomodoro", "Settings", "notification_sound"): "nothing here plays sound; the "
    "settings say so beneath their rows (NO_SOUND, 2026-09-27)",
    ("apps/screenrecorder", "OutputSettings", "auto_increment"): "data, not display: it "
    "numbers the output file's name (generate_filename) -- and no take is made (CANNOT_RECORD)",
    ("apps/settings", "DynDnsConfig", "update_interval_minutes"): _REMOTE_RS,
    ("apps/settings", "RemoteDesktopConfig", "max_sessions"): _REMOTE_RS,
    ("apps/settings", "RemoteDesktopConfig", "idle_timeout_minutes"): _REMOTE_RS,
    ("apps/torrent", "ClientSettings", "max_active_downloads"): _TORRENT,
    ("apps/torrent", "ClientSettings", "max_active_seeds"): _TORRENT,
    ("apps/torrent", "ClientSettings", "max_connections_global"): _TORRENT,
    ("apps/torrent", "ClientSettings", "dht_enabled"): _TORRENT,
    ("apps/torrent", "ClientSettings", "pex_enabled"): _TORRENT,
    ("apps/torrent", "ClientSettings", "encryption_mode"): _TORRENT,
    ("apps/torrent", "ClientSettings", "enable_utp"): _TORRENT,
    ("apps/torrent", "ClientSettings", "proxy_type"): _TORRENT,
    ("apps/videoplayer", "PlayerPreferences", "resume_playback"): "nothing decodes, so "
    "nothing plays to resume; the row says Not applied (2026-09-27)",
    ("apps/videoplayer", "PlayerPreferences", "hardware_decode"): "as resume_playback",
    ("apps/videoplayer", "PlayerPreferences", "on_finish"): "as resume_playback",
    ("apps/videoplayer", "PlayerPreferences", "deinterlace"): "as resume_playback",
    ("apps/videoplayer", "PlayerPreferences", "screenshot_config"): _NO_SHOT,
    ("apps/videoplayer", "ScreenshotConfig", "include_subtitles"): _NO_SHOT,
    ("apps/videoplayer", "ScreenshotConfig", "quality"): _NO_SHOT,
}


def _self_test() -> int:
    failures = 0

    def expect(label, got, want):
        nonlocal failures
        if got != want:
            failures += 1
            print(f"FAIL  {label}\n  got  {got!r}\n  want {want!r}")
        else:
            print(f"  ok    {label}")

    def fields(src):
        return [(s, f) for s, f, _ in analyse(src)]

    # The videoplayer's shape: a row drawn, flipped and saved; a sibling acted on.
    player = """
struct PlayerPreferences {
    pub hardware_decode: bool,
    pub osd_duration_ms: u64,
    pub on_finish: OnFinish,
}
fn on_off(b: bool) -> &'static str { if b { "On" } else { "Off" } }
impl Row {
    fn value(self, prefs: &PlayerPreferences) -> &'static str {
        match self {
            Self::Hw => on_off(prefs.hardware_decode),
            Self::Finish => prefs.on_finish.label(),
        }
    }
    fn cycle(self, prefs: &mut PlayerPreferences) {
        match self {
            Self::Hw => prefs.hardware_decode = !prefs.hardware_decode,
            Self::Finish => prefs.on_finish = prefs.on_finish.next(),
        }
    }
}
fn show_osd(app: &mut App) { app.left = app.preferences.osd_duration_ms; }
fn save_settings(doc: &mut Doc, prefs: &PlayerPreferences) {
    doc.set_bool("hardware_decode", prefs.hardware_decode);
}
fn main() { Row::Hw.value(&p); Row::Hw.cycle(&mut p); show_osd(&mut a); save_settings(&mut d, &p); }
"""
    expect("a row drawn, flipped and saved, beside one acted on, is reported",
           fields(player),
           [("PlayerPreferences", "hardware_decode"), ("PlayerPreferences", "on_finish")])

    # A display flag: read in render to decide what is drawn. That is acting.
    grid = """
struct CanvasSettings {
    pub show_grid: bool,
    pub zoom: f32,
}
fn render(s: &CanvasSettings, out: &mut Vec<Cmd>) {
    if s.show_grid { draw_grid(out); }
    out.push(Cmd::Scale(s.zoom));
}
fn main() { render(&s, &mut out); }
"""
    expect("a flag that decides what is drawn is not reported", fields(grid), [])

    # A struct nothing acts on at all: a dump, or unwired -- not this finding.
    dump = """
struct ViewOptions {
    pub a: bool,
    pub b: bool,
}
fn value(o: &ViewOptions) -> String { format!("{} {}", o.a, o.b) }
fn main() { value(&o); }
"""
    expect("a struct none of whose fields acts is not reported", fields(dump), [])

    # A self-update split over two lines by rustfmt is still a self-update.
    split = """
struct AppPrefs {
    pub language: Option<Lang>,
    pub volume: u32,
}
fn cycle(p: &mut AppPrefs) {
    p.language =
        Lang::after(p.language);
}
fn play(p: &AppPrefs) { set_level(p.volume); }
fn main() { cycle(&mut p); play(&p); }
"""
    expect("a self-update over two lines does not act", fields(split),
           [("AppPrefs", "language")])

    # A table of `&mut` fields a loader fills and a saver walks is writing,
    # not acting.
    table = """
struct KeptPrefs {
    pub resume: bool,
    pub level: u32,
}
fn switches(p: &mut KeptPrefs) -> [(&'static str, &mut bool); 1] {
    [("resume", &mut p.resume)]
}
fn play(p: &KeptPrefs) { set_level(p.level); }
fn main() { switches(&mut p); play(&p); }
"""
    expect("a field handed out by &mut does not act", fields(table),
           [("KeptPrefs", "resume")])

    # Read only by a formatter nothing calls: stranded, not echoed.
    uncalled = """
struct AudioSettings {
    pub codec: Codec,
    pub rate: u32,
}
impl AudioSettings {
    pub fn summary(&self) -> String { format!("{}", self.codec.label()) }
}
fn convert(s: &AudioSettings) { resample(s.rate); }
"""
    aside: list = []
    expect("a field read only by a formatter nothing calls is not echoed",
           [(s, f) for s, f, _ in analyse(uncalled, aside)], [])
    expect("...it is set aside as stranded", aside, [("AudioSettings", "codec")])

    # A value interpolated straight into a label is echoed; one handed to a
    # function that picks a rendering is not.
    shown = """
struct ClockSettings {
    pub zone: String,
    pub hour24: bool,
    pub tick: u64,
}
fn render(s: &ClockSettings) -> String {
    let label = format!("Zone: {}", s.zone);
    let time = format_time(now(), s.hour24);
    wait(s.tick);
    label + &time
}
fn main() { render(&s); }
"""
    expect("interpolated is echoed; handed to a formatter is acting",
           fields(shown), [("ClockSettings", "zone")])

    # Reads only in tests do not act: `live_code` removes them.
    tested = """
struct GameOptions {
    pub hints: bool,
    pub speed: u32,
}
fn step(o: &GameOptions) { advance(o.speed); }
fn value(o: &GameOptions) -> &'static str { if o.hints { "On" } else { "Off" } }
fn main() { step(&o); value(&o); }
#[cfg(test)]
mod tests {
    #[test]
    fn t() { assert!(super::uses(OPTS.hints)); }
}
"""
    live, _ = rustlex.live_code(tested)
    expect("a read in a test does not act",
           fields(rustlex.strip_noise(live)), [("GameOptions", "hints")])

    print(f"\n{failures} failure(s)")
    return 1 if failures else 0


def main() -> int:
    if selftestflag.wants_selftest(sys.argv[1:]):
        return _self_test()
    ap = argparse.ArgumentParser()
    ap.add_argument("--roots", default=",".join(DEFAULT_ROOTS))
    args = ap.parse_args()
    roots = [ROOT / r for r in args.roots.split(",") if r]
    missing = [r for r in roots if not r.is_dir()]
    if missing:
        print(f"no such root(s): {', '.join(str(m) for m in missing)}", file=sys.stderr)
        return 2

    rows, stranded, judged = [], [], 0
    for root in roots:
        for manifest in sorted(root.glob("*/Cargo.toml")):
            crate = manifest.parent
            code = crate_code(crate)
            judged += sum(
                1 for s, fs in ECHO.struct_fields(code).items()
                if CONFIG_STRUCT.search(s) and len(fs) >= 2
            )
            rel = crate.relative_to(ROOT).as_posix()
            unread: list = []
            rows.extend((rel, s, f, uses) for s, f, uses in analyse(code, unread))
            stranded.extend((rel, s, f) for s, f in unread)

    seen = {(c, s, f) for c, s, f, _ in rows}
    open_rows = [r for r in rows if (r[0], r[1], r[2]) not in KNOWN]
    answered = [r for r in rows if (r[0], r[1], r[2]) in KNOWN]
    # Only answers about the roots scanned: an answer about `apps/` is not
    # stale because this run looked at `gui/`.
    scanned = {r.relative_to(ROOT).as_posix() for r in roots}
    stale = [
        k for k in KNOWN
        if k not in seen and k[0].split("/", 1)[0] in scanned
    ]

    print(f"settings structs judged: {judged}")
    print(f"{len(open_rows)} setting(s) read only to be drawn, flipped or kept, "
          f"beside a sibling the program acts on -- not yet answered\n")
    for crate, struct, field, uses in open_rows:
        how = ", ".join(f"{k} {n}" for k, n in sorted(uses.items()))
        print(f"  {crate:28} {struct}.{field:28} ({how})")
    if answered:
        print(f"\n{len(answered)} answered:")
        for crate, struct, field, _ in answered:
            print(f"  {crate:28} {struct}.{field:28} {KNOWN[(crate, struct, field)]}")
    if stale:
        print("\nstale answers -- no longer reported, so no longer about this code:")
        for crate, struct, field in stale:
            print(f"  {crate} {struct}.{field}")
    if stranded:
        print(f"\n{len(stranded)} read only by functions nothing calls -- "
              "find-stranded-serialisers.py's finding, not this one's:")
        for crate, struct, field in stranded:
            print(f"  {crate:28} {struct}.{field}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
