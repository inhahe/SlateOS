#!/usr/bin/env python3
"""Generate videocodec's subtitle fixtures and their answers.

Each fixture NAME.mkv (or NAME.webm, NAME.mp4) carries one text subtitle track;
NAME.ffmpeg.srt is what ffmpeg makes of that track with `-c:s srt`: every
cue's start and end, and its text as SRT markup. Each cue's text is one
question about the conversion, so a disagreement names the rule broken.

NAME.srt is the answer `tests/subtitles.rs` holds `videocodec::Subtitles` to,
cue by cue: ffmpeg's, except for the cues DEPARTURES below names. Those are
where the crate follows the format's own renderer instead -- libass for ASS
and SSA, the WebVTT specification for WebVTT -- because ffmpeg's conversion
says something the renderer does not show (a comment or a drawing printed as
text, a style lost, a reset to the wrong style, a size in the wrong units).
Each such answer was checked by hand against libass's source or the
specification, cue by cue, and says why beside it; the rest of every answer
is ffmpeg's, untouched. `diff NAME.ffmpeg.srt NAME.srt` shows exactly what
differs.

Most tracks are written by ffmpeg from the sources below (`-c:s copy`), so
they are what ffmpeg's muxers store: S_TEXT/UTF8 from an `.srt`, S_TEXT/ASS
from an `.ass`, and WebM's D_WEBVTT/SUBTITLES from a `.vtt` -- each block the
cue's identifier, its settings and its text, a line each. ffmpeg writes no
S_TEXT/SSA or Matroska's S_TEXT/WEBVTT (the text alone in the block), so
`ssa.mkv` and `webvtt_mkv.mkv` are MKVToolNix's mkvmerge's. ffmpeg reads
S_TEXT/SSA as it reads S_TEXT/ASS; S_TEXT/WEBVTT this ffmpeg does not know,
so `webvtt_mkv`'s answer is ffmpeg's for the same cues in WebM.

MP4's 3GPP timed text (`tx3g`): `movtext.mp4` is ffmpeg's mov_text encoder's
from SubRip; the others are written here, box by box (`tx3g_mp4`), for what
that encoder never writes -- a styled default, justification, colours,
fonts, style runs out of order. ffmpeg's SRT of each is the answer, as for
the rest.

Run from this directory, on Windows, with gyan.dev's ffmpeg (2026-03-09, git
9b7439c31b) and MKVToolNix's mkvmerge 99.0 on PATH (or in the MKVMERGE
environment variable): `python generate_subtitle_fixtures.py`.
"""

import os
import shutil
import struct
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MKVMERGE = os.environ.get("MKVMERGE", "mkvmerge")

# --- SubRip: markup, as players have written it for twenty years ---
SUBRIP = [
    "Plain text",
    "<i>Italic</i> and <b>bold</b> and <u>underline</u> and <s>strike</s>",
    "<I>Upper</I> <B>BOLD</B> <U>Under</U>",
    "<i>unclosed",
    "</i>closing without opening",
    "<b><i>both</b></i> mis-nested",
    "<i><i>twice</i> inner</i> outer",
    '<font color="#ff0000">hex</font> <font color="#00FF00">upper hex</font>',
    "<font color=red>named</font> <font color=Navy>navy</font> <font color=lightgoldenrodyellow>long name</font>",
    "<font color=#ff00ff>bare hash</font> <font color='#0000ff'>single quotes</font> <font color=00ff00>no hash</font>",
    '<font color="#80ff0000">eight digits</font> <font color="#abc">three digits</font>',
    '<font color="nonsense">bad name</font> <font color="">empty</font>',
    '<font face="Arial" size="20">face and size</font>',
    '<font size="40">size</font> <font size=12>bare size</font> <font size="big">bad size</font>',
    '<font face="Times New Roman">spaced face</font> <font face=Courier>bare face</font>',
    '<font color="#ff0000" face="Arial" size="30">all three</font>',
    '<font color="#ff0000"><font color="#00ff00">inner</font> outer</font>',
    "</font>stray close <font>no attributes</font>",
    '<font COLOR="#123456" SIZE="10">upper attributes</font>',
    "Two\nlines",
    "Three\nshort\nlines",
    "{\\an8}Top",
    "{\\an1}Bottom left {\\an9}second alignment",
    "{\\i1}ass in srt{\\i0} {\\b1}bold{\\b0} {\\pos(10,20)}pos",
    "{comment} kept {\\an5} middle",
    "a\\Nb \\n c \\h d",
    "<span>unknown</span> <foo bar=1>attr</foo> <br> break",
    "a < b > c <3 << >>",
    "& &amp; &lt;tag&gt; &quot;q&quot; &nbsp;x &#65;",
    "   leading spaces",
    "<i>   inner leading</i>",
    "trailing in tag <i>x   </i>",
    "été 日本語 \U0001f600",
    "<b>bold\nacross lines</b>",
    "<i></i>empty tag",
    "<i>a<b>b<u>c</u>d</b>e</i>",
    "<p>paragraph</p> <div>div</div>",
    "<i/>self closing <br/> slash",
    "< i >spaced tag</ i >",
    "line\n\n",
    '<font size="20" color="#ff0000">size first</font>',
    '<font color="#ff0000" size="20" face="Arial">color size face</font>',
    '<font color=" #ff0000 ">padded</font> <font color="#ff0000"extra>no space</font>',
    "{\\an10}out of range {\\an0}zero {\\anx}letter",
    "{\\an8}{\\an2}two first",
    "{ \\an8}space before {\\an8 }space after",
    "{unclosed brace",
    "}stray close brace",
    "<i>a</I> mixed case close",
    "<b>x</b><b>y</b> adjacent",
    '<font color="#ff0000">a<font color="#00ff00">b</font>c</font>after',
    "<b><i>x</b>y</i>z",
    '<font color="#ff0000" size="30">x</font>y',
    "tab\there",
    "​zero width",
]

# Every CSS colour name, and a few that are X11's alone, each in its own cue:
# which ffmpeg accepts, and as what, is the answer.
CSS_COLOURS = [
    "aliceblue", "antiquewhite", "aqua", "aquamarine", "azure", "beige", "bisque", "black",
    "blanchedalmond", "blue", "blueviolet", "brown", "burlywood", "cadetblue", "chartreuse",
    "chocolate", "coral", "cornflowerblue", "cornsilk", "crimson", "cyan", "darkblue",
    "darkcyan", "darkgoldenrod", "darkgray", "darkgreen", "darkgrey", "darkkhaki",
    "darkmagenta", "darkolivegreen", "darkorange", "darkorchid", "darkred", "darksalmon",
    "darkseagreen", "darkslateblue", "darkslategray", "darkslategrey", "darkturquoise",
    "darkviolet", "deeppink", "deepskyblue", "dimgray", "dimgrey", "dodgerblue", "firebrick",
    "floralwhite", "forestgreen", "fuchsia", "gainsboro", "ghostwhite", "gold", "goldenrod",
    "gray", "green", "greenyellow", "grey", "honeydew", "hotpink", "indianred", "indigo",
    "ivory", "khaki", "lavender", "lavenderblush", "lawngreen", "lemonchiffon", "lightblue",
    "lightcoral", "lightcyan", "lightgoldenrodyellow", "lightgray", "lightgreen", "lightgrey",
    "lightpink", "lightsalmon", "lightseagreen", "lightskyblue", "lightslategray",
    "lightslategrey", "lightsteelblue", "lightyellow", "lime", "limegreen", "linen", "magenta",
    "maroon", "mediumaquamarine", "mediumblue", "mediumorchid", "mediumpurple",
    "mediumseagreen", "mediumslateblue", "mediumspringgreen", "mediumturquoise",
    "mediumvioletred", "midnightblue", "mintcream", "mistyrose", "moccasin", "navajowhite",
    "navy", "oldlace", "olive", "olivedrab", "orange", "orangered", "orchid", "palegoldenrod",
    "palegreen", "paleturquoise", "palevioletred", "papayawhip", "peachpuff", "peru", "pink",
    "plum", "powderblue", "purple", "rebeccapurple", "red", "rosybrown", "royalblue",
    "saddlebrown", "salmon", "sandybrown", "seagreen", "seashell", "sienna", "silver",
    "skyblue", "slateblue", "slategray", "slategrey", "snow", "springgreen", "steelblue",
    "tan", "teal", "thistle", "tomato", "turquoise", "violet", "wheat", "white", "whitesmoke",
    "yellow", "yellowgreen",
    # Not CSS: X11's, or nobody's. (Not "random", which ffmpeg's colour
    # parser makes a random colour of: no answer.)
    "darkyellow", "lightred", "transparent", "RED", "LightGoldenRodYellow",
]
SUBRIP_COLOURS = [f"<font color={name}>{name}</font>" for name in CSS_COLOURS]

# --- ASS: an event's override tags, styles from the header ---
# No PlayResX or PlayResY: 288 lines, SRT's own, so that sizes are as written.
ASS_HEADER = """[Script Info]
ScriptType: v4.00+
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,16,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1
Style: Shout,Impact,32,&H000000FF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,2,2,8,10,10,10,1
Style: Lean,Georgia,24,&H0000FF00,&H000000FF,&H00000000,&H00000000,0,-1,-1,-1,100,100,0,0,1,2,2,5,10,10,10,1
Style: Big,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1
Style: Alpha,Arial,16,&H80FFFF00,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,7,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""

# (style, text)
ASS = [
    ("Default", "Plain line"),
    ("Default", "{\\i1}Italic{\\i0} and {\\b1}bold{\\b0} and {\\u1}under{\\u0} and {\\s1}strike{\\s0}"),
    ("Default", "{\\c&H0000FF&}red{\\c} {\\1c&HFF0000&}blue {\\fnCourier New}font {\\fs40}big{\\fs}"),
    ("Default", "Line one\\NLine two\\nsoft\\hhard"),
    ("Shout", "Shouted in style"),
    ("Lean", "Leaning style {\\i0}not italic{\\r}reset"),
    ("Big", "Big by style"),
    ("Alpha", "Alpha style top left"),
    ("Default", "{\\an8}Top {\\an2}bottom {\\a6}legacy"),
    ("Default", "{\\a6}legacy only"),
    ("Default", "{\\pos(320,240)\\fad(100,100)}Positioned {\\k20}ka{\\k30}ra{\\kf40}oke"),
    ("Default", "{\\i1\\b1}both{\\i0\\b0} done"),
    ("Default", "{\\b700}weight{\\b0} {\\i}bare{\\i} {\\c&HFFFFFF&}white"),
    ("Default", "   spaced   "),
    ("Default", "{\\alpha&H80&}half {\\3c&H00FF00&}outline {\\4c&H00FF00&}back"),
    ("Default", "<i>html in ass</i> & < >"),
    ("Shout", "{\\rDefault}reset to default {\\rLean}to lean"),
    ("Default", "{\\fs8\\fnTimes}tiny times{\\fs}{\\fn} back"),
    ("Default", "{\\i1}open {\\u1}both {\\i0}closes i {\\u0}then u"),
    ("Default", "{\\c&H00FF00&}green {\\c&H0000FF&}then red {\\c}reset"),
    ("Default", "{\\1c&H123456&\\fs30}two in one block"),
    ("Default", "no tags here {}empty block{\\} lone backslash"),
    ("Default", "{\\i1}unclosed italic"),
    ("Default", "{\\fnArial}default font named"),
    ("Default", "{\\fs16}default size named"),
    ("Nonexistent", "unknown style"),
    ("Default", "été 日本語"),
    ("Default", "{\\b1}bold\\Nacross{\\b0} lines"),
    ("Default", "{\\bord2\\shad1\\blur3\\frz45\\fscx120}effects only"),
    ("Default", "{\\i1}{\\i0}{\\i1}toggled"),
    ("Shout", "{\\an2}explicit bottom in a top style"),
    ("Shout", "{\\an8}explicit top in a top style"),
    ("Half", "fractional size"),
    ("Bold1", "bold written as 1"),
    ("Default", "{ \\i1}space before{\\i0 } and after"),
    ("Default", "{\\i1}open {unclosed brace"),
    ("Default", "}stray {\\b1}x{\\b0}"),
    ("Default", "{a{\\i1}b}nested braces"),
    ("Default", "{\\p1}m 0 0 l 100 0 100 100{\\p0}after drawing"),
    ("Default", "{This is a comment}Visible after comment"),
    ("Default", "{\\fs}{\\fn}{\\c}closes with nothing open"),
    ("Default", "{\\c&H00FF00}no trailing ampersand {\\c&HFF&}short {\\cH0000FF&}no ampersand"),
    ("Default", "{\\fs12.5}fractional override {\\fs-5}negative"),
    ("Default", "{\\i1}a\\Nb{\\i0}"),
    ("Default", "{\\i1}on {\\i}after bare"),
    ("Default", "{\\b1}on {\\b\\u1}bare then under"),
    ("Lean", "{\\i}bare in an italic style"),
    ("Default", "{\\fs30}big {\\fs-5}after minus five"),
    ("Default", "{\\fs30}big {\\fs+5}after plus five"),
    ("Default", "{\\b100}light {\\b700}heavy {\\b400}normal"),
    ("Default", "{\\c&H0000FF&}red {\\fs30}big {\\c}after colour reset"),
    ("Lean", "{\\i0}x {\\rNope}unknown reset"),
    ("Default", "{\\c&HFF0000&}blue {\\c&H00FF00}unterminated after"),
    ("Default", "tab\there"),
    ("Default", "{\\q2}soft\\nbreak in wrap style 2"),
    ("Shout", "middle {\\an1}late alignment"),
    ("Default", "{\\an0}invalid then {\\an8}valid"),
    ("Default", "{\\a4}illegal legacy"),
    ("Default", "{\\fnTimes\\i1}a{\\fn}b"),
    ("Default", "{\\i1\\i1}twice{\\i0} once off"),
    ("Default", "{\\fs0}zero size"),
    ("Default", "{\\fn0}face zero"),
    ("Default", "{\\fs20\\t(\\fs40)}animated stays as it starts"),
    ("Default", "{\\pos(10,20)\\i1}after a position"),
    ("Default", "{\\i1}\\Nleading break"),
]

# --- ASS laid out for 720 lines: sizes scaled to SRT's 288 ---
ASS_720_HEADER = """[Script Info]
ScriptType: v4.00+
PlayResX: 1280
PlayResY: 720
WrapStyle: 0

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Verdana,50,&H0000FFFF,&H000000FF,&H00000000,&H00000000,-1,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1
Style: Plain,Arial,40,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""

ASS_720 = [
    ("Default", "Default, fifty lines of 720"),
    ("Missing", "a style the script lacks is its Default"),
    ("default", "a style named in lower case"),
    ("*Default", "a starred style name"),
    ("Plain", "forty lines of 720, sixteen of 288"),
    ("Plain", "{\\fs60}sixty{\\fs} back"),
    ("Plain", "{\\rDefault}to default {\\r}back to plain"),
    ("Plain", "{\\i1}x {\\rNope}an unknown reset is the line's own style"),
    ("Plain", "{\\fs+5}one and a half times"),
    ("Default", "{\\b0}not bold {\\fs}{\\b}the style's again"),
]

# Styles only the probes above use, appended to the header's.
ASS_EXTRA_STYLES = (
    "Style: Half,Arial,18.5,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n"
    "Style: Bold1,Arial,16,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,1,1,0,0,100,100,0,0,1,2,2,2,10,10,10,1\n"
)

# --- SSA (v4): the older header, whose styles name their columns differently ---
SSA_HEADER = """[Script Info]
ScriptType: v4.00

[V4 Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, TertiaryColour, BackColour, Bold, Italic, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, AlphaLevel, Encoding
Style: Default,Arial,16,16777215,255,0,0,0,0,1,2,2,2,10,10,10,0,0
Style: Old,Verdana,28,255,255,0,0,-1,-1,1,2,2,6,10,10,10,0,0

[Events]
Format: Marked, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
"""

SSA = [
    ("Default", "Plain SSA"),
    ("Old", "Old style, top centre in SSA's numbering"),
    ("Default", "{\\i1}override{\\i0} in SSA"),
    ("Default", "{\\a10}legacy override in SSA"),
]

# --- WebVTT: cue text ---
WEBVTT = [
    "Plain cue",
    "<i>Italic</i> <b>bold</b> <u>under</u>",
    "<c.yellow>class</c> <v Roger>Voice</v> <lang en>lang</lang>",
    "&amp; &lt; &gt; x&nbsp;y &lrm;l &rlm;r &quot;q&quot;",
    "<ruby>漢<rt>kan</rt></ruby> <00:00:07.500>timed",
    "Two\nlines",
    "   spaced   ",
    "<b><i>nested</i></b> <i>unclosed",
    "<v.loud Roger>voice with class</v>",
    "<c.a.b>two classes</c>",
    "été 日本語",
    "<b>bold\nacross lines</b>",
    "brace {\\an8} and backslash \\N here",
    "<i.loud>classed italic</i>",
    "a < b and c > d",
    "it&#39;s &#x41; &apos;q&apos;",
    "<b><i>x</b>y</i>z",
]

# Cues placed by their settings, which ffmpeg ignores: (settings, text).
WEBVTT_PLACED = [
    ("line:0", "top by line number"),
    ("line:-1", "bottom by line number"),
    ("line:7", "seven lines down: the middle"),
    ("line:50%", "middle by percentage"),
    ("align:start position:0%", "left, as YouTube writes it"),
    ("align:end", "right aligned"),
    ("line:10% align:left", "top left"),
    ("line:0 position:90% align:end", "top right"),
    ("align:center position:20%", "centred a fifth across: left"),
    ("vertical:rl line:0", "vertical, put where a cue without settings goes"),
    ("line:0,bottom", "a setting the specification passes over"),
]

# --- 3GPP timed text in MP4 ---
# Through ffmpeg's own mov_text encoder, from SubRip: the styles it writes.
MOVTEXT = [
    "Plain text",
    "<i>Italic</i> and <b>bold</b> and <u>under</u>",
    '<font size="30">big</font> text',
    "Two\nlines",
    "été <b>日本</b> end",
    "<b><i>both</i></b> nested",
]


# Written here, sample by sample, for what ffmpeg's encoder never writes: a
# box per cue of a tx3g sample (a StyleRecord, `styl`, `hlit`...).
def tx3g_box(kind, *parts):
    body = b"".join(parts)
    return struct.pack(">I4s", 8 + len(body), kind) + body


def tx3g_style(start, end, font=1, flags=0, size=16, rgba=0xFFFFFFFF):
    """A StyleRecord: characters [start, end), a font ID, face flags (1 bold,
    2 italic, 4 underline), a size and a colour."""
    return struct.pack(">HHHBBI", start, end, font, flags, size, rgba)


def tx3g_setup(fonts=((1, "Arial"),), across=1, down=-1, default=None):
    """A TextSampleEntry's fields after the sample entry's header: display
    flags, justification across (0 left, 1 centre, -1 right) and down (0
    top, 1 middle, -1 bottom), background, text box, default style, fonts."""
    default = default if default is not None else tx3g_style(0, 0)
    ftab = tx3g_box(b"ftab", struct.pack(">H", len(fonts)),
                    *[struct.pack(">HB", fid, len(name.encode())) + name.encode() for fid, name in fonts])
    return (struct.pack(">Ibb", 0, across, down) + struct.pack(">I", 0x000000FF)
            + struct.pack(">hhhh", 0, 0, 0, 0) + default + ftab)


def tx3g_sample(text, *modifiers):
    raw = text.encode()
    return struct.pack(">H", len(raw)) + raw + b"".join(modifiers)


def styl(*records):
    return tx3g_box(b"styl", struct.pack(">H", len(records)), *records)


MOVTEXT_STYLES = [
    tx3g_sample("plain"),
    tx3g_sample("bold italic under", styl(tx3g_style(0, 4, flags=1), tx3g_style(5, 11, flags=2),
                                          tx3g_style(12, 17, flags=4))),
    tx3g_sample("overlapping ranges", styl(tx3g_style(0, 6, flags=1), tx3g_style(3, 9, flags=2))),
    tx3g_sample("red text", styl(tx3g_style(0, 3, rgba=0xFF0000FF))),
    tx3g_sample("big text", styl(tx3g_style(0, 3, size=32))),
    tx3g_sample("mono text", styl(tx3g_style(0, 4, font=2))),
    tx3g_sample("aaaa  bbbb", styl(tx3g_style(6, 10, flags=1), tx3g_style(0, 4, flags=2))),
    tx3g_sample("short", styl(tx3g_style(0, 100, flags=1))),
    tx3g_sample("backwards", styl(tx3g_style(5, 2, flags=1))),
    tx3g_sample("été 日本 x", styl(tx3g_style(4, 6, flags=1))),
    tx3g_sample("highlight this", tx3g_box(b"hlit", struct.pack(">HH", 0, 4)),
                tx3g_box(b"hclr", struct.pack(">I", 0xFFFF00FF))),
    tx3g_sample("wrapped", tx3g_box(b"twrp", b"\x01")),
    tx3g_sample("one\ntwo"),
    tx3g_sample("three\r\nfour"),
    tx3g_sample("<i>not italic</i> {\\an8} braces & amp"),
    tx3g_sample("all flags", styl(tx3g_style(0, 3, flags=7))),
    tx3g_sample("as default", styl(tx3g_style(0, 2))),
    tx3g_sample("half red", styl(tx3g_style(0, 4, rgba=0xFF000080))),
    tx3g_sample("  leading spaces"),
    tx3g_sample("trailing spaces  "),
    tx3g_sample("all four differ", styl(tx3g_style(0, 3, font=2, flags=1, size=30, rgba=0xFF0000FF))),
    tx3g_sample("adjacent runs here", styl(tx3g_style(0, 8, flags=1), tx3g_style(8, 13, flags=2))),
    tx3g_sample("mid text run here", styl(tx3g_style(4, 8, flags=1))),
    tx3g_sample("unknown font", styl(tx3g_style(0, 7, font=9))),
    tx3g_sample("back\\Nslash and \\h hard"),
    struct.pack(">H", 40) + b"too long",
    tx3g_sample("zero length run", styl(tx3g_style(2, 2, flags=1), tx3g_style(3, 6, flags=2))),
    tx3g_sample("styl too short") + struct.pack(">I4sH", 12, b"styl", 3),
    tx3g_sample("colour then size", styl(tx3g_style(0, 6, rgba=0x00FF00FF, size=20))),
    tx3g_sample("  styled at the start", styl(tx3g_style(0, 4, flags=1))),
]

# A track whose default is Georgia 24, yellow, bold and italic, top left.
MOVTEXT_DEFAULT = [
    tx3g_sample("styled by default"),
    tx3g_sample("plain part then styled", styl(tx3g_style(0, 10, flags=0, size=24, rgba=0xFFFFFFFF))),
    tx3g_sample("small", styl(tx3g_style(0, 5, flags=3, size=12, rgba=0xFFFF00FF))),
]


def tx3g_mp4(path, entry_setup, samples):
    """An MP4 of one 3GPP timed text track: each sample a second and a half
    after the one before, a second long, an empty sample in each gap as
    ffmpeg's muxer writes them."""
    data, durations, at = [], [], 0
    for i, s in enumerate(samples):
        start, end = 1000 + 1500 * i, 2000 + 1500 * i
        if start > at:
            data.append(struct.pack(">H", 0))
            durations.append(start - at)
        data.append(s)
        durations.append(end - start)
        at = end
    total = sum(durations)
    identity = struct.pack(">9i", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)

    def full(kind, version, flags, *parts):
        return tx3g_box(kind, struct.pack(">I", (version << 24) | flags), *parts)

    ftyp = tx3g_box(b"ftyp", b"isom", struct.pack(">I", 0x200), b"isom", b"iso2", b"mp41")
    mdat = tx3g_box(b"mdat", b"".join(data))
    entry = tx3g_box(b"tx3g", b"\0" * 6, struct.pack(">H", 1), entry_setup)
    runs = []
    for d in durations:
        if runs and runs[-1][1] == d:
            runs[-1][0] += 1
        else:
            runs.append([1, d])
    stbl = tx3g_box(
        b"stbl",
        full(b"stsd", 0, 0, struct.pack(">I", 1), entry),
        full(b"stts", 0, 0, struct.pack(">I", len(runs)), *[struct.pack(">II", n, d) for n, d in runs]),
        full(b"stsc", 0, 0, struct.pack(">I", 1), struct.pack(">III", 1, len(data), 1)),
        full(b"stsz", 0, 0, struct.pack(">II", 0, len(data)), *[struct.pack(">I", len(s)) for s in data]),
        full(b"stco", 0, 0, struct.pack(">I", 1), struct.pack(">I", len(ftyp) + 8)),
    )
    dinf = tx3g_box(b"dinf", full(b"dref", 0, 0, struct.pack(">I", 1), full(b"url ", 0, 1)))
    mdia = tx3g_box(
        b"mdia",
        full(b"mdhd", 0, 0, struct.pack(">IIIIHH", 0, 0, 1000, total, 0x55C4, 0)),
        full(b"hdlr", 0, 0, struct.pack(">I4s", 0, b"sbtl"), b"\0" * 12, b"SubtitleHandler\0"),
        tx3g_box(b"minf", full(b"nmhd", 0, 0), dinf, stbl),
    )
    tkhd = full(b"tkhd", 0, 3, struct.pack(">IIIII", 0, 0, 1, 0, total), b"\0" * 8,
                struct.pack(">hhhH", 0, 0, 0, 0), identity, struct.pack(">II", 0, 0))
    mvhd = full(b"mvhd", 0, 0, struct.pack(">IIII", 0, 0, 1000, total), struct.pack(">IH", 0x10000, 0x100),
                b"\0" * 10, identity, b"\0" * 24, struct.pack(">I", 2))
    moov = tx3g_box(b"moov", mvhd, tx3g_box(b"trak", tkhd, mdia))
    with open(path, "wb") as f:
        f.write(ftyp + mdat + moov)


# --- Cues that overlap, for seeking: (start ms, end ms, text) ---
OVERLAP = [
    (1000, 10000, "long, under the next two"),
    (2000, 3000, "short, inside the long"),
    (5000, 6000, "another inside the long"),
    (12000, 13000, "after them all"),
]

# Fixture name -> {cue number: the answer, where it is not ffmpeg's}, each
# checked by hand against libass (ASS, SSA), HTML and libass through ffmpeg's
# own SubRip decoder (SubRip), or the WebVTT specification.
DEPARTURES = {
    "subrip": {
        # ffmpeg's `</font>` closes only the font it opened to put the outer
        # colour back, and leaves `after` green.
        51: '<font color="#ff0000">a<font color="#00ff00">b<font color="#ff0000">c</font></font></font>after',
        # `</b>` closes only the bold: `y` is italic in HTML, and in libass
        # through ffmpeg's SubRip decoder; ffmpeg's encoder loses it.
        52: '<b><i>x</i></b><i>y</i>z',
    },
    "ass": {
        # `\n` is a space at WrapStyle 0; `\h` is a no-break space.
        4: 'Line one\nLine two soft\xa0hard',
        # The style's strike-out is `<s>`; `\i0` leaves the underline on;
        # `{\r}` is the line's own style, not "Default".
        6: '<font face="Georgia" size="24" color="#00ff00"><i><u><s>{\\an5}Leaning style </s></u></i><u><s>not italic</s></u></font><font face="Georgia" size="24" color="#00ff00"><i><u><s>reset</s></u></i></font>',
        # `\b700` is bold.
        13: '<b>weight</b> bare <font color="#ffffff">white</font>',
        # A reset does not move the line (no second alignment); `<s>`.
        17: '<font face="Impact" size="32" color="#ff0000"><b>{\\an8}</b></font>reset to default <font face="Georgia" size="24" color="#00ff00"><i><u><s>to lean</s></u></i></font>',
        # `\i0` leaves the underline on.
        19: '<i>open <u>both </u></i><u>closes i </u>then u',
        # `{\c}` is the style's colour, not the colour before.
        20: '<font color="#00ff00">green <font color="#ff0000">then red </font></font>reset',
        # `{}` is an empty block.
        22: 'no tags here empty block lone backslash',
        # The line's own `\an2` is its alignment, not its style's 8.
        31: '<font face="Impact" size="32" color="#ff0000"><b>{\\an2}explicit bottom in a top style</b></font>',
        # 18.5 rounds to 19.
        33: '<font size="19">fractional size</font>',
        # `{ \i1}` is a block, and `{\i0 }` reads: ffmpeg stops at it.
        35: '<i>space before</i> and after',
        # A block runs from `{` to the first `}`.
        38: '<i>b}nested braces</i>',
        # A drawing is not text.
        39: 'after drawing',
        # A comment is not text.
        40: 'Visible after comment',
        # A colour reads as far as its digits go.
        42: '<font color="#00ff00">no trailing ampersand <font color="#ff0000">short <font color="#ff0000">no ampersand</font></font></font>',
        # `\fs12.5` is 12.5 (13); `\fs-5` is half the size in force.
        43: '<font size="13">fractional override <font size="6">negative</font></font>',
        # A bare `\i` is the style's italic, which this style has.
        47: '<font face="Georgia" size="24" color="#00ff00"><i><u><s>{\\an5}bare in an italic style</s></u></i></font>',
        # `\fs-5`: half the size in force.
        48: '<font size="30">big <font size="15">after minus five</font></font>',
        # `\fs+5`: half as large again.
        49: '<font size="30">big <font size="45">after plus five</font></font>',
        # `\b100` is a light weight, `\b700` bold, `\b400` regular; ffmpeg
        # stops at `\b100` and leaves its `<b>` open into the next cue.
        50: 'light <b>heavy </b>normal',
        # `{\c}` closes the colour, not the size opened after it.
        51: '<font color="#ff0000">red <font size="30">big </font></font><font size="30">after colour reset</font>',
        # `{\rNope}` is the line's own style; `\i0` leaves the underline on.
        52: '<font face="Georgia" size="24" color="#00ff00"><i><u><s>{\\an5}</s></u></i><u><s>x </s></u></font><font face="Georgia" size="24" color="#00ff00"><i><u><s>unknown reset</s></u></i></font>',
        # `\c&H00FF00` without its last `&` still reads.
        53: '<font color="#0000ff">blue <font color="#00ff00">unterminated after</font></font>',
        # A tab is a space.
        54: 'tab here',
        # The line's first `\an` is its alignment, wherever it stands.
        56: '<font face="Impact" size="32" color="#ff0000"><b>{\\an1}middle late alignment</b></font>',
        # `\an0` is the style's alignment, and is the line's first.
        57: 'invalid then valid',
        # VSFilter's `\a4`: top left.
        58: '{\\an7}illegal legacy',
        # `{\fn}` leaves the italic on.
        59: '<font face="Times"><i>a</i></font><i>b</i>',
        # Italic is on or off, not counted.
        60: '<i>twice</i> once off',
        # `\fs0` is the style's size.
        61: 'zero size',
        # `\fn0` is the style's face.
        62: 'face zero',
    },
    "ass_720": {
        # Sizes in 720 lines, scaled to SRT's 288; a style the script lacks,
        # however written, is its Default; `{\r}` is the line's own style.
        1: '<font face="Verdana" size="20" color="#ffff00"><b>Default, fifty lines of 720</b></font>',
        2: '<font face="Verdana" size="20" color="#ffff00"><b>a style the script lacks is its Default</b></font>',
        3: '<font face="Verdana" size="20" color="#ffff00"><b>a style named in lower case</b></font>',
        4: '<font face="Verdana" size="20" color="#ffff00"><b>a starred style name</b></font>',
        5: 'forty lines of 720, sixteen of 288',
        6: '<font size="24">sixty</font> back',
        7: '<font face="Verdana" size="20" color="#ffff00"><b>to default </b></font>back to plain',
        8: "<i>x </i>an unknown reset is the line's own style",
        9: '<font size="24">one and a half times</font>',
        10: '<font face="Verdana" size="20" color="#ffff00"><b></b>not bold <b>the style\'s again</b></font>',
    },
    "webvtt": {
        # `&nbsp;` is U+00A0, `&quot;` a quote; `&lt;` is kept from reading
        # as a tag.
        4: '& <⁠ > x\xa0y ‎l ‏r "q"',
        # A reading in parentheses after its text.
        5: '漢(kan) timed',
        # Kept from reading as SRT markup, without ASS's escapes.
        13: 'brace {⁠\\an8} and backslash \\⁠N here',
        # Numeric references and `&apos;`.
        16: "it's A 'q'",
        # `</b>` with `<i>` innermost is passed over.
        17: '<b><i>xy</i>z</b>',
        # Placed by their settings, as the specification lays them out, in
        # the third of the picture each way their text's anchor falls in.
        18: '{\\an8}top by line number',
        20: '{\\an5}seven lines down: the middle',
        21: '{\\an5}middle by percentage',
        22: '{\\an1}left, as YouTube writes it',
        23: '{\\an3}right aligned',
        24: '{\\an7}top left',
        25: '{\\an9}top right',
        26: '{\\an1}centred a fifth across: left',
    },
}
# Matroska's WebVTT is WebM's cues, so its answers too; and mkvmerge stores
# a cue's text without the spaces around it.
DEPARTURES["webvtt_mkv"] = {**DEPARTURES["webvtt"], 7: 'spaced'}
DEPARTURES["movtext_styles"] = {
    # Timed text is plain text: `<i>` and `{\an8}` in it are characters,
    # kept from reading as SRT markup.
    15: '<⁠i>not italic<⁠/i> {⁠\\an8} braces & amp',
    # And `\N` is not a line break, nor `\h` a hard space.
    25: 'back\\⁠Nslash and \\⁠h hard',
}
DEPARTURES["movtext_default"] = {
    # The run's reset to the default style does not write the place again.
    2: '<font face="Georgia" size="24" color="#ffff00"><b><i>{\\an7}</i></b><font color="#ffffff">plain part</font>'
       '</font><font face="Georgia" size="24" color="#ffff00"><b><i> then styled</i></b></font>',
}


def ms(t):
    h, rest = divmod(t, 3_600_000)
    m, rest = divmod(rest, 60_000)
    s, rest = divmod(rest, 1000)
    return h, m, s, rest


def srt_time(t):
    return "%02d:%02d:%02d,%03d" % ms(t)


def ass_time(t):
    h, m, s, rest = ms(t)
    return "%d:%02d:%02d.%02d" % (h, m, s, rest // 10)


def vtt_time(t):
    return "%02d:%02d:%02d.%03d" % ms(t)


def write(path, text):
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def ffmpeg(*args):
    r = subprocess.run(["ffmpeg", "-v", "error", "-y", *args], capture_output=True, text=True,
                       encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"ffmpeg {' '.join(args)} failed:\n{r.stderr}")


def mkvmerge(out, src):
    # Deterministic: no date, and the same UIDs each run.
    r = subprocess.run([MKVMERGE, "--quiet", "--deterministic", "subtitles", "-o", out, src],
                       capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"mkvmerge {out} {src} failed:\n{r.stdout}{r.stderr}")


def cues_of(srt):
    """An SRT file's cues, each [number, times, text]."""
    if "\r" in srt:
        sys.exit("ffmpeg's SRT has a carriage return: not the file this reads")
    cues = []
    for block in srt.split("\n\n"):
        if block.strip():
            number, times, *text = block.split("\n")
            cues.append([number, times, "\n".join(text)])
    return cues


def make(name, source_ext, source, container, muxer="ffmpeg", answer_from=None, codec="copy"):
    """Mux `source` into NAME.container -- with ffmpeg (`codec` its subtitle
    codec), with mkvmerge, or, for muxer "written", by `source` itself, a
    function writing the file -- then write ffmpeg's SRT of it
    (NAME.ffmpeg.srt) and the answer (NAME.srt): ffmpeg's, its DEPARTURES
    made. A track this ffmpeg cannot read takes its SRT from fixture
    `answer_from`, made already, which holds the same cues."""
    src = os.path.join(HERE, f"{name}.source.{source_ext}")
    out = os.path.join(HERE, f"{name}.{container}")
    if muxer == "written":
        source(out)
    else:
        write(src, source)
        if muxer == "mkvmerge":
            mkvmerge(out, src)
        else:
            ffmpeg("-i", src, "-c:s", codec, "-fflags", "+bitexact", "-flags", "+bitexact", out)
        os.remove(src)
    raw = os.path.join(HERE, f"{name}.ffmpeg.srt")
    if answer_from:
        shutil.copyfile(os.path.join(HERE, f"{answer_from}.ffmpeg.srt"), raw)
    else:
        ffmpeg("-i", out, "-map", "0:s:0", "-c:s", "srt", "-f", "srt", raw)
    with open(raw, encoding="utf-8", newline="") as f:
        cues = cues_of(f.read())
    for number, text in DEPARTURES.get(name, {}).items():
        if not 1 <= number <= len(cues):
            sys.exit(f"{name}: a departure for cue {number}, of {len(cues)}")
        cues[number - 1][2] = text
    answer = os.path.join(HERE, f"{name}.srt")
    write(answer, "".join(f"{n}\n{times}\n{text}\n\n" for n, times, text in cues))
    print("wrote", out, raw, answer)


def main():
    # One cue a second and a half apart, a second long.
    def srt_of(texts):
        return "".join(f"{i + 1}\n{srt_time(1000 + 1500 * i)} --> {srt_time(2000 + 1500 * i)}\n{text}\n\n"
                       for i, text in enumerate(texts))

    make("subrip", "srt", srt_of(SUBRIP), "mkv")
    make("subrip_colours", "srt", srt_of(SUBRIP_COLOURS), "mkv")

    events = "".join(f"Dialogue: 0,{ass_time(1000 + 1500 * i)},{ass_time(2000 + 1500 * i)},{style},,0,0,0,,{text}\n"
                     for i, (style, text) in enumerate(ASS))
    header = ASS_HEADER.replace("\n[Events]", ASS_EXTRA_STYLES + "\n[Events]")
    make("ass", "ass", header + events, "mkv")

    events = "".join(f"Dialogue: 0,{ass_time(1000 + 1500 * i)},{ass_time(2000 + 1500 * i)},{style},,0,0,0,,{text}\n"
                     for i, (style, text) in enumerate(ASS_720))
    make("ass_720", "ass", ASS_720_HEADER + events, "mkv")

    events = "".join(f"Dialogue: Marked=0,{ass_time(1000 + 1500 * i)},{ass_time(2000 + 1500 * i)},{style},,0,0,0,,{text}\n"
                     for i, (style, text) in enumerate(SSA))
    make("ssa", "ssa", SSA_HEADER + events, "mkv", muxer="mkvmerge")

    cues = [("", text) for text in WEBVTT] + WEBVTT_PLACED
    vtt = "WEBVTT\n\n" + "".join(f"{vtt_time(1000 + 1500 * i)} --> {vtt_time(2000 + 1500 * i)}"
                                  f"{' ' + settings if settings else ''}\n{text}\n\n"
                                  for i, (settings, text) in enumerate(cues))
    make("webvtt", "vtt", vtt, "webm")
    make("webvtt_mkv", "vtt", vtt, "mkv", muxer="mkvmerge", answer_from="webvtt")

    overlap = "".join(f"{i + 1}\n{srt_time(start)} --> {srt_time(end)}\n{text}\n\n"
                      for i, (start, end, text) in enumerate(OVERLAP))
    make("overlap", "srt", overlap, "mkv")

    make("movtext", "srt", srt_of(MOVTEXT), "mp4", codec="mov_text")
    two_fonts = tx3g_setup(fonts=((1, "Arial"), (2, "Courier")))
    make("movtext_styles", "", lambda out: tx3g_mp4(out, two_fonts, MOVTEXT_STYLES), "mp4", muxer="written")
    georgia = tx3g_setup(fonts=((1, "Georgia"),), across=0, down=0,
                         default=tx3g_style(0, 0, font=1, flags=3, size=24, rgba=0xFFFF00FF))
    make("movtext_default", "", lambda out: tx3g_mp4(out, georgia, MOVTEXT_DEFAULT), "mp4", muxer="written")
    right_middle = tx3g_setup(across=-1, down=1)
    make("movtext_justified", "", lambda out: tx3g_mp4(out, right_middle, [tx3g_sample("right and middle")]),
         "mp4", muxer="written")


if __name__ == "__main__":
    main()
