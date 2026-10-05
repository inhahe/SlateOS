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

MP4's WebVTT (`wvtt`), which ffmpeg does not read: `webvtt_mp4.mp4` and
`webvtt_overlap*.mp4` are GPAC's MP4Box's, the format's reference
implementation, from the same `.vtt` as a WebM fixture, whose answer is
theirs -- a cue cut into samples wherever another begins or ends, which
the reader joins again, plain, overlapping and in fragments.

Blu-ray's PGS, pictures of text, which ffmpeg cannot write: `pgs*.mkv` are
written here segment by segment (`PgsSet`, `make_pgs`) and muxed by
mkvmerge. Their answer, NAME.states, is a list of pictures: the canvas
FFmpeg's sub2video draws the track on, at each change, as an MD5 of its
RGBA bytes -- with every subtitle shown, and (`forced` lines) with only the
forced ones (ffmpeg's `-forced_subs_only`). For the well-formed fixtures the
generator draws every display set itself, from what it wrote, and stops
unless ffmpeg agrees at every one, colours to the bit; `pgs_cropped`'s crops
are where the crate crops as a Blu-ray player does and ffmpeg does not, and
there the answer is the generator's drawing. `pgs_damage` is display sets
no muxer writes, and its answer is ffmpeg's alone.

DVD's VobSub, pictures too, which ffmpeg writes only from other pictures
(dropping their clears and quantising their colours): `vobsub*.mkv` are
written here SPU by SPU (`VobSpu`, `vob_write`, an MPEG program stream and
its index) and muxed by mkvmerge, their answers made as PGS's are
(`make_vobsub`). `vobsub_dvd`'s departures are where a DVD player shows
otherwise -- a later control sequence's colours, fade or second start taking
effect at its date, a transparent SPU clearing the screen -- and there the
answer is the generator's drawing; `vobsub_damage`'s is ffmpeg's alone.

Run from this directory, on Windows, with gyan.dev's ffmpeg (2026-03-09, git
9b7439c31b) and MKVToolNix's mkvmerge 99.0 on PATH (or in the MKVMERGE
environment variable), and GPAC's MP4Box (26.08-DEV, git a23ae2d) built in
WSL (MP4BOX): `python generate_subtitle_fixtures.py`, or `--webvtt` or
`--dvb` for those fixtures alone.
"""

import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
MKVMERGE = os.environ.get("MKVMERGE", "mkvmerge")
# GPAC's MP4Box, the reference implementation of WebVTT in MP4 (`wvtt`,
# ISO/IEC 14496-30), which ffmpeg does not read: built from source in WSL
# (github.com/gpac/gpac, `./configure --static-bin && make`), there being no
# Windows build that installs without an administrator. A command line run
# under `wsl -d Ubuntu --`.
MP4BOX = os.environ.get("MP4BOX", "~/gpac/bin/gcc/MP4Box")

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

# WebVTT cues overlapping, which WebVTT in MP4 cuts into a sample for every
# stretch between one cue's start or end and the next -- each cue in every
# sample it shows through, a sample saying so where none shows -- for a
# reader to join again: (start, end, identifier, settings, text). Two cues
# beginning together come in the file's order; two alike but for their
# identifier are two, the one after the other.
WEBVTT_OVERLAP = [
    (1000, 10000, "long", "line:0", "long, under all but the last"),
    (2000, 3000, "", "", "short, inside the long"),
    (2000, 4000, "", "align:start position:0%", "begun with the short, ended after it"),
    (5000, 6000, "a", "", "the same text"),
    (6000, 7000, "b", "", "the same text"),
    (12000, 13000, "", "", "after them all, past a stretch of none"),
    # A pair: the first ends while the second still shows, so a reader
    # gives it out with the second still on screen.
    (14000, 16000, "", "", "first of a pair"),
    (15000, 17000, "", "", "second of a pair"),
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
# MP4's WebVTT is the same cues again, split into samples and joined; and
# MP4Box stores a cue's text without the spaces after it -- those before it
# the reader passes over, as in WebM.
DEPARTURES["webvtt_mp4"] = {**DEPARTURES["webvtt"], 7: 'spaced'}
DEPARTURES["webvtt_overlap"] = {
    # Placed by their settings, as in `webvtt`.
    1: '{\\an8}long, under all but the last',
    3: '{\\an1}begun with the short, ended after it',
}
DEPARTURES["webvtt_overlap_mp4"] = DEPARTURES["webvtt_overlap"]
DEPARTURES["webvtt_overlap_fragments"] = DEPARTURES["webvtt_overlap"]
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


def mkvmerge(out, src, *options):
    # Deterministic: no date, and the same UIDs each run.
    r = subprocess.run([MKVMERGE, "--quiet", "--deterministic", "subtitles", *options, "-o", out, src],
                       capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"mkvmerge {out} {src} failed:\n{r.stdout}{r.stderr}")


def mp4box(out, src, *options):
    """`src`, a .vtt or a .ttml, imported by MP4Box as the one track of a new
    MP4, `out`, split into samples as ISO/IEC 14496-30 has them -- with
    `-for-test`, which writes no date and no version, so the same source
    makes the same file. Run in WSL, from a scratch directory whose path
    has no spaces."""
    scratch = tempfile.mkdtemp(prefix="mp4box")
    try:
        source = "in" + os.path.splitext(src)[1]
        shutil.copyfile(src, os.path.join(scratch, source))
        drive, rest = os.path.splitdrive(scratch)
        where = "/mnt/" + drive.rstrip(":").lower() + rest.replace(os.sep, "/")
        command = f"cd '{where}' && {MP4BOX} -for-test -add {source} -new out.mp4 {' '.join(options)}"
        r = subprocess.run(["wsl", "-d", "Ubuntu", "--", "bash", "-c", command],
                           capture_output=True, text=True, encoding="utf-8", errors="replace")
        made = os.path.join(scratch, "out.mp4")
        if r.returncode != 0 or not os.path.exists(made):
            sys.exit(f"MP4Box {out} {src} failed:\n{r.stdout}{r.stderr}")
        shutil.copyfile(made, out)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)


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


def make(name, source_ext, source, container, muxer="ffmpeg", answer_from=None, codec="copy",
         options=()):
    """Mux `source` into NAME.container -- with ffmpeg (`codec` its subtitle
    codec), with mkvmerge, with MP4Box (`options` its own), or, for muxer
    "written", by `source` itself, a function writing the file -- then write
    ffmpeg's SRT of it (NAME.ffmpeg.srt) and the answer (NAME.srt):
    ffmpeg's, its DEPARTURES made. A track this ffmpeg cannot read takes its
    SRT from fixture `answer_from`, made already, which holds the same
    cues."""
    src = os.path.join(HERE, f"{name}.source.{source_ext}")
    out = os.path.join(HERE, f"{name}.{container}")
    if muxer == "written":
        source(out)
    else:
        write(src, source)
        if muxer == "mkvmerge":
            mkvmerge(out, src)
        elif muxer == "mp4box":
            mp4box(out, src, *options)
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


# --- Blu-ray's PGS: pictures of text, written here segment by segment -------
#
# ffmpeg writes no PGS, so these are written here, as the probes that found
# FFmpeg's rules were: a display set a block, each a list of segments. The
# answer, NAME.states, is FFmpeg's: the picture its sub2video shows at each
# change, as an MD5 of the canvas's RGBA bytes. For the well-formed fixtures
# the generator also draws every state itself (`pgs_draw`, from what it
# wrote) and stops unless FFmpeg agrees -- but for the states PGS_DEPARTURES
# names, where the crate crops an object as a Blu-ray player does and FFmpeg
# does not; there the answer is the generator's drawing.

PGS_PDS, PGS_ODS, PGS_PCS, PGS_WDS, PGS_END = 0x14, 0x15, 0x16, 0x17, 0x80


def pgs_segment(kind, ms_time, body):
    """A .sup segment: "PG", the time at 90 kHz twice (PTS, DTS 0), the type,
    the length, the body."""
    return b"PG" + struct.pack(">IIBH", ms_time * 90, 0, kind, len(body)) + body


def pgs_pcs(canvas, number, state, palette, objects):
    """objects: (id, x, y, forced, crop or None)."""
    body = struct.pack(">HHBHBBBB", *canvas, 0x10, number, state, 0, palette, len(objects))
    for oid, x, y, forced, crop in objects:
        body += struct.pack(">HBBHH", oid, 0, (0x80 if crop else 0) | (0x40 if forced else 0), x, y)
        if crop:
            body += struct.pack(">HHHH", *crop)
    return body


def pgs_rle(rows):
    """Rows of palette indexes, coded as PGS codes them: the shortest code
    for each run."""
    out = bytearray()
    for row in rows:
        i = 0
        while i < len(row):
            c, n = row[i], 1
            while i + n < len(row) and row[i + n] == c and n < 16383:
                n += 1
            if c == 0:
                out += bytes([0, n]) if n < 64 else bytes([0, 0x40 | n >> 8, n & 0xFF])
            elif n < 3:
                out += bytes([c]) * n
            elif n < 64:
                out += bytes([0, 0x80 | n, c])
            else:
                out += bytes([0, 0xC0 | n >> 8, n & 0xFF, c])
            i += n
        out += b"\x00\x00"
    return bytes(out)


def pgs_ods(oid, rows, pieces=1):
    """An object's segments' bodies: whole, or its codes cut into `pieces`."""
    codes = pgs_rle(rows)
    head = struct.pack(">HB", oid, 0)
    size = (len(codes) + 4).to_bytes(3, "big") + struct.pack(">HH", len(rows[0]), len(rows))
    cuts = [len(codes) * k // pieces for k in range(pieces + 1)]
    bodies = []
    for k in range(pieces):
        flags = (0x80 if k == 0 else 0) | (0x40 if k == pieces - 1 else 0)
        bodies.append(head + bytes([flags]) + (size if k == 0 else b"") + codes[cuts[k]:cuts[k + 1]])
    return bodies


class PgsSet:
    """One display set: at `ms`, composing `objects` (id, x, y, forced, crop)
    in `palette`, after defining `palettes` ((id, [(entry, Y, Cr, Cb, A)]))
    and `shapes` ((id, rows, pieces)). `raw` replaces the segments' bodies
    with these (type, body) pairs, for damage the writer would not make."""

    def __init__(self, ms_time, state=0x80, objects=(), palette=0, palettes=(), shapes=(),
                 raw=None):
        self.ms, self.state, self.objects, self.palette = ms_time, state, list(objects), palette
        self.palettes, self.shapes, self.raw = list(palettes), list(shapes), raw

    def segments(self, canvas, number):
        if self.raw is not None:
            return b"".join(pgs_segment(kind, self.ms, body) for kind, body in self.raw)
        out = pgs_segment(PGS_PCS, self.ms, pgs_pcs(canvas, number, self.state, self.palette, self.objects))
        windows = [(i, x, y, 1, 1) for i, (_, x, y, _, _) in enumerate(self.objects[:2])]
        out += pgs_segment(PGS_WDS, self.ms, bytes([len(windows)]) + b"".join(
            struct.pack(">BHHHH", *w) for w in windows))
        for pid, entries in self.palettes:
            out += pgs_segment(PGS_PDS, self.ms, bytes([pid, 0]) + b"".join(bytes(e) for e in entries))
        for oid, rows, pieces in self.shapes:
            for body in pgs_ods(oid, rows, pieces):
                out += pgs_segment(PGS_ODS, self.ms, body)
        return out + pgs_segment(PGS_END, self.ms, b"")


def pgs_colour(y, cr, cb, sd):
    """FFmpeg's conversion: BT.709's or (sd) BT.601's matrix on the studio
    range, in 10-bit fixed point (found by probing; design-decisions §1362)."""
    def fix(x):
        return int(x * 1024 + 0.5)
    kr, gb, gr, kb = (1.402, 0.34414, 0.71414, 1.772) if sd else (1.5747, 0.1873, 0.4682, 1.8556)
    cb, cr = cb - 128, cr - 128
    yy = (y - 16) * fix(255 / 219)
    return tuple(max(0, min(255, (yy + v + 512) >> 10)) for v in (
        fix(kr * 255 / 224) * cr,
        -fix(gb * 255 / 224) * cb - fix(gr * 255 / 224) * cr,
        fix(kb * 255 / 224) * cb))


def pgs_draw(sets, canvas):
    """Each display set's picture as the crate shows it -- for the sets the
    generator wrote whole: [(ms, md5)] for those that change it."""
    w, h = canvas
    sd = 0 < h <= 576
    objects, palettes, states = {}, {}, []
    for s in sets:
        if s.state & 0xC0:
            objects.clear()
            palettes.clear()
        for pid, entries in s.palettes:
            p = palettes.setdefault(pid, [(0, 0, 0, 0)] * 256)
            for e, y, cr, cb, a in entries:
                p[e] = (*pgs_colour(y, cr, cb, sd), a)
        for oid, rows, _ in s.shapes:
            objects[oid] = rows
        if s.objects and s.palette not in palettes:
            continue
        frame = bytearray(w * h * 4)
        for oid, x, y, _, crop in s.objects[:2]:
            rows = objects.get(oid)
            if rows is None:
                continue
            left, top, cw, ch = crop or (0, 0, len(rows[0]), len(rows))
            for r in range(top, min(top + ch, len(rows))):
                for c in range(left, min(left + cw, len(rows[0]))):
                    at = ((y + r - top) * w + x + c - left) * 4
                    frame[at:at + 4] = bytes(palettes[s.palette][rows[r][c]])
        states.append((s.ms, hashlib.md5(frame).hexdigest()))
    return states


def dedup(states):
    """[(ms, md5)] with each repeat of the picture before it dropped, and of
    pictures at one time the last alone -- what shows."""
    out = []
    for t, md5 in states:
        if out and out[-1][0] == t:
            out.pop()
        if not out or out[-1][1] != md5:
            out.append((t, md5))
    return out


def sub2video_states(mkv, canvas, forced=False, extra=()):
    """FFmpeg's picture at each change -- of the forced subtitles alone with
    `forced` -- as [(ms, md5)], the blank it shows before the first
    subtitle's dropped. `extra`: the decoder's options."""
    r = subprocess.run(
        ["ffmpeg", "-v", "error", *(["-forced_subs_only", "1"] if forced else []), *extra, "-copyts",
         "-i", mkv, "-filter_complex", "[0:s:0]format=rgba[v]", "-map", "[v]", "-fps_mode",
         "passthrough", "-copyts", "-f", "framemd5", "-"],
        capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        sys.exit(f"sub2video of {mkv} failed:\n{r.stderr}")
    blank = hashlib.md5(bytes(canvas[0] * canvas[1] * 4)).hexdigest()
    states = []
    for line in r.stdout.splitlines():
        if line.startswith("#"):
            continue
        _, _, pts, _, _, md5 = (f.strip() for f in line.split(","))
        t = int(pts)
        if t % 1000:
            continue  # the frame sub2video shows a microsecond before a change
        if states and states[-1][0] == t // 1000:
            states[-1] = (t // 1000, md5)  # the last frame at a time is what shows
        elif not states or states[-1][1] != md5:
            states.append((t // 1000, md5))
    while states and states[0][1] == blank:
        states.pop(0)
    # sub2video's last frame, at a time past every subtitle's end.
    if states and states[-1][0] > 4_000_000:
        states.pop()
    return dedup(states)


def shown_at(states, t):
    """The picture `states` shows at `t` ms: the last change at or before."""
    md5 = None
    for st, m in states:
        if st <= t:
            md5 = m
    return md5


def make_pgs(name, canvas, sets, departures=(), drawn=True):
    """Write NAME.mkv of `sets`, and NAME.states: FFmpeg's pictures, all of
    them and the forced alone. With `drawn` they are checked against the
    generator's drawing of every display set, and the sets at the times in
    `departures` show the drawing's instead."""
    sup = os.path.join(HERE, f"{name}.source.sup")
    out = os.path.join(HERE, f"{name}.mkv")
    with open(sup, "wb") as f:
        for n, s in enumerate(sets):
            f.write(s.segments(canvas, n))
    mkvmerge(out, sup)
    os.remove(sup)
    states = sub2video_states(out, canvas)
    forced = sub2video_states(out, canvas, forced=True)
    if drawn:
        drawing = pgs_draw(sets, canvas)
        # Of sets at one time, the last is what shows.
        for t, md5 in dict(drawing).items():
            if t not in departures and shown_at(states, t) != md5:
                sys.exit(f"{name}: at {t} ms FFmpeg shows {shown_at(states, t)}, the drawing {md5}")
        times = {t for t, _ in drawing}
        for t, _ in states:
            if t not in times:
                sys.exit(f"{name}: FFmpeg's picture changes at {t} ms, where nothing was written")
        blank = hashlib.md5(bytes(canvas[0] * canvas[1] * 4)).hexdigest()
        states = dedup(drawing)
        while states and states[0][1] == blank:
            states.pop(0)
        if departures and any(s.objects and any(o[3] for o in s.objects) for s in sets):
            sys.exit(f"{name}: forced subtitles and departures in one fixture: the forced"
                     " answer would be FFmpeg's, uncropped")
    answer = os.path.join(HERE, f"{name}.states")
    write(answer, f"# {name}.mkv: the picture at each change, as an MD5 of the RGBA canvas;"
                  f" `forced` lines with only forced subtitles shown"
                  f" (generate_subtitle_fixtures.py)\ncanvas {canvas[0]} {canvas[1]}\n"
                  + "".join(f"state {t} {md5}\n" for t, md5 in states)
                  + "".join(f"forced {t} {md5}\n" for t, md5 in forced))
    print("wrote", out, answer)


PGS_CANVAS = (1280, 720)
# White, black, a half-transparent red, a green at 200.
PGS_PALETTE = [(1, 235, 128, 128, 255), (2, 16, 128, 128, 255), (3, 81, 240, 90, 128),
               (4, 145, 34, 54, 200)]
# Entries 1 and 2 again, blue and yellow: 3 and 4 kept.
PGS_UPDATE = [(1, 41, 110, 240, 255), (2, 210, 146, 16, 255)]
# Alpha from 0 (its colour kept, unseen) to 255.
PGS_ALPHAS = [(1, 235, 128, 128, 0), (2, 81, 240, 90, 1), (3, 145, 34, 54, 127),
              (4, 41, 110, 240, 254), (5, 210, 146, 16, 255)]
# Colours across the cube, for the 601 matrix.
PGS_SD_PALETTE = [(i, 16 + 27 * i, 255 - 20 * i, 30 * i % 256, 255) for i in range(1, 9)]


def pgs_shape(w, h, k):
    """A picture like a line of text: strokes of 1, outlined in 2, on 0."""
    rows = []
    for r in range(h):
        row = []
        for c in range(w):
            ink = (c // 7 + k) % 3 != 0 and 3 <= r % 12 < 10 and c % 7 < 5
            edge = (c // 7 + k) % 3 != 0 and (r % 12 in (2, 10) or c % 7 == 5) and 2 <= r % 12 <= 10
            row.append(1 if ink else 2 if edge else 0)
        rows.append(row)
    return rows


def pgs_scenes():
    line = pgs_shape(300, 40, 0)
    line2 = pgs_shape(200, 30, 1)
    # Runs of 64 and more: the long codes.
    wide = [[1] * 1000 + [0] * 100 + [3] * 70 for _ in range(10)]
    alphas = [[1, 2, 3, 4, 5] * 8 for _ in range(8)]
    pal = [(0, PGS_PALETTE)]
    shown = [
        # A line, then cleared.
        PgsSet(1000, objects=[(0, 490, 620, False, None)], palettes=pal, shapes=[(0, line, 1)]),
        PgsSet(2000, state=0),
        # Two objects at once.
        PgsSet(3000, objects=[(0, 100, 50, False, None), (1, 700, 600, False, None)], palettes=pal,
               shapes=[(0, line, 1), (1, line2, 1)]),
        PgsSet(4000, state=0),
        # Within an epoch: shown, its palette changed, moved, its picture
        # replaced -- each set naming what the epoch holds.
        PgsSet(5000, objects=[(0, 200, 300, False, None)], palettes=pal, shapes=[(0, line, 1)]),
        PgsSet(6000, state=0, objects=[(0, 200, 300, False, None)], palettes=[(0, PGS_UPDATE)]),
        PgsSet(7000, state=0, objects=[(0, 400, 350, False, None)]),
        PgsSet(8000, state=0, objects=[(0, 400, 350, False, None)], shapes=[(0, line2, 1)]),
        PgsSet(9000, state=0),
        # An object in three segments.
        PgsSet(10000, objects=[(3, 10, 10, False, None)], palettes=pal, shapes=[(3, line, 3)]),
        PgsSet(11000, state=0),
        # Forced; then replaced by the next epoch with no clear between.
        PgsSet(12000, objects=[(0, 300, 500, True, None)], palettes=pal, shapes=[(0, line2, 1)]),
        PgsSet(13000, objects=[(1, 50, 650, False, None)], palettes=pal, shapes=[(1, wide, 1)]),
        PgsSet(14000, state=0),
        # Every alpha.
        PgsSet(15000, objects=[(0, 0, 0, False, None)], palettes=[(0, PGS_ALPHAS)], shapes=[(0, alphas, 1)]),
        PgsSet(16000, state=0),
        # An acquisition point bringing what it shows; a palette numbered 5.
        PgsSet(17000, state=0x40, objects=[(0, 640, 360, False, None)], palettes=pal, shapes=[(0, line, 1)]),
        PgsSet(18000, state=0),
        PgsSet(19000, objects=[(0, 640, 360, True, None)], palette=5, palettes=[(5, PGS_PALETTE)],
               shapes=[(0, line, 1)]),
        PgsSet(20000, state=0),
        # Two sets at one time: the first never seen. The second is never
        # cleared, and shows until the film ends.
        PgsSet(21000, objects=[(0, 100, 100, False, None)], palettes=pal, shapes=[(0, line2, 1)]),
        PgsSet(21000, objects=[(0, 500, 100, False, None)], palettes=pal, shapes=[(0, line, 1)]),
    ]
    sd = [
        PgsSet(1000, objects=[(0, 20, 400, False, None)], palettes=[(0, PGS_SD_PALETTE)],
               shapes=[(0, [[(c // 8) % 9 for c in range(72)] for _ in range(16)], 1)]),
        PgsSet(2000, state=0),
    ]
    cropped = [
        # A crop of the line; one reaching past it, cut to it; the whole
        # again; one wholly outside it, showing nothing.
        PgsSet(1000, objects=[(0, 100, 100, False, (10, 5, 100, 20))], palettes=pal, shapes=[(0, line, 1)]),
        PgsSet(2000, state=0, objects=[(0, 100, 100, False, (250, 30, 200, 200))]),
        PgsSet(3000, state=0, objects=[(0, 100, 100, False, None)]),
        PgsSet(4000, state=0, objects=[(0, 100, 100, False, (400, 0, 10, 10))]),
        PgsSet(5000, state=0),
    ]
    return shown, sd, cropped


def pgs_damage():
    """Display sets a muxer would not write, each as FFmpeg's rules take it:
    the answer is FFmpeg's pictures alone."""
    tile = pgs_shape(40, 24, 0)
    pal = [(0, PGS_PALETTE)]
    end = (PGS_END, b"")

    def palette(pid):
        return (PGS_PDS, bytes([pid, 0]) + b"".join(bytes(e) for e in PGS_PALETTE))

    def obj(oid, rows):
        return (PGS_ODS, pgs_ods(oid, rows)[0])

    def comp(state, palette_id, objects):
        return (PGS_PCS, pgs_pcs(PGS_CANVAS, 0, state, palette_id, objects))

    def raw(ms_time, *segments):
        return PgsSet(ms_time, raw=list(segments))

    def show(ms_time, x, y):
        return PgsSet(ms_time, objects=[(0, x, y, False, None)], palettes=pal, shapes=[(0, tile, 1)])

    # Codes a line short of the height they declare.
    codes = pgs_rle(tile[:-1])
    short = (PGS_ODS, struct.pack(">HBB", 0, 0, 0xC0) + (len(codes) + 4).to_bytes(3, "big")
             + struct.pack(">HH", 40, 24) + codes)
    # Lines a pixel longer than the 60 they declare: each line's extra pixel
    # pushes the next along, and the last line's run of 2s, which would pass
    # the end, is skipped -- so the codes still fill the object.
    codes = pgs_rle([[1] * 30 + [2] * 30 + [1] for _ in range(30)])
    long = (PGS_ODS, struct.pack(">HBB", 0, 0, 0xC0) + (len(codes) + 4).to_bytes(3, "big")
            + struct.pack(">HH", 60, 30) + codes)
    return [
        # A first set naming an object and a palette no set has defined:
        # nothing shows. (A reader that read on to the last epoch, and seeks
        # back here, must have forgotten that epoch's.)
        raw(500, comp(0x00, 0, [(0, 600, 100, False, None)]), end),
        # A palette never defined: the picture shown stays.
        show(1000, 100, 100),
        raw(2000, comp(0x80, 7, [(0, 200, 100, False, None)]), palette(0), obj(0, tile), end),
        PgsSet(3000, state=0),
        # Three objects: the first two shown.
        PgsSet(4000, objects=[(i, 100 + 60 * i, 200, False, None) for i in range(3)], palettes=pal,
               shapes=[(i, tile, 1) for i in range(3)]),
        PgsSet(5000, state=0),
        # Ten palettes: the ninth and tenth not held, the eighth shown.
        raw(6000, comp(0x80, 9, [(0, 100, 300, False, None)]), *[palette(p) for p in range(10)],
            obj(0, tile), end),
        raw(7000, comp(0x00, 7, [(0, 100, 300, False, None)]), end),
        PgsSet(8000, state=0),
        # Sixty-five objects: the last not held, and object 3 replaced.
        raw(9000, comp(0x80, 0, [(64, 0, 400, False, None), (3, 100, 400, False, None)]), palette(0),
            *[obj(i, tile) for i in range(65)], obj(3, pgs_shape(80, 12, 2)), end),
        PgsSet(10000, state=0),
        # Wider than the canvas: not shown.
        PgsSet(11000, objects=[(0, 0, 500, False, None)], palettes=pal, shapes=[(0, [[1] * 1300] * 4, 1)]),
        # Codes a line short: not shown, and the object they replace goes too.
        show(13000, 300, 300),
        raw(14000, comp(0x00, 0, [(0, 300, 300, False, None)]), short, end),
        # Lines longer than the width: the codes run on into the next line.
        raw(15000, comp(0x80, 0, [(0, 500, 300, False, None)]), palette(0), long, end),
        PgsSet(16000, state=0),
        # An epoch start naming the last epoch's object: gone.
        show(17000, 100, 600),
        raw(18000, comp(0x80, 0, [(0, 200, 600, False, None)]), palette(0), end),
        # An acquisition point bringing nothing, then a normal set bringing
        # nothing: neither has a palette, and the picture shown stays.
        show(19000, 100, 600),
        raw(20000, comp(0x40, 0, [(0, 200, 600, False, None)]), end),
        raw(21000, comp(0x00, 0, [(0, 300, 600, False, None)]), end),
        PgsSet(22000, state=0),
        # A composition cut short clears.
        show(23000, 100, 600),
        raw(24000, (PGS_PCS, pgs_pcs(PGS_CANVAS, 0, 0, 0, [])[:5]), end),
        # A state of neither top bit keeps the epoch.
        show(25000, 100, 600),
        raw(26000, comp(0x20, 0, [(0, 400, 600, False, None)]), end),
        PgsSet(27000, state=0),
    ]


# --- DVD's VobSub: pictures of text, written here SPU by SPU ----------------
#
# ffmpeg writes VobSub only from other pictures, and drops their clears and
# quantises their colours on the way, so these are written here as the
# probes that found FFmpeg's rules were: each subtitle an SPU of two-bit
# run-length fields and control sequences (`VobSpu`), in an MPEG program
# stream (`.sub`) with its index (`.idx`), muxed by mkvmerge. The answer is
# FFmpeg's pictures, as for PGS (`make_pgs`); for the well-formed fixtures
# the generator draws every change itself and stops unless FFmpeg agrees,
# but for the times VOB departures name -- where a DVD player shows
# otherwise -- where the answer is the drawing.

VOB_FORCED_START, VOB_START, VOB_STOP = 0x00, 0x01, 0x02
VOB_COLOURS, VOB_ALPHAS, VOB_AREA, VOB_FIELDS, VOB_END = 0x03, 0x04, 0x05, 0x06, 0xFF


def vob_field(rows):
    """Rows of two-bit colours, each run in the fewest nibbles, each line
    ending on a byte; a run reaching the line's end, if longer than 63, as
    the fill-to-the-end code."""
    nib = []
    for row in rows:
        i = 0
        while i < len(row):
            c, n = row[i], 1
            while i + n < len(row) and row[i + n] == c and n < 255:
                n += 1
            if i + n == len(row) and n > 63:
                nib += [0, 0, 0, c]
            else:
                v = (n << 2) | c
                count = 1 if n < 4 else 2 if n < 16 else 3 if n < 64 else 4
                nib += [(v >> (4 * k)) & 0xF for k in range(count - 1, -1, -1)]
            i += n
        if len(nib) % 2:
            nib.append(0)
    return bytes((nib[k] << 4) | nib[k + 1] for k in range(0, len(nib), 2))


class VobSpu:
    """One SPU: `rows` placed at (x, y) with `colours` and `alphas`, and its
    control sequences, each (date, [commands]): a command is "place" (all of
    the above), "start", "forced", "stop", ("colours", c) or ("alphas", a)."""

    def __init__(self, ms_time, rows, x, y, sequences, colours=(0, 1, 2, 3), alphas=(0, 15, 15, 15)):
        self.ms, self.rows, self.x, self.y = ms_time, rows, x, y
        self.sequences, self.colours, self.alphas = sequences, colours, alphas

    def fields(self):
        return vob_field(self.rows[0::2]), vob_field(self.rows[1::2])

    def bytes(self):
        top, bottom = self.fields()
        data = top + bottom
        h, w = len(self.rows), len(self.rows[0])
        x2, y2 = self.x + w - 1, self.y + h - 1

        def four(v):
            return bytes([(v[3] << 4) | v[2], (v[1] << 4) | v[0]])

        def command(c):
            if c == "place":
                return (bytes([VOB_COLOURS]) + four(self.colours) + bytes([VOB_ALPHAS]) + four(self.alphas)
                        + bytes([VOB_AREA, self.x >> 4, ((self.x & 0xF) << 4) | (x2 >> 8), x2 & 0xFF,
                                 self.y >> 4, ((self.y & 0xF) << 4) | (y2 >> 8), y2 & 0xFF, VOB_FIELDS])
                        + struct.pack(">HH", 4, 4 + len(top)))
            if c == "start":
                return bytes([VOB_START])
            if c == "forced":
                return bytes([VOB_FORCED_START])
            if c == "stop":
                return bytes([VOB_STOP])
            kind, value = c
            return bytes([VOB_COLOURS if kind == "colours" else VOB_ALPHAS]) + four(value)

        bodies = [b"".join(command(c) for c in cmds) + bytes([VOB_END]) for _, cmds in self.sequences]
        first = 4 + len(data)
        offsets, at = [], first
        for b in bodies:
            offsets.append(at)
            at += 4 + len(b)
        out = struct.pack(">HH", at, first) + data
        for k, ((date, _), body) in enumerate(zip(self.sequences, bodies)):
            nxt = offsets[k + 1] if k + 1 < len(self.sequences) else offsets[k]
            out += struct.pack(">HH", date, nxt) + body
        return out


def vob_pack_header(scr):
    """An MPEG-2 pack header, its clock from a 90 kHz count."""
    return bytes([0, 0, 1, 0xBA, 0x44 | ((scr >> 27) & 0x38) | ((scr >> 28) & 0x03), (scr >> 20) & 0xFF,
                  0x04 | ((scr >> 12) & 0xF8) | ((scr >> 13) & 0x03), (scr >> 5) & 0xFF,
                  0x04 | ((scr << 3) & 0xF8), 0x01, 0x01, 0x89, 0xC3, 0xF8])


def vob_packs(spu, ms_time):
    """An SPU in 2048-byte packs: PES private stream 1, sub-stream 0x20, its
    first packet timed."""
    pts = ms_time * 90
    out, rest, first = b"", spu, True
    while rest:
        head = vob_pack_header(pts)
        room = 2048 - len(head) - 6 - 3 - (5 if first else 0) - 1
        chunk, rest = rest[:room], rest[room:]
        stamp = bytes([0x21 | ((pts >> 29) & 0x0E), (pts >> 22) & 0xFF, 0x01 | ((pts >> 14) & 0xFE),
                       (pts >> 7) & 0xFF, 0x01 | ((pts << 1) & 0xFE)]) if first else b""
        payload = bytes([0x81, 0x80 if first else 0, len(stamp)]) + stamp + bytes([0x20]) + chunk
        pack = head + b"\x00\x00\x01\xbd" + struct.pack(">H", len(payload)) + payload
        pad = 2048 - len(pack)
        if pad >= 6:
            pack += b"\x00\x00\x01\xbe" + struct.pack(">H", pad - 6) + b"\xff" * (pad - 6)
        else:
            pack += b"\xff" * pad
        out += pack
        first = False
    return out


def vob_write(base, canvas, palette, spus, raw=()):
    """base.idx and base.sub: `palette` sixteen RGB ints or None, `canvas`
    (w, h) or None; `raw` (ms, bytes) SPUs written as they are."""
    sub, lines = b"", []
    items = [(s.ms, s.bytes()) for s in spus] + list(raw)
    for ms_time, data in sorted(items, key=lambda i: i[0]):
        lines.append("timestamp: %02d:%02d:%02d:%03d, filepos: %09x" % (
            ms_time // 3_600_000, ms_time // 60_000 % 60, ms_time // 1000 % 60, ms_time % 1000, len(sub)))
        sub += vob_packs(data, ms_time)
    idx = "# VobSub index file, v7 (do not modify this line!)\n"
    if canvas:
        idx += f"size: {canvas[0]}x{canvas[1]}\n"
    if palette:
        idx += "palette: " + ", ".join(f"{c:06x}" for c in palette) + "\n"
    idx += "\nid: en, index: 0\n" + "\n".join(lines) + "\n"
    with open(base + ".idx", "w", encoding="ascii", newline="\n") as f:
        f.write(idx)
    with open(base + ".sub", "wb") as f:
        f.write(sub)


def vob_rgba(palette, colours, alphas):
    """The four colours as FFmpeg makes them: from the palette, or greys."""
    if palette:
        return [((palette[colours[i]] >> 16) & 0xFF, (palette[colours[i]] >> 8) & 0xFF,
                 palette[colours[i]] & 0xFF, alphas[i] * 17) for i in range(4)]
    given = []
    for i in range(4):
        if alphas[i] and colours[i] not in given:
            given.append(colours[i])
    ramp = {1: [0xFF], 2: [0, 0xFF], 3: [0, 0x80, 0xFF]}.get(len(given), [0, 0x55, 0xAA, 0xFF])
    out = []
    for i in range(4):
        if not alphas[i]:
            out.append((0, 0, 0, 0))
        else:
            g = ramp[given.index(colours[i])] * 255 >> 8
            out.append((g, g, g, alphas[i] * 17))
    return out


def vob_changes(spu, palette, duration_ms):
    """An SPU's changes, (ms after its time, picture or None for a clear),
    as the crate makes them: each sequence at its own date."""
    colours, alphas, placed = None, None, False
    seqs = []
    for date, cmds in spu.sequences:
        start = forced = stop = False
        for c in cmds:
            if c == "place":
                colours, alphas, placed = list(spu.colours), list(spu.alphas), True
            elif c in ("start", "forced"):
                start, forced = True, c == "forced"
            elif c == "stop":
                stop = True
            elif c[0] == "colours":
                colours = list(c[1])
            else:
                alphas = list(c[1])
        seqs.append([date * 1024 // 90, start, forced, stop, (tuple(colours or ()), tuple(alphas or ()), placed)])
    if not any(s[1] for s in seqs):
        for s in seqs:
            if s[4][2]:
                s[0], s[1] = 0, True
                break
    shown, out = None, []
    for ms_after, start, forced, stop, (c, a, placed) in seqs:
        now = None if stop else (c, a, forced) if start else ((c, a, shown[2]) if shown else None)
        if now == shown and not start:
            continue
        out.append((max(ms_after, out[-1][0] if out else 0), now))
        shown = now
    if shown is not None and duration_ms is not None and duration_ms > (out[-1][0] if out else 0):
        out.append((duration_ms, None))
    return out


def vob_frame(spu, palette, canvas, picture):
    """The canvas with an SPU's picture drawn as sub2video draws it: the
    transparent edges cut away -- but of a forced picture, which FFmpeg
    keeps whole -- a transparent inside kept."""
    w, h = canvas
    frame = bytearray(w * h * 4)
    if picture is None:
        return frame
    colours, alphas, forced = picture
    rgba = vob_rgba(palette, colours, alphas)
    rows = spu.rows
    if forced:
        seen = [(0, 0), (len(rows) - 1, len(rows[0]) - 1)]
    else:
        seen = [(r, c) for r, row in enumerate(rows) for c, v in enumerate(row) if rgba[v][3]]
    if not seen:
        return frame
    top, bottom = min(r for r, _ in seen), max(r for r, _ in seen)
    left, right = min(c for _, c in seen), max(c for _, c in seen)
    for r in range(top, bottom + 1):
        for c in range(left, right + 1):
            at = ((spu.y + r) * w + spu.x + c) * 4
            frame[at:at + 4] = bytes(rgba[rows[r][c]])
    return frame


def vob_draw(spus, palette, canvas, durations):
    """Every change, as the crate shows it: [(ms, md5)], a later SPU's first
    change superseding what an earlier one had still to change."""
    events = []
    for spu in spus:
        changes = [(spu.ms + at, spu, picture) for at, picture in vob_changes(spu, palette, durations.get(spu.ms))]
        if not changes:
            continue
        first = changes[0][0]
        events = [e for e in events if e[0] < first] + changes
    return [(t, hashlib.md5(vob_frame(s, palette, canvas, p)).hexdigest()) for t, s, p in events]


def mkv_durations(mkv):
    """Each block's duration as mkvmerge wrote it, by its time in ms."""
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "packet=pts_time,duration_time",
                        "-of", "csv=p=0", mkv], capture_output=True, text=True, encoding="utf-8")
    out = {}
    for line in r.stdout.split():
        t, d = line.split(",")
        out[round(float(t) * 1000)] = round(float(d) * 1000) if d not in ("N/A", "") else None
    return out


def make_vobsub(name, canvas, palette, spus, departures=(), drawn=True, raw=()):
    """Write NAME.mkv of `spus` (and `raw` SPUs), and NAME.states: FFmpeg's
    pictures, all and the forced alone; with `drawn`, each checked against
    the generator's drawing, the times in `departures` taking the drawing's."""
    base = os.path.join(HERE, f"{name}.source")
    out = os.path.join(HERE, f"{name}.mkv")
    vob_write(base, canvas, palette, spus, raw)
    # A cluster a block: a seek lands on the block it names, not on a
    # cluster that begins earlier -- whose subtitles a demuxer gives from
    # its start, as FFmpeg's does -- so that the subpicture before must be
    # gone back for (`a_seek_in_dvd_pictures_finds_one_still_showing`).
    mkvmerge(out, base + ".idx", "--cluster-length", "1")
    os.remove(base + ".idx")
    os.remove(base + ".sub")
    shown_canvas = canvas or (720, 576)
    states = sub2video_states(out, shown_canvas)
    forced = sub2video_states(out, shown_canvas, forced=True)
    if drawn:
        drawing = vob_draw(spus, palette, shown_canvas, mkv_durations(out))
        # Departures are times where the two differ either way: FFmpeg
        # showing something else, or changing where the crate does not.
        for t, md5 in dict(drawing).items():
            if t not in departures and shown_at(states, t) != md5:
                sys.exit(f"{name}: at {t} ms FFmpeg shows {shown_at(states, t)}, the drawing {md5}")
        times = {t for t, _ in drawing}
        for t, _ in states:
            if t not in times and t not in departures:
                sys.exit(f"{name}: FFmpeg's picture changes at {t} ms, where nothing was written")
        if departures and any("forced" in cmds for s in spus for _, cmds in s.sequences):
            sys.exit(f"{name}: forced subtitles and departures in one fixture: the forced"
                     " answer would be FFmpeg's")
        blank = hashlib.md5(bytes(shown_canvas[0] * shown_canvas[1] * 4)).hexdigest()
        states = dedup(drawing)
        while states and states[0][1] == blank:
            states.pop(0)
    answer = os.path.join(HERE, f"{name}.states")
    write(answer, f"# {name}.mkv: the picture at each change, as an MD5 of the RGBA canvas;"
                  f" `forced` lines with only forced subtitles shown"
                  f" (generate_subtitle_fixtures.py)\ncanvas {shown_canvas[0]} {shown_canvas[1]}\n"
                  + "".join(f"state {t} {md5}\n" for t, md5 in states)
                  + "".join(f"forced {t} {md5}\n" for t, md5 in forced))
    print("wrote", out, answer)


VOB_CANVAS = (720, 480)
# Colour 0 a dark grey: transparent, but not black, so that a picture's
# transparent edges cut away and its transparent inside kept show.
VOB_PALETTE = [0x404040, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0x808080, 0x00FFFF,
               0xFF00FF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0]


def vob_shape(w, h, k):
    """A picture like a line of text: strokes of 1 outlined in 2, on 0."""
    rows = []
    for r in range(h):
        row = []
        for c in range(w):
            on = (c // 6 + k) % 3 != 0
            ink = on and 3 <= r % 12 < 9 and c % 6 < 4
            edge = on and (r % 12 in (2, 9) or c % 6 == 4) and 2 <= r % 12 <= 9
            row.append(1 if ink else 2 if edge else 0)
        rows.append(row)
    return rows


def vob_scenes():
    line = vob_shape(240, 24, 0)
    two = vob_shape(300, 36, 1)
    # A box inside a two-pixel border of colour 0, a hole of 0 inside it.
    framed = [[0] * 44 for _ in range(24)]
    for r in range(2, 22):
        for c in range(2, 42):
            framed[r][c] = 0 if 10 <= r < 14 and 18 <= c < 26 else 2 if r in (2, 21) or c in (2, 41) else 1
    # An odd width, and runs to the line's end longer than 63.
    wide = [[1] * 3 + [2] * 70 for _ in range(5)]
    start_stop = [(0, ["place", "start"]), (88, ["stop"])]
    main = [
        VobSpu(1000, line, 100, 400, start_stop),
        VobSpu(3000, line, 200, 300, [(0, ["place", "forced"]), (176, ["stop"])],
               colours=(4, 5, 6, 7), alphas=(0, 8, 15, 4)),
        # No stop: until the next.
        VobSpu(6000, line, 300, 100, [(0, ["place", "start"])]),
        VobSpu(8000, framed, 100, 100, start_stop),
        VobSpu(10000, wide, 101, 201, start_stop),
        # Placed at 0, started at 22: shown from a quarter second on.
        VobSpu(12000, line, 100, 400, [(0, ["place"]), (22, ["start"]), (88, ["stop"])]),
        VobSpu(14000, two, 200, 380, [(0, ["place", "start"]), (132, ["stop"])]),
        # No start: shown from its time.
        VobSpu(17000, line, 100, 400, [(0, ["place"]), (88, ["stop"])]),
        # No stop: mkvmerge's duration -- to the next -- ends it.
        VobSpu(19000, line, 100, 400, [(0, ["place", "start"])]),
        # Stopped five seconds on, but replaced before then by the next,
        # which starts a quarter second after its time: a seek between the
        # two finds this one still showing.
        VobSpu(20000, line, 100, 400, [(0, ["place", "start"]), (440, ["stop"])]),
        VobSpu(22000, two, 200, 380, [(0, ["place"]), (22, ["start"]), (88, ["stop"])]),
        # Colours changed and the picture stopped at one date: the new
        # colours are never seen -- no cue lasting no time.
        VobSpu(25000, line, 100, 400, [(0, ["place", "start"]), (88, [("colours", (0, 5, 6, 7))]),
                                       (88, ["stop"])]),
    ]
    dvd = [
        # A colour change at half a second, a fade at three quarters.
        VobSpu(1000, line, 100, 400, [(0, ["place", "start"]), (44, [("colours", (0, 5, 6, 7))]),
                                      (66, [("alphas", (0, 8, 8, 8))]), (88, ["stop"])]),
        # Started, stopped, started again, stopped.
        VobSpu(3000, line, 100, 400, [(0, ["place", "start"]), (44, ["stop"]), (88, ["start"]),
                                      (132, ["stop"])]),
        # Shown for four seconds, a transparent SPU after one.
        VobSpu(5000, line, 100, 400, [(0, ["place", "start"]), (352, ["stop"])]),
        VobSpu(6000, line, 100, 400, start_stop, alphas=(0, 0, 0, 0)),
    ]
    # Where a DVD player shows otherwise: the colour change and the fade
    # (1.5, 1.75 s); the first of two starts and its stop (3, 3.5 s); the
    # transparent SPU's clear and its own stop (6, 7.001 s), and the stop of
    # the picture it replaced, which FFmpeg shows on until (9.004 s).
    dvd_departures = {1500, 1750, 3000, 3500, 6000, 7001, 9004}
    grey = [
        VobSpu(1000, line, 100, 400, start_stop, alphas=(0, 15, 12, 8)),
        VobSpu(3000, framed, 100, 100, start_stop, colours=(0, 1, 2, 3), alphas=(15, 15, 15, 15)),
        VobSpu(5000, line, 100, 400, start_stop, colours=(5, 5, 7, 5), alphas=(15, 15, 15, 15)),
    ]
    pal = [VobSpu(1000, line, 100, 500, start_stop)]
    return main, dvd, dvd_departures, grey, pal


def vob_damage():
    """SPUs no muxer writes, each as FFmpeg takes it: the answer FFmpeg's."""
    line = vob_shape(240, 24, 0)
    good = VobSpu(0, line, 100, 400, [(0, ["place", "start"]), (352, ["stop"])])

    def patched(ms_time, fix):
        b = bytearray(good.bytes())
        fix(b)
        return (ms_time, bytes(b))

    first = lambda b: struct.unpack(">H", bytes(b[2:4]))[0]  # noqa: E731

    def fields_past(b):
        f = first(b) + 4 + bytes(b[first(b) + 4:]).index(bytes([VOB_FIELDS]))
        b[f + 3:f + 5] = struct.pack(">H", 0x7FF0)

    def taller(b):
        a = first(b) + 4 + bytes(b[first(b) + 4:]).index(bytes([VOB_AREA]))
        # y2's top four bits, 1 (y2 is 0x1A7): 3 makes the area 512 rows
        # taller than its codes.
        b[a + 5] = (b[a + 5] & 0xF0) | 0x03

    def loops(b):
        b[first(b) + 2:first(b) + 4] = struct.pack(">H", 4)

    def short(b):
        del b[12:]

    raw = [
        # Each while a picture shows: it stays.
        (1000, good.bytes()),
        patched(2000, fields_past),
        patched(3000, taller),
        patched(4000, short),
        # A sequence naming an earlier one next: read once, shown.
        patched(6000, loops),
    ]
    return raw


# --- DVB's subtitles: digital television's pictures, written here segment
# by segment --------------------------------------------------------------
#
# A block is segments -- 0x0F, a type, a page, a length, the body -- with no
# PES header around them (Matroska's S_DVBSUB). FFmpeg's dvbsub decoder is
# the oracle under `-dvb_substream 0 -compute_clut 0`, which make it read the
# service's pages alone and give a region of no CLUT the standard's default
# one, as a receiver does. mkvmerge takes no DVB stream of its own, so the
# generator writes the Matroska file too.

DVB_PAGE, DVB_REGION, DVB_CLUT, DVB_OBJECT, DVB_DISPLAY, DVB_END = 0x10, 0x11, 0x12, 0x13, 0x14, 0x80
DVB_ORACLE = ["-dvb_substream", "0", "-compute_clut", "0"]
DVB_MAP_2_TO_4 = [0x0, 0x7, 0x8, 0xF]
DVB_MAP_2_TO_8 = [0x00, 0x77, 0x88, 0xFF]
DVB_MAP_4_TO_8 = [v * 0x11 for v in range(16)]


def dvb_seg(kind, body, page=1):
    return bytes([0x0F, kind]) + struct.pack(">HH", page, len(body)) + body


class DvbBits:
    def __init__(self):
        self.bits = []

    def put(self, value, n):
        self.bits += [(value >> k) & 1 for k in range(n - 1, -1, -1)]

    def bytes(self):
        bits = self.bits + [0] * (-len(self.bits) % 8)
        return bytes(int("".join(map(str, bits[i:i + 8])), 2) for i in range(0, len(bits), 8))


def dvb_runs(row, longest):
    """A line as runs of one value, none longer than `longest`."""
    runs, i = [], 0
    while i < len(row):
        n = 1
        while i + n < len(row) and row[i + n] == row[i] and n < longest:
            n += 1
        runs.append((row[i], n))
        i += n
    return runs


def dvb_two_bit(row):
    """A 2-bit/pixel code string (EN 300 743 7.2.5.2), ended."""
    b = DvbBits()
    for c, n in dvb_runs(row, 284):
        while n:
            if c == 0 and n == 1:
                b.put(0, 2); b.put(0b01, 2); k = 1
            elif c == 0 and n == 2:
                b.put(0, 2); b.put(0b0001, 4); k = 2
            elif n < 3:
                b.put(c, 2); k = 1
            elif n <= 10:
                b.put(0, 2); b.put(1, 1); b.put(n - 3, 3); b.put(c, 2); k = n
            elif n == 11:
                b.put(0, 2); b.put(1, 1); b.put(7, 3); b.put(c, 2); k = 10
            elif n <= 27:
                b.put(0, 2); b.put(0b0010, 4); b.put(n - 12, 4); b.put(c, 2); k = n
            elif n == 28:
                b.put(0, 2); b.put(0b0010, 4); b.put(15, 4); b.put(c, 2); k = 27
            else:
                b.put(0, 2); b.put(0b0011, 4); b.put(n - 29, 8); b.put(c, 2); k = n
            n -= k
    b.put(0, 2); b.put(0b0000, 4)
    return b.bytes()


def dvb_four_bit(row):
    """A 4-bit/pixel code string, ended."""
    b = DvbBits()
    for c, n in dvb_runs(row, 280):
        while n:
            if c == 0 and n == 1:
                b.put(0, 4); b.put(0b1100, 4); k = 1
            elif c == 0 and n == 2:
                b.put(0, 4); b.put(0b1101, 4); k = 2
            elif c == 0 and n <= 9:
                b.put(0, 4); b.put(0, 1); b.put(n - 2, 3); k = n
            elif n < 4:
                b.put(c, 4); k = 1
            elif n <= 7:
                b.put(0, 4); b.put(0b10, 2); b.put(n - 4, 2); b.put(c, 4); k = n
            elif n == 8:
                b.put(0, 4); b.put(0b10, 2); b.put(3, 2); b.put(c, 4); k = 7
            elif n <= 24:
                b.put(0, 4); b.put(0b1110, 4); b.put(n - 9, 4); b.put(c, 4); k = n
            else:
                b.put(0, 4); b.put(0b1111, 4); b.put(n - 25, 8); b.put(c, 4); k = n
            n -= k
    b.put(0, 4); b.put(0, 4)
    return b.bytes()


def dvb_eight_bit(row):
    """An 8-bit/pixel code string, ended."""
    out = b""
    for c, n in dvb_runs(row, 127):
        while n:
            if c == 0:
                out += bytes([0, n]); k = n
            elif n < 3:
                out += bytes([c]); k = 1
            else:
                out += bytes([0, 0x80 | n, c]); k = n
            n -= k
    return out + bytes([0, 0])


DVB_STRINGS = {2: (0x10, dvb_two_bit), 4: (0x11, dvb_four_bit), 8: (0x12, dvb_eight_bit)}


class DvbObject:
    """An object's pixels, `bits` a pixel, coded in fields: the even lines
    the top field, the odd the bottom -- or, with `top_only`, no bottom
    field, which the top's lines are drawn again for. `maps`, (code, table),
    replace the standard map tables in each field."""

    def __init__(self, oid, rows, bits=4, non_mod=False, maps=(), top_only=False):
        self.oid, self.rows, self.bits = oid, rows, bits
        self.non_mod, self.maps, self.top_only = non_mod, maps, top_only

    def field(self, lines):
        kind, code = DVB_STRINGS[self.bits]
        out = b""
        for map_kind, table in self.maps:
            out += bytes([map_kind]) + (bytes([(table[0] << 4) | table[1], (table[2] << 4) | table[3]])
                                        if map_kind == 0x20 else bytes(table))
        for line in lines:
            out += bytes([kind]) + code(line) + bytes([0xF0])
        return out

    def segment(self, version=0):
        top = self.field(self.rows[0::2])
        bottom = b"" if self.top_only else self.field(self.rows[1::2])
        flags = (version << 4) | (int(self.non_mod) << 1) | 0x01
        return dvb_seg(DVB_OBJECT, struct.pack(">HBHH", self.oid, flags, len(top), len(bottom)) + top + bottom)


class DvbSet:
    """What a block holds, in order: segments as descriptions --
    ("display", w, h, version, window), ("page", timeout, version, state,
    regions), ("region", id, version, w, h, depth, clut, fill, objects),
    ("clut", id, version, entries), ("object", DvbObject), ("end",) -- and
    raw bytes as ("raw", bytes), drawn as nothing."""

    def __init__(self, ms_time, *segments):
        self.ms, self.segments = ms_time, segments

    def bytes(self):
        out = b""
        for s in self.segments:
            kind = s[0]
            if kind == "display":
                _, w, h, version, window = s
                body = bytes([(version << 4) | (0x08 if window else 0) | 0x07]) + struct.pack(">HH", w - 1, h - 1)
                if window:
                    body += struct.pack(">HHHH", *window)
                out += dvb_seg(DVB_DISPLAY, body)
            elif kind == "page":
                _, timeout, version, state, regions = s
                body = bytes([timeout, (version << 4) | (state << 2) | 0x03])
                for rid, x, y in regions:
                    body += bytes([rid, 0xFF]) + struct.pack(">HH", x, y)
                out += dvb_seg(DVB_PAGE, body)
            elif kind == "region":
                _, rid, version, w, h, depth, clut, fill, objects = s
                d = {2: 1, 4: 2, 8: 3}[depth]
                f = fill or 0
                body = bytes([rid, (version << 4) | ((fill is not None) << 3) | 0x07]) + struct.pack(">HH", w, h)
                body += bytes([(d << 5) | (d << 2) | 0x03, clut, f if depth == 8 else 0,
                               ((f if depth == 4 else 0) << 4) | ((f if depth == 2 else 0) << 2) | 0x03])
                for oid, x, y in objects:
                    body += struct.pack(">HHH", oid, x, 0xF000 | y)
                out += dvb_seg(DVB_REGION, body)
            elif kind == "clut":
                _, cid, version, entries = s
                body = bytes([cid, (version << 4) | 0x0F])
                for eid, flags, y, cr, cb, t in entries:
                    body += bytes([eid, flags])
                    if flags & 1:
                        body += bytes([y, cr, cb, t])
                    else:
                        body += struct.pack(">H", (y << 10) | (cr << 6) | (cb << 2) | t)
                out += dvb_seg(DVB_CLUT, body)
            elif kind == "object":
                out += s[1].segment()
            elif kind == "end":
                out += dvb_seg(DVB_END, b"")
            elif kind == "raw":
                out += s[1]
        return out


def dvb_ebml(eid, body):
    return eid + bytes([0x01]) + len(body).to_bytes(7, "big") + body


def dvb_uint(eid, v):
    return dvb_ebml(eid, v.to_bytes(8, "big"))


def mkv_tracks(path, tracks):
    """A Matroska file of subtitle tracks, each (codec ID, setup, default,
    [(ms, bytes)]), numbered from 1: a cluster at each time, holding the
    blocks of every track at it, with cues for the first track's -- a seek
    lands on the block it names."""
    header = dvb_ebml(b"\x1A\x45\xDF\xA3", dvb_ebml(b"\x42\x82", b"matroska") + dvb_uint(b"\x42\x87", 4)
                      + dvb_uint(b"\x42\x85", 2))
    info = dvb_ebml(b"\x15\x49\xA9\x66", dvb_uint(b"\x2A\xD7\xB1", 1_000_000) + dvb_ebml(b"\x4D\x80", b"lanef")
                    + dvb_ebml(b"\x57\x41", b"lanef"))
    entries = b""
    for n, (codec, private, default, _) in enumerate(tracks, 1):
        entry = (dvb_uint(b"\xD7", n) + dvb_uint(b"\x73\xC5", n) + dvb_uint(b"\x83", 0x11)
                 + dvb_uint(b"\x88", int(default)) + dvb_ebml(b"\x86", codec) + dvb_ebml(b"\x63\xA2", private))
        entries += dvb_ebml(b"\xAE", entry)
    track_list = dvb_ebml(b"\x16\x54\xAE\x6B", entries)
    times = sorted({t for *_, blocks in tracks for t, _ in blocks})
    clusters, cues = b"", []
    for ms_time in times:
        body = dvb_uint(b"\xE7", ms_time)
        for n, (*_, blocks) in enumerate(tracks, 1):
            for t, data in blocks:
                if t == ms_time:
                    body += dvb_ebml(b"\xA3", bytes([0x80 | n]) + struct.pack(">h", 0) + bytes([0x80]) + data)
                    if n == 1:
                        cues.append((ms_time, len(info) + len(track_list) + len(clusters)))
        clusters += dvb_ebml(b"\x1F\x43\xB6\x75", body)
    cue = dvb_ebml(b"\x1C\x53\xBB\x6B", b"".join(
        dvb_ebml(b"\xBB", dvb_uint(b"\xB3", t) + dvb_ebml(b"\xB7", dvb_uint(b"\xF7", 1) + dvb_uint(b"\xF1", pos)))
        for t, pos in cues))
    with open(path, "wb") as f:
        f.write(header + dvb_ebml(b"\x18\x53\x80\x67", info + track_list + clusters + cue))


def dvb_mkv(path, blocks, private):
    """A Matroska file of one S_DVBSUB track of `blocks`, (ms, bytes)."""
    mkv_tracks(path, [(b"S_DVBSUB", private, True, blocks)])


def dvb_colour(y, cr, cb, t):
    """A CLUT entry's RGBA: FFmpeg's BT.601 studio-range conversion (PGS's),
    alpha 255 - T, Y of 0 transparent."""
    r, g, b = pgs_colour(y, cr, cb, True)
    return (r, g, b, 0 if y == 0 else 255 - t)


def dvb_default_clut():
    """The standard's default CLUTs (EN 300 743 section 10): 2-, 4- and 8-bit."""
    def lv(on, full):
        return full if on else 0
    two = [(0, 0, 0, 0), (255, 255, 255, 255), (0, 0, 0, 255), (127, 127, 127, 255)]
    four = [(0, 0, 0, 0)] + [(lv(i & 1, f), lv(i & 2, f), lv(i & 4, f), 255)
                             for i in range(1, 16) for f in [255 if i < 8 else 127]]
    eight = [(0, 0, 0, 0)]
    for i in range(1, 256):
        if i < 8:
            eight.append((lv(i & 1, 255), lv(i & 2, 255), lv(i & 4, 255), 63))
            continue
        a, b = (85, 170) if i & 0x88 in (0x00, 0x08) else (43, 85)
        base = 127 if i & 0x88 == 0x80 else 0
        alpha = 127 if i & 0x88 == 0x08 else 255
        eight.append(tuple(base + lv(i & (1 << lo), a) + lv(i & (1 << hi), b) for lo, hi in ((0, 4), (1, 5), (2, 6)))
                     + (alpha,))
    return {2: two, 4: four, 8: eight}


def dvb_frame(display, page, regions, cluts, canvas):
    """The page's picture: its regions, the first it lists drawn last, each
    pixel through its CLUT's table for its depth (the standard's default
    for a CLUT not defined), copied over what is there as sub2video copies."""
    w, h = canvas
    frame = bytearray(w * h * 4)
    default = dvb_default_clut()
    for rid, x, y in reversed(page["regions"]):
        r = regions.get(rid)
        if not r or not r["w"] or not r["h"]:
            continue
        table = cluts[r["clut"]][r["depth"]] if r["clut"] in cluts else default[r["depth"]]
        for j in range(r["h"]):
            for i in range(r["w"]):
                v = r["pixels"][j * r["w"] + i]
                px, py = x + display["x"] + i, y + display["y"] + j
                if px < w and py < h:
                    frame[(py * w + px) * 4:(py * w + px) * 4 + 4] = bytes(table[v] if v < len(table) else (0, 0, 0, 0))
    return bytes(frame)


def dvb_draw_object(o, r, x0, y0):
    """Object `o`'s pixels drawn into region `r` at (x0, y0), as a receiver
    draws them: through the map table from its depth to the region's, a
    pixel of the non-modifying colour left as it was, the top field's lines
    again where there is no bottom field, no further than the region."""
    if o.bits > r["depth"]:
        return
    tables = {(2, 4): list(DVB_MAP_2_TO_4), (2, 8): list(DVB_MAP_2_TO_8), (4, 8): list(DVB_MAP_4_TO_8)}
    for map_kind, table in o.maps:
        tables[{0x20: (2, 4), 0x21: (2, 8), 0x22: (4, 8)}[map_kind]] = list(table)
    table = tables.get((o.bits, r["depth"]))
    lines = list(enumerate(o.rows))
    if o.top_only:
        lines = [(i + f, row) for i, row in lines[0::2] for f in (0, 1)]
    for i, row in lines:
        y = y0 + i
        if y >= r["h"]:
            continue
        for j, v in enumerate(row):
            x = x0 + j
            if x >= r["w"]:
                break
            if not (o.non_mod and v == 1):
                r["pixels"][y * r["w"] + x] = table[v] if table else v


def dvb_states(sets, canvas):
    """What a receiver shows of `sets` -- the crate's rules written here
    again, from the descriptions -- as [(ms, md5)] at each change: a block's
    display set shown for its page's timeout, a later block's change
    superseding what the last had still to make. Of several display sets
    in a block, the last."""
    display = {"version": None, "x": 0, "y": 0}
    page = {"version": None, "timeout": 0, "regions": []}
    regions, cluts, placed = {}, {}, []
    blank = hashlib.md5(bytes(canvas[0] * canvas[1] * 4)).hexdigest()
    made, pending = [], []
    for s in sets:
        shown, seen = None, 0
        for seg in s.segments:
            kind = seg[0]
            if kind == "display":
                _, w, h, version, window = seg
                if display["version"] != version:
                    display.update(version=version, x=window[0] if window else 0, y=window[2] if window else 0)
            elif kind == "page":
                _, timeout, version, state, listed = seg
                seen |= 1
                if page["version"] == version:
                    continue
                page.update(version=version, timeout=timeout, regions=[])
                if state in (1, 2):
                    regions.clear()
                    cluts.clear()
                    placed.clear()
                for rid, x, y in listed:
                    if any(r == rid for r, _, _ in page["regions"]):
                        break
                    page["regions"].append((rid, x, y))
            elif kind == "region":
                _, rid, version, w, h, depth, clut, fill, objects = seg
                seen |= 2
                r = regions.setdefault(rid, {"pixels": []})
                fresh = len(r["pixels"]) != w * h
                r.update(w=w, h=h, depth=depth, clut=clut)
                if fill is not None or fresh:
                    r["pixels"] = [fill or 0] * (w * h)
                placed[:] = [p for p in placed if p[1] != rid] + [(oid, rid, x, y) for oid, x, y in objects]
            elif kind == "clut":
                _, cid, version, entries = seg
                seen |= 4
                c = cluts.setdefault(cid, {"version": None, **{k: list(v) for k, v in dvb_default_clut().items()}})
                if c["version"] == version:
                    continue
                c["version"] = version
                for eid, flags, y, cr, cb, t in entries:
                    if not flags & 1:
                        y, cr, cb, t = y << 2, cr << 4, cb << 4, t << 6
                    for flag, depth, size in ((0x80, 2, 4), (0x40, 4, 16), (0x20, 8, 256)):
                        if flags & flag and eid < size:
                            c[depth][eid] = dvb_colour(y, cr, cb, t)
            elif kind == "object":
                seen |= 8
                for oid, rid, x0, y0 in reversed(placed):
                    if oid == seg[1].oid and rid in regions:
                        dvb_draw_object(seg[1], regions[rid], x0, y0)
            elif kind == "end":
                seen |= 16
                shown = dvb_frame(display, page, regions, cluts, canvas)
        # A page, a region and an object with no end segment: shown all the
        # same.
        if seen & 11 == 11 and not seen & 16:
            shown = dvb_frame(display, page, regions, cluts, canvas)
        if shown is None:
            continue
        timeout = page["timeout"]
        changes = [(s.ms, hashlib.md5(shown).hexdigest() if timeout else blank)]
        if timeout:
            changes.append((s.ms + 1000 * timeout, blank))
        made += [c for c in pending if c[0] < s.ms]
        pending = changes
    made += pending
    states = dedup(made)
    while states and states[0][1] == blank:
        states.pop(0)
    return states


def make_dvb(name, sets, canvas=(720, 576), private=b"\x00\x01\x00\x01\x10", departures=(), drawn=True):
    """Write NAME.mkv of `sets`, a block each, and NAME.states: the pictures a
    receiver shows -- FFmpeg's under DVB_ORACLE, each checked against the
    generator's drawing, the times in `departures` taking the drawing's; or,
    not `drawn`, FFmpeg's alone."""
    out = os.path.join(HERE, f"{name}.mkv")
    dvb_mkv(out, [(s.ms, s.bytes()) for s in sets], private)
    extra = DVB_ORACLE + (["-canvas_size", f"{canvas[0]}x{canvas[1]}"] if canvas != (720, 576) else [])
    states = sub2video_states(out, canvas, extra=extra)
    if drawn:
        drawing = dvb_states(sets if drawn is True else drawn, canvas)
        for t, md5 in drawing:
            if t not in departures and shown_at(states, t) != md5:
                sys.exit(f"{name}: at {t} ms FFmpeg shows {shown_at(states, t)}, the drawing {md5}")
        times = {t for t, _ in drawing}
        for t, _ in states:
            if t not in times and t not in departures:
                sys.exit(f"{name}: FFmpeg's picture changes at {t} ms, where the drawing's does not")
        states = drawing
    answer = os.path.join(HERE, f"{name}.states")
    write(answer, f"# {name}.mkv: the picture at each change, as an MD5 of the RGBA canvas"
                  f" (generate_subtitle_fixtures.py)\ncanvas {canvas[0]} {canvas[1]}\n"
                  + "".join(f"state {t} {md5}\n" for t, md5 in states))
    print("wrote", out, answer)


# A CLUT's entries: (entry, flags, Y, Cr, Cb, T); flags 0x80/0x40/0x20 the
# 2-, 4- and 8-bit tables, 0x01 full range (else Y in 6 bits, Cr and Cb in
# 4, T in 2).
DVB_FOUR = [(1, 0x41, 235, 128, 128, 0),    # white
            (2, 0x40, 4, 8, 8, 0),          # black, reduced range
            (3, 0x41, 81, 240, 90, 128),    # red, half transparent
            (4, 0x41, 145, 34, 54, 55),     # green
            (5, 0x41, 0, 128, 128, 0)]      # Y of 0: transparent
DVB_YELLOW = [(1, 0x41, 210, 146, 16, 0)]
DVB_BLUE = [(1, 0x41, 41, 110, 240, 0)]
DVB_TWO = [(1, 0x81, 235, 128, 128, 0), (2, 0x81, 81, 240, 90, 0), (3, 0x81, 41, 110, 240, 64)]
DVB_EIGHT = [(v, 0x21, 16 + v * 219 // 255, 255 - v, v, 0) for v in range(1, 256)]


def dvb_scenes():
    """The well-formed fixtures: what a receiver shows, FFmpeg agreeing."""
    line = vob_shape(200, 24, 0)
    short = vob_shape(120, 20, 2)
    two = [[(c // 5 + r) % 4 for c in range(100)] for r in range(16)]
    eight = [[(c * 2 + r * 8) % 256 for c in range(128)] for r in range(16)]
    gradient = [[(c // 8) % 16 for c in range(128)] for r in range(12)]
    # Runs of every length the 4-bit codes have, and of entry 5, whose Y of
    # 0 makes it transparent.
    runs = [[1] * 3 + [2] * 9 + [3] * 25 + [0] * 1 + [1] * 2 + [0] * 7 + [2] * 280 + [3] * 4 + [1] * 8
            + [5] * 6 + [1] * 2]
    # Runs of every length the 2-bit codes have.
    runs2 = [[1] * 3 + [2] * 12 + [3] * 29 + [0] * 2 + [1] * 1 + [0] * 1 + [2] * 284 + [3] * 10 + [1] * 27]
    v = iter(range(1, 10_000))

    def page(timeout, state, regions):
        return ("page", timeout, next(v) % 16, state, regions)

    # The page at 18 s; the track's last is of its version, so that a seek
    # back to 18 s after reading the track meets the page held, and must
    # have forgotten it.
    afresh = ("page", 10, 9, 2, [(1, 200, 400)])
    main = [
        DvbSet(500, ("display", 720, 576, 0, None)),
        # A line: a 4-bit region of a 4-bit object, the CLUT's colours full
        # range and reduced, half transparent, transparent by Y.
        DvbSet(1000, page(10, 2, [(1, 100, 400)]), ("region", 1, 0, 200, 24, 4, 1, None, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(1, line)), ("end",)),
        # Moved by the page alone: its pixels kept.
        DvbSet(2000, page(10, 0, [(1, 100, 300)]), ("end",)),
        # A new version of the CLUT recolours them; the same version again
        # changes nothing.
        DvbSet(3000, page(10, 0, [(1, 100, 300)]), ("clut", 1, 1, DVB_YELLOW), ("end",)),
        DvbSet(4000, page(10, 0, [(1, 100, 300)]), ("clut", 1, 1, DVB_BLUE), ("end",)),
        # A 2-bit region and an 8-bit one beside it, each its own CLUT.
        DvbSet(5000, page(10, 0, [(1, 100, 300), (2, 100, 100), (3, 300, 100)]),
               ("region", 2, 0, 100, 16, 2, 2, None, [(2, 0, 0)]),
               ("region", 3, 0, 128, 16, 8, 3, None, [(3, 0, 0)]),
               ("clut", 2, 0, DVB_TWO), ("clut", 3, 0, DVB_EIGHT),
               ("object", DvbObject(2, two, bits=2)), ("object", DvbObject(3, eight, bits=8)), ("end",)),
        # The page listing one: the others hidden, and back again with
        # their pixels -- region 2 filled again, its object gone ...
        DvbSet(6000, page(10, 0, [(3, 300, 100)]), ("end",)),
        DvbSet(7000, page(10, 0, [(1, 100, 300), (2, 100, 100), (3, 300, 100)]),
               ("region", 2, 1, 100, 16, 2, 2, 2, [(2, 0, 0)]), ("end",)),
        # ... until its data comes again.
        DvbSet(8000, page(10, 0, [(1, 100, 300), (2, 100, 100), (3, 300, 100)]),
               ("object", DvbObject(2, two, bits=2)), ("end",)),
        # One object placed in two regions, drawn in each when it comes.
        DvbSet(9000, page(10, 0, [(1, 100, 300), (2, 100, 100)]),
               ("region", 1, 1, 200, 24, 4, 1, None, [(4, 150, 4)]),
               ("region", 2, 2, 100, 16, 2, 2, None, [(4, 60, 2)]),
               ("object", DvbObject(4, [[1, 2, 3, 1, 2, 3, 1, 2]] * 6, bits=2)), ("end",)),
        # Objects of fewer bits than their regions: the standard's maps, and
        # a field's own.
        DvbSet(10000, page(10, 2, [(4, 50, 50), (5, 50, 100), (6, 50, 150), (7, 50, 200), (13, 300, 50),
                                   (14, 300, 100)]),
               ("region", 4, 0, 100, 16, 4, 1, None, [(5, 0, 0)]),
               ("region", 5, 0, 128, 12, 8, 3, None, [(6, 0, 0)]),
               ("region", 6, 0, 100, 16, 8, 3, None, [(7, 0, 0)]),
               ("region", 7, 0, 100, 16, 4, 1, None, [(8, 0, 0)]),
               ("region", 13, 0, 128, 12, 8, 3, None, [(14, 0, 0)]),
               ("region", 14, 0, 60, 10, 2, 2, 1, [(15, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("clut", 3, 0, DVB_EIGHT),
               ("object", DvbObject(5, two, bits=2)), ("object", DvbObject(6, gradient, bits=4)),
               ("object", DvbObject(7, two, bits=2, maps=[(0x21, [0x10, 0x50, 0x90, 0xD0])])),
               ("object", DvbObject(8, two, bits=2, maps=[(0x20, [4, 3, 2, 1])])),
               ("object", DvbObject(14, gradient, bits=4, maps=[(0x22, [255 - 16 * v for v in range(16)])])),
               # 4-bit data in a 2-bit region: nothing drawn over its fill.
               ("object", DvbObject(15, [[9] * 10] * 4, bits=4)), ("end",)),
        # No bottom field: the top's lines again; runs of every length the
        # codes have; an 8-bit region filled; 8-bit data in a 4-bit region,
        # which draws nothing over its fill.
        DvbSet(11000, page(10, 0, [(8, 100, 100), (9, 100, 200), (10, 100, 250), (11, 450, 100), (12, 450, 150)]),
               ("region", 8, 0, 200, 24, 4, 1, None, [(9, 0, 0)]),
               ("region", 9, 0, 350, 1, 4, 1, None, [(10, 0, 0)]),
               ("region", 10, 0, 370, 1, 4, 1, None, [(11, 0, 0)]),
               ("region", 11, 0, 60, 20, 8, 3, 200, [(12, 0, 0)]),
               ("region", 12, 0, 60, 20, 4, 1, 3, [(13, 0, 0)]),
               ("object", DvbObject(9, line, top_only=True)), ("object", DvbObject(10, runs)),
               ("object", DvbObject(11, runs2, bits=2)),
               ("object", DvbObject(12, [[9] * 10] * 4, bits=8)), ("object", DvbObject(13, [[9] * 10] * 4, bits=8)),
               ("end",)),
        # Gone after its page's two seconds.
        DvbSet(12000, page(2, 0, [(8, 100, 100)]), ("end",)),
        # A page of no time: never shown, but what showed goes.
        DvbSet(15000, page(10, 0, [(8, 100, 300)]), ("end",)),
        DvbSet(16000, page(0, 0, [(9, 100, 300)]), ("end",)),
        # A seek pair: a mode change at 18 s, its page changed at 20 s -- a
        # seek to 21 s must read from 18 s.
        DvbSet(18000, afresh, ("region", 1, 0, 120, 20, 4, 1, None, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(1, short)), ("end",)),
        DvbSet(20000, page(10, 0, [(1, 300, 400)]), ("end",)),
        # A CLUT never defined: the standard's default.
        DvbSet(22000, page(10, 2, [(1, 100, 100)]), ("region", 1, 0, 120, 20, 4, 9, None, [(1, 0, 0)]),
               ("object", DvbObject(1, short)), ("end",)),
        # Regions overlapping: the first listed on top; one listed twice,
        # shown at its first place.
        DvbSet(24000, page(10, 2, [(1, 100, 100), (2, 110, 108), (1, 400, 400)]),
               ("region", 1, 0, 40, 20, 4, 1, 1, [(1, 0, 0)]), ("region", 2, 0, 40, 20, 4, 1, 4, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(1, [[2] * 4] * 2)), ("end",)),
        # A mode change forgets every region: a page listing one defined
        # before the change at 24 s -- region 8, at 11 s -- shows nothing.
        DvbSet(25000, page(10, 0, [(8, 100, 100)]), ("end",)),
    ]
    # A page of the version held changes nothing: the second lists the
    # regions again, and the screen stays clear.
    cleared = page(10, 0, [])
    main += [
        DvbSet(26000, cleared, ("end",)),
        DvbSet(27000, ("page", 10, cleared[2], 0, [(1, 100, 100)]), ("end",)),
        DvbSet(28000, page(10, 0, [(2, 100, 100)]), ("end",)),
        DvbSet(29000, ("page", 10, afresh[2], 0, []), ("end",)),
    ]
    # Each of those two pages is read: the page before it is of another
    # version.
    pages = {s.ms: seg[2] for s in main for seg in s.segments if seg[0] == "page"}
    if pages[16000] == pages[18000] or pages[28000] == pages[29000]:
        sys.exit("dvb: a page meant to be read is of the version before it")
    return main


def dvb_receiver():
    """Where a receiver shows otherwise than FFmpeg: [sets], {departures}."""
    line = vob_shape(200, 24, 1)
    v = iter(range(1, 10_000))

    def page(timeout, state, regions):
        return ("page", timeout, next(v) % 16, state, regions)

    # Entries for the 2- and the 4-bit table at once: FFmpeg's 4-bit table
    # keeps its defaults.
    both = [(e, 0xC1, y, cr, cb, t) for e, _, y, cr, cb, t in DVB_FOUR[:3]]
    stripes = [[2, 2, 2, 2, 1, 1, 1, 1] * 25] * 8
    sets = [
        DvbSet(1000, page(10, 2, [(1, 100, 400)]), ("region", 1, 0, 200, 24, 4, 1, None, [(1, 0, 0)]),
               ("clut", 1, 0, both), ("object", DvbObject(1, line)), ("end",)),
        # A region filled, no object drawn in it yet: FFmpeg shows nothing.
        DvbSet(2000, page(10, 2, [(2, 100, 100)]), ("region", 2, 0, 80, 20, 4, 1, 3, []),
               ("clut", 1, 0, DVB_FOUR), ("end",)),
        # The non-modifying colour over a fill: FFmpeg draws what follows a
        # run of it that many pixels to the left.
        DvbSet(3000, page(10, 2, [(3, 100, 200)]), ("region", 3, 0, 200, 8, 4, 1, 4, [(3, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(3, stripes, non_mod=True)), ("end",)),
        # Two display sets in one block, each showing: the last shows
        # (FFmpeg: the first) ...
        DvbSet(4000, page(10, 2, [(1, 100, 400)]), ("region", 1, 0, 200, 24, 4, 1, None, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(1, line)), ("end",),
               page(10, 0, [(1, 300, 300)]), ("end",)),
        # ... and showing, then clearing: cleared (FFmpeg: shown).
        DvbSet(5000, page(10, 0, [(1, 100, 100)]), ("end",), page(10, 0, []), ("end",)),
        DvbSet(6000, page(10, 0, [(1, 200, 200)]), ("end",)),
        DvbSet(7000, page(10, 0, []), ("end",)),
    ]
    return sets, {1000, 2000, 3000, 4000, 5000}


def dvb_hd():
    """A 1920 x 1080 display with a window: the regions placed in it."""
    line = vob_shape(300, 30, 0)
    return [
        DvbSet(1000, ("display", 1920, 1080, 0, (240, 1679, 135, 944)), ("page", 10, 1, 2, [(1, 0, 0), (2, 1000, 700)]),
               ("region", 1, 0, 300, 30, 4, 1, None, [(1, 0, 0)]), ("region", 2, 0, 300, 30, 4, 1, None, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", DvbObject(1, line)), ("end",)),
        # A display definition of the version held, another size and no
        # window: changes nothing.
        DvbSet(2000, ("display", 1280, 720, 0, None), ("page", 10, 3, 0, [(1, 0, 0)]), ("end",)),
        DvbSet(3000, ("page", 10, 2, 0, []), ("end",)),
    ]


def dvb_pages():
    """A service's page beside another's: the setup names pages 1 and 2 --
    the composition page, and the ancillary page whose CLUT and object it
    shares -- and page 3's sets, another service's, are not read."""
    line = vob_shape(200, 24, 0)

    def on(page_id, segment):
        return ("raw", segment[:2] + struct.pack(">H", page_id) + segment[4:])

    clut = DvbSet(0, ("clut", 1, 0, DVB_FOUR)).bytes()
    obj = DvbObject(1, line).segment()
    other = DvbSet(0, ("page", 10, 7, 2, [(5, 0, 0)]), ("region", 5, 0, 40, 40, 4, 1, 4, []), ("end",)).bytes()
    other = b"".join(dvb_seg(s[1], s[6:], 3) for s in [other[i:i + 6 + struct.unpack(">H", other[i + 4:i + 6])[0]]
                                                       for i in dvb_offsets(other)])
    return [
        DvbSet(1000, ("page", 10, 1, 2, [(1, 100, 400)]), ("region", 1, 0, 200, 24, 4, 1, None, [(1, 0, 0)]),
               ("raw", clut[:2] + struct.pack(">H", 2) + clut[4:]), ("raw", obj[:2] + struct.pack(">H", 2) + obj[4:]),
               ("end",)),
        DvbSet(2000, ("raw", other)),
        DvbSet(3000, ("page", 10, 2, 0, [(1, 100, 300)]), ("end",)),
        DvbSet(4000, ("raw", other)),
        DvbSet(5000, ("page", 10, 3, 0, []), ("end",)),
    ]


def dvb_offsets(data):
    """Where each segment of `data` starts."""
    at, out = 0, []
    while at + 6 <= len(data):
        out.append(at)
        at += 6 + struct.unpack(">H", data[at + 4:at + 6])[0]
    return out


def dvb_drawn_pages(sets):
    """`dvb_pages`'s sets as the receiver reads them, for the drawing: the
    raw segments on pages 1 and 2 put back as descriptions, page 3's gone."""
    line = vob_shape(200, 24, 0)
    out = []
    for s in sets:
        segs = []
        for seg in s.segments:
            if seg[0] != "raw":
                segs.append(seg)
            elif seg[1][1] == DVB_CLUT:
                segs.append(("clut", 1, 0, DVB_FOUR))
            elif seg[1][1] == DVB_OBJECT:
                segs.append(("object", DvbObject(1, line)))
        out.append(DvbSet(s.ms, *segs))
    return out


def dvb_damage():
    """Blocks no muxer writes, each as FFmpeg takes it: the answer FFmpeg's.
    A good display set shows a line at 1 s; each later block moves it, and
    meets damage."""
    line = vob_shape(200, 24, 0)
    v = iter(range(1, 10_000))

    def page(x, y=400, state=0, timeout=30):
        return ("page", timeout, next(v) % 16, state, [(1, x, y)])

    def raw(*segments):
        return ("raw", b"".join(segments))

    good = [("region", 1, 0, 200, 24, 4, 1, None, [(1, 0, 0)]), ("clut", 1, 0, DVB_FOUR),
            ("object", DvbObject(1, line))]
    unplaced = DvbObject(9, line).segment()
    # An object of characters: none, then bytes that read as pixels would be
    # a top field of two bytes and no bottom -- damage only as characters,
    # which FFmpeg does not read, not by any length.
    chars = dvb_seg(DVB_OBJECT, struct.pack(">HB", 1, 0x05) + bytes([0, 2, 0, 0, 0x10, 0xF0, 0]))
    fields_past = dvb_seg(DVB_OBJECT, struct.pack(">HBHH", 1, 0x01, 400, 400) + bytes(20))
    clut_cut = DvbSet(0, ("clut", 1, 5, DVB_BLUE)).bytes()[:-2]
    clut_cut = clut_cut[:4] + struct.pack(">H", len(clut_cut) - 6) + clut_cut[6:]
    # A line of runs longer than the region, in several codes: the rest of
    # the string read as what follows.
    long_line = DvbObject(1, [[1] * 150 + [2] * 150 + [3] * 3 + [1] * 2] * 2)
    return [
        DvbSet(1000, page(100, state=2), *good, ("end",)),
        # Every kind of segment but the end: shown all the same.
        DvbSet(2000, page(110), *good),
        # A page alone, no end: not shown.
        DvbSet(3000, page(120)),
        DvbSet(3500, page(130), ("end",)),
        # A segment running past the block.
        DvbSet(4000, page(140), raw(bytes([0x0F, DVB_END, 0, 1, 0, 9]))),
        # A byte other than the sync byte, then an end segment never read.
        DvbSet(5000, page(150), raw(b"\x0E"), ("end",)),
        # Unknown segments and stuffing passed over.
        DvbSet(6000, page(160), raw(dvb_seg(0x40, b"\x01\x02\x03"), dvb_seg(0xFF, b"\xFF\xFF")), ("end",)),
        # Data for an object no region places.
        DvbSet(7000, page(170), raw(unplaced), ("end",)),
        # An object of characters.
        DvbSet(8000, page(180), raw(chars), ("end",)),
        # Field lengths past the segment.
        DvbSet(9000, page(190), raw(fields_past), ("end",)),
        # A CLUT whose last entry is cut short.
        DvbSet(10000, page(200), raw(clut_cut), ("end",)),
        # A region of no width; one placing an object outside it.
        DvbSet(11000, page(210), ("region", 1, 1, 0, 24, 4, 1, None, []), ("end",)),
        DvbSet(12000, page(220, state=2), ("region", 1, 0, 200, 24, 4, 1, None, [(1, 300, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("end",)),
        # A line longer than its region, in several codes.
        DvbSet(13000, page(230, state=2), ("region", 1, 0, 200, 24, 4, 1, 2, [(1, 0, 0)]),
               ("clut", 1, 0, DVB_FOUR), ("object", long_line), ("end",)),
        # A page moving the line, no end; then a block of an end segment
        # alone, six bytes, which FFmpeg refuses: the move never shown.
        DvbSet(14000, page(240)),
        DvbSet(14500, ("end",)),
        DvbSet(15000, ("page", 30, next(v) % 16, 0, []), ("end",)),
    ]


def dvb_main():
    """The DVB fixtures (`python generate_subtitle_fixtures.py --dvb` makes
    them alone)."""
    make_dvb("dvb", dvb_scenes())
    receiver, receiver_departures = dvb_receiver()
    make_dvb("dvb_receiver", receiver, departures=receiver_departures)
    make_dvb("dvb_hd", dvb_hd(), canvas=(1920, 1080))
    pages = dvb_pages()
    make_dvb("dvb_pages", pages, private=b"\x00\x01\x00\x02\x10", drawn=dvb_drawn_pages(pages))
    make_dvb("dvb_damage", dvb_damage(), drawn=False)
    # Kate -- not read here -- marked default beside DVB pictures.
    kate = os.path.join(HERE, "kate_and_dvb.mkv")
    mkv_tracks(kate, [(b"S_KATE", b"", True, [(1000, b"kate")]),
                      (b"S_DVBSUB", b"\x00\x01\x00\x01\x10", False, [(s.ms, s.bytes()) for s in dvb_hd()])])
    print("wrote", kate)


def make_mixed():
    """Films with subtitle tracks of several kinds, for which one Subtitles
    opens: `pgs_and_text.mkv`, Blu-ray pictures marked default beside SubRip
    text; `dvb_and_pgs.mkv`, DVB pictures (ffmpeg's dvbsub encoder's, from
    pgs_sd's) marked default beside Blu-ray's. Made from fixtures made
    already. (`kate_and_dvb.mkv` is `dvb_main`'s.)"""
    def path(name):
        return os.path.join(HERE, name)

    def merge(out, *sources):
        args = [MKVMERGE, "--quiet", "--deterministic", "mixed", "-o", path(out)]
        for name, default in sources:
            args += ["--default-track-flag", f"0:{'yes' if default else 'no'}", path(name)]
        r = subprocess.run(args, capture_output=True, text=True, encoding="utf-8")
        if r.returncode != 0:
            sys.exit(f"mkvmerge {out} failed:\n{r.stdout}{r.stderr}")
        print("wrote", path(out))

    merge("pgs_and_text.mkv", ("pgs_sd.mkv", True), ("subrip.mkv", False))
    # mkvmerge takes no DVB track of ffmpeg's (its setup is not what
    # mkvmerge looks for), so ffmpeg muxes the two.
    dvb = path("dvb.tmp.mkv")
    ffmpeg("-copyts", "-i", path("pgs_sd.mkv"), "-map", "0", "-c:s", "dvbsub",
           "-fflags", "+bitexact", "-flags", "+bitexact", dvb)
    ffmpeg("-copyts", "-i", dvb, "-i", path("pgs_sd.mkv"), "-map", "0:0", "-map", "1:0", "-c", "copy",
           "-disposition:0", "default", "-disposition:1", "0",
           "-fflags", "+bitexact", "-flags", "+bitexact", path("dvb_and_pgs.mkv"))
    os.remove(dvb)
    print("wrote", path("dvb_and_pgs.mkv"))


# --- TTML in MP4 (`stpp`) ------------------------------------------------
#
# FFmpeg has no TTML decoder. The answers are ttconv's (pip install ttconv,
# 1.2.3: the converter from the IMSC 1 reference implementation's authors),
# used as a library: its intermediate synchronic documents say what shows
# at each moment, region by region -- its timing, region association,
# styles and white space -- and the generator says each paragraph as the
# crate says it (subtitle/ttml.rs): a cue a stretch of showing the same;
# bold, italic (oblique too), underline, line-through and colour (white
# being none); breaks never two together nor at either end; hidden text
# dropped; `{\anN}` for the third of the picture each way its anchor falls
# in. The answer, NAME.cues, is those cues as runs of styled text, which
# the test compares with the crate's markup read back into runs.
#
# ISO/IEC 14496-30 makes each sample a whole document, shown only for the
# sample's stretch of time, so the answer is read from the MP4's samples,
# not from the document they were made from: ttconv's reading of each
# sample, cut to the sample, the stretches of showing the same joined across
# samples. A sample ttconv cannot read shows nothing, and is counted
# (`damaged N`). The samples written here are each the whole document, and
# their reading is checked to be the document's. MP4Box's are its own
# rewriting of the document, and say less, two ways (GPAC git a23ae2d):
#
#   - Its splitter times each paragraph by its own begin and end alone: a
#     container's begin, a sequence's turns and an untimed paragraph's
#     parent are not counted, so a paragraph lands in samples it does not
#     show in, and is missing from those it does (`ttml_timing`: four cues
#     of the document's thirteen).
#   - It writes `&lt;` and `&amp;` back as bare `<` and `&` -- its DOM
#     decodes the references when it parses, and its serialiser skips
#     escaping for text it marked as parsed from a file -- so a sample with
#     either is not XML. GPAC rejects it too, reading the file back
#     (`gpac -i ttml.mp4 -o back.ttml`: "Corrupted Data", the paragraph
#     gone), as do ttconv and the browsers' parsers (`ttml`: its sample of
#     `a &lt; b &amp; c`).
#
# What MP4Box's files say is held to as they say it -- a reader shows what
# the samples hold, inside each sample's stretch -- and `ttml_timing_one`,
# the timing document in one sample of its own, holds the reader to the
# document's own timing.

TTML_NS = ('xmlns="http://www.w3.org/ns/ttml" xmlns:tts="http://www.w3.org/ns/ttml#styling" '
           'xmlns:ttp="http://www.w3.org/ns/ttml#parameter"')

TTML_BASIC = f"""<?xml version="1.0" encoding="UTF-8"?>
<tt {TTML_NS} ttp:timeBase="media" xml:lang="en">
  <head>
    <styling>
      <style xml:id="base" tts:color="yellow"/>
      <style xml:id="loud" style="base" tts:fontWeight="bold"/>
      <style xml:id="quiet" tts:fontStyle="italic" tts:color="cyan"/>
    </styling>
    <layout>
      <region xml:id="bottom" tts:origin="10% 75%" tts:extent="80% 20%" tts:displayAlign="after" tts:textAlign="center"/>
      <region xml:id="top" tts:origin="10% 5%" tts:extent="80% 20%" tts:displayAlign="before" tts:textAlign="center"/>
      <region xml:id="left" tts:origin="5% 40%" tts:extent="25% 20%" tts:displayAlign="center" tts:textAlign="start"/>
    </layout>
  </head>
  <body>
    <div region="bottom">
      <p xml:id="plain" begin="00:00:01.000" end="00:00:02.500">Plain text</p>
      <p xml:id="styles" begin="3s" end="4s"><span tts:fontWeight="bold">bold</span>, <span tts:fontStyle="italic">italic</span>, <span tts:textDecoration="underline">under</span>, <span tts:textDecoration="lineThrough">struck</span></p>
      <p xml:id="referenced" begin="5s" end="6s" style="loud">loud yellow <span style="quiet">quiet cyan</span> <span style="quiet" tts:color="lime">its own lime</span> loud again</p>
      <p xml:id="broken" begin="7s" end="8s">two<br/>lines</p>
      <p xml:id="spaced" begin="9s" end="10s">   spaced
         out   </p>
      <p xml:id="kept" begin="11s" end="12s" xml:space="preserve">kept   spaces</p>
      <p xml:id="colours" begin="13s" end="14s"><span tts:color="#ff000080">half red</span> <span tts:color="rgb(0,128,255)">rgb</span> <span tts:color="white">white</span></p>
      <p xml:id="escaped" begin="15s" end="16s"><span tts:fontStyle="oblique">oblique</span> a &lt; b &amp; c {{\\an8}}</p>
      <p xml:id="hidden" begin="17s" end="18s">seen <span tts:visibility="hidden">hidden</span> and on</p>
      <p xml:id="nested" begin="19s" end="20s"><span tts:textDecoration="underline">under <span tts:fontWeight="bold">and bold</span> <span tts:textDecoration="noUnderline">not under</span></span></p>
      <p xml:id="breaks" begin="21s" end="22s"><br/>a<br/><br/>b<br/></p>
    </div>
    <div region="top"><p xml:id="up" begin="1s" end="2s">at the top</p></div>
    <div region="left"><p xml:id="aside" begin="1s" end="2s">at the left</p></div>
  </body>
</tt>
"""

TTML_TIMING = f"""<?xml version="1.0" encoding="UTF-8"?>
<tt {TTML_NS} ttp:frameRate="25" ttp:tickRate="1000">
  <head>
    <layout>
      <region xml:id="r" tts:origin="10% 80%" tts:extent="80% 15%" tts:displayAlign="after" tts:textAlign="center"/>
    </layout>
  </head>
  <body region="r">
    <!-- An end of its own: ttconv ends a parallel container that begins
         late early, where its children's ends are on its own clock and its
         implicit end on its parent's (the reader follows TTML there, and a
         unit test holds it). -->
    <div begin="1s" end="6s">
      <p xml:id="offset" begin="0s" end="2s">offset by the div</p>
      <p xml:id="frames" begin="00:00:01:05" dur="00:00:00:20">frames</p>
      <p xml:id="ticks" begin="2500t" end="3.5s">ticks</p>
    </div>
    <div begin="5s" timeContainer="seq">
      <p xml:id="first" dur="1s">first in turn</p>
      <p xml:id="second" dur="1500ms">second in turn</p>
      <p xml:id="gap" begin="500ms" dur="1s">after a gap</p>
    </div>
    <div begin="10s" end="13s">
      <p xml:id="growing">whole div <span begin="1s">and later</span><span begin="2s" end="2500ms"> and briefly</span></p>
    </div>
    <div>
      <p xml:id="long" begin="14s" end="17s">long</p>
      <p xml:id="inside" begin="15s" end="16s">inside</p>
      <p xml:id="hours" begin="0.005h" end="0.31m">by hours and minutes</p>
    </div>
  </body>
</tt>
"""

TTML_REGIONS = f"""<?xml version="1.0" encoding="UTF-8"?>
<tt {TTML_NS}>
  <head>
    <styling>
      <style xml:id="hide" tts:display="none"/>
    </styling>
    <layout>
      <region xml:id="a" tts:origin="0% 0%" tts:extent="50% 50%" tts:displayAlign="before" tts:textAlign="start"/>
      <region xml:id="b" tts:origin="50% 50%" tts:extent="50% 50%" tts:displayAlign="after" tts:textAlign="end"/>
      <region xml:id="c" tts:origin="25% 25%" tts:extent="50% 50%" tts:displayAlign="center"/>
    </layout>
  </head>
  <body>
    <div region="a">
      <p xml:id="in_a" begin="1s" end="2s">in a</p>
      <p xml:id="overridden" begin="1s" end="2s" region="b">named b inside a: shown nowhere</p>
    </div>
    <div>
      <p xml:id="in_b" begin="1s" end="2s" region="b">in b</p>
      <p xml:id="in_c" begin="1s" end="2s" region="c">in c, the middle of it</p>
      <p xml:id="nowhere" begin="1s" end="2s">in no region: shown nowhere</p>
      <p xml:id="unknown" begin="1s" end="2s" region="zz">an unknown region: shown nowhere</p>
      <p xml:id="gone" begin="3s" end="4s" region="a" style="hide">display none</p>
      <p xml:id="span_region" begin="3s" end="4s"><span region="b">a span's own region</span></p>
    </div>
  </body>
</tt>
"""

# No layout at all: the root container is the one region, and TTML's
# initial alignments put the text at its top left.
TTML_DEFAULT = f"""<?xml version="1.0" encoding="UTF-8"?>
<tt {TTML_NS}><body><div><p xml:id="lone" begin="1s" end="2s">no region at all</p></div></body></tt>
"""


def stpp_mp4(path, samples):
    """An MP4 of one TTML track (`stpp`, ISO/IEC 14496-30), written here
    box by box: `samples` each (start, end, document) in milliseconds,
    contiguous, the first from 0."""
    def full(kind, version, flags, *parts):
        return tx3g_box(kind, struct.pack(">I", (version << 24) | flags), *parts)

    data = [doc for _, _, doc in samples]
    durations = [end - start for start, end, _ in samples]
    total = sum(durations)
    identity = struct.pack(">9i", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)
    ftyp = tx3g_box(b"ftyp", b"isom", struct.pack(">I", 0x200), b"isom", b"iso2", b"mp41")
    mdat = tx3g_box(b"mdat", b"".join(data))
    entry = tx3g_box(b"stpp", b"\0" * 6, struct.pack(">H", 1),
                     b"http://www.w3.org/ns/ttml\0", b"\0", b"\0")
    runs = []
    for d in durations:
        if runs and runs[-1][1] == d:
            runs[-1][0] += 1
        else:
            runs.append([1, d])
    offsets, at = [], len(ftyp) + 8
    for d in data:
        offsets.append(at)
        at += len(d)
    stbl = tx3g_box(
        b"stbl",
        full(b"stsd", 0, 0, struct.pack(">I", 1), entry),
        full(b"stts", 0, 0, struct.pack(">I", len(runs)), *[struct.pack(">II", n, d) for n, d in runs]),
        full(b"stsc", 0, 0, struct.pack(">I", 1), struct.pack(">III", 1, 1, 1)),
        full(b"stsz", 0, 0, struct.pack(">II", 0, len(data)), *[struct.pack(">I", len(s)) for s in data]),
        full(b"stco", 0, 0, struct.pack(">I", len(offsets)), *[struct.pack(">I", o) for o in offsets]),
    )
    dinf = tx3g_box(b"dinf", full(b"dref", 0, 0, struct.pack(">I", 1), full(b"url ", 0, 1)))
    mdia = tx3g_box(
        b"mdia",
        full(b"mdhd", 0, 0, struct.pack(">IIIIHH", 0, 0, 1000, total, 0x55C4, 0)),
        full(b"hdlr", 0, 0, struct.pack(">I4s", 0, b"subt"), b"\0" * 12, b"SubtitleHandler\0"),
        tx3g_box(b"minf", full(b"sthd", 0, 0), dinf, stbl),
    )
    tkhd = full(b"tkhd", 0, 3, struct.pack(">IIIII", 0, 0, 1, 0, total), b"\0" * 8,
                struct.pack(">hhhH", 0, 0, 0, 0), identity, struct.pack(">II", 0, 0))
    mvhd = full(b"mvhd", 0, 0, struct.pack(">IIII", 0, 0, 1000, total), struct.pack(">IH", 0x10000, 0x100),
                b"\0" * 10, identity, b"\0" * 24, struct.pack(">I", 2))
    moov = tx3g_box(b"moov", mvhd, tx3g_box(b"trak", tkhd, mdia))
    with open(path, "wb") as f:
        f.write(ftyp + mdat + moov)


def stpp_samples(path):
    """The samples of the MP4 at `path`, its one track TTML: [(start, end,
    bytes)], start and end exact fractions of a second. The track is
    checked to carry nothing that would move its samples (an edit list,
    composition offsets), which this does not apply."""
    from fractions import Fraction

    with open(path, "rb") as f:
        data = f.read()

    def boxes(start, end):
        at = start
        while at + 8 <= end:
            size, kind = struct.unpack(">I4s", data[at:at + 8])
            head = 8
            if size == 1:
                size, head = struct.unpack(">Q", data[at + 8:at + 16])[0], 16
            elif size == 0:
                size = end - at
            yield kind, at + head, at + size
            at += size

    def inside(start, end, *path):
        for kind, body, stop in boxes(start, end):
            if kind == path[0]:
                return (body, stop) if len(path) == 1 else inside(body, stop, *path[1:])
        sys.exit(f"{path}: no {path[0]!r}")

    trak = inside(0, len(data), b"moov", b"trak")
    if any(kind == b"edts" for kind, _, _ in boxes(*trak)):
        sys.exit(f"{path}: an edit list, which stpp_samples does not apply")
    mdhd = inside(*trak, b"mdia", b"mdhd")[0]
    scale = struct.unpack(">I", data[mdhd + (20 if data[mdhd] == 1 else 12):][:4])[0]
    tables = {kind: body for kind, body, _ in boxes(*inside(*trak, b"mdia", b"minf", b"stbl"))}
    if b"ctts" in tables:
        sys.exit(f"{path}: composition offsets, which stpp_samples does not apply")

    def entries(kind, fmt):
        body = tables[kind]
        n = struct.unpack(">I", data[body + 4:body + 8])[0]
        size = struct.calcsize(fmt)
        return [struct.unpack(fmt, data[body + 8 + size * i:body + 8 + size * (i + 1)]) for i in range(n)]

    durations = [d for n, d in entries(b"stts", ">II") for _ in range(n)]
    body = tables[b"stsz"]
    fixed, count = struct.unpack(">II", data[body + 4:body + 12])
    sizes = [fixed] * count if fixed else list(struct.unpack(f">{count}I", data[body + 12:body + 12 + 4 * count]))
    chunks = [o for (o,) in (entries(b"stco", ">I") if b"stco" in tables else entries(b"co64", ">Q"))]
    stsc = entries(b"stsc", ">III")
    offsets = []
    for number, at in enumerate(chunks, 1):
        for _ in range([e for e in stsc if e[0] <= number][-1][1]):
            if len(offsets) < len(sizes):
                offsets.append(at)
                at += sizes[len(offsets) - 1]
    if not len(offsets) == len(sizes) == len(durations):
        sys.exit(f"{path}: tables that disagree on how many samples")
    samples, t = [], 0
    for offset, size, d in zip(offsets, sizes, durations):
        samples.append((Fraction(t, scale), Fraction(t + d, scale), data[offset:offset + size]))
        t += d
    return samples


def ttml_cues(samples):
    """ttconv's reading of a TTML track's `samples` [(start, end, bytes)] --
    each a whole document shown only from `start` to `end` (`None`: on and
    on), as ISO/IEC 14496-30 has it -- said as the crate says it:
    ([(start_ns, end_ns, an, runs)], damaged). Each run is ("text", text,
    flags, colour) or ("break",); the cues come in the order they begin --
    those beginning together region by region in the document's order, then
    in the document's order -- and `damaged` counts the samples ttconv
    cannot read, which show nothing."""
    import xml.etree.ElementTree as et
    from fractions import Fraction
    import ttconv.model as model
    from ttconv.imsc.reader import to_model
    from ttconv.isd import ISD
    from ttconv.style_properties import (DisplayAlignType, FontStyleType, FontWeightType, StyleProperties as S,
                                         TextAlignType, VisibilityType)

    third = lambda v: 0 if v < Fraction(1, 3) else (1 if v < Fraction(2, 3) else 2)

    def runs_of(p):
        pieces = []

        def walk(e, style):
            for c in e:
                if isinstance(c, model.Br):
                    pieces.append(("break",))
                elif isinstance(c, model.Text):
                    if c.get_text():
                        pieces.append(("text", c.get_text(), style))
                else:
                    walk(c, span_style(c) if isinstance(c, model.Span) else style)
        walk(p, None)
        # Hidden text dropped; kept line feeds breaks; breaks never two
        # together nor at either end.
        steps = []
        for piece in pieces:
            if piece[0] == "break":
                if steps and steps[-1][0] != "break":
                    steps.append(piece)
                continue
            text, style = piece[1], piece[2]
            if style["hidden"]:
                continue
            for i, line in enumerate(text.split("\n")):
                if i and steps and steps[-1][0] != "break":
                    steps.append(("break",))
                if line:
                    steps.append(("text", line, style))
        while steps and steps[-1][0] == "break":
            steps.pop()
        if all(s[0] == "break" or s[1].isspace() or not s[1] for s in steps):
            return None
        # Runs of one style together.
        runs = []
        for s in steps:
            if s[0] == "break":
                runs.append(("break",))
                continue
            flags = "".join("1" if s[2][k] else "0" for k in ("bold", "italic", "underline", "strike"))
            colour = s[2]["colour"]
            if runs and runs[-1][0] == "text" and runs[-1][2:] == (flags, colour):
                runs[-1] = ("text", runs[-1][1] + s[1], flags, colour)
            else:
                runs.append(("text", s[1], flags, colour))
        return tuple(runs)

    def span_style(span):
        r, g, b, _ = span.get_style(S.Color).components
        td = span.get_style(S.TextDecoration)
        return {
            "bold": span.get_style(S.FontWeight) is FontWeightType.bold,
            "italic": span.get_style(S.FontStyle) in (FontStyleType.italic, FontStyleType.oblique),
            "underline": bool(td and td.underline),
            "strike": bool(td and td.line_through),
            "colour": "-" if (r, g, b) == (255, 255, 255) else "%02x%02x%02x" % (r, g, b),
            "hidden": span.get_style(S.Visibility) is VisibilityType.hidden,
        }

    def placement(region, p):
        origin, extent = region.get_style(S.Origin), region.get_style(S.Extent)
        left, top = Fraction(origin.x.value) / 100, Fraction(origin.y.value) / 100
        width, height = Fraction(extent.width.value) / 100, Fraction(extent.height.value) / 100
        # ttconv reads left and justify as start, right as end.
        across = {TextAlignType.start: 0, TextAlignType.center: 1, TextAlignType.end: 2}[p.get_style(S.TextAlign)]
        down = {DisplayAlignType.before: 0, DisplayAlignType.center: 1,
                DisplayAlignType.after: 2}[region.get_style(S.DisplayAlign)]
        column, row = third(left + width * Fraction(across, 2)), third(top + height * Fraction(down, 2))
        return (7, 4, 1)[row] + column

    def ns(t):
        return int((t * 10**9 + Fraction(1, 2)) // 1)

    def paragraphs(e):
        for c in e:
            if isinstance(c, model.P):
                yield c
            elif not isinstance(c, (model.Span, model.Text, model.Br)):
                yield from paragraphs(c)

    def shown_in(isd):
        """What shows in `isd`: (id, an, runs), region by region."""
        shown = []
        for region in isd.iter_regions():
            for body in region:
                for p in paragraphs(body):
                    runs = runs_of(p)
                    if runs is not None:
                        shown.append((p.get_id() or "", placement(region, p), runs))
        return shown

    # What shows in each stretch, sample by sample: from the sample's start,
    # and from each change inside it, what ttconv shows then -- its
    # intermediate synchronic document in force, the last at or before.
    stretches, damaged = [], 0
    for start, stop, document in samples:
        try:
            doc = to_model(et.ElementTree(et.fromstring(document)))
        except et.ParseError:
            damaged += 1
            stretches.append((start, stop, []))
            continue
        isds = ISD.generate_isd_sequence(doc)
        points = [start] + [t for t, _ in isds if t > start and (stop is None or t < stop)]
        for k, at in enumerate(points):
            until = points[k + 1] if k + 1 < len(points) else stop
            in_force = [isd for t, isd in isds if t <= at]
            shown = shown_in(in_force[-1]) if in_force else []
            if shown and until is None:
                sys.exit("a TTML sample shows something for ever")
            stretches.append((at, until, shown))
    # Each paragraph's stretches of showing the same, joined; cues in the
    # order they begin, then of beginning.
    cues, showing, order = [], {}, 0
    for begin, end, shown in stretches:
        now = {}
        for key in shown:
            if key in showing and showing[key][1] == begin:
                now[key] = (showing[key][0], end, showing[key][2])
            else:
                now[key] = (begin, end, order)
                order += 1
        for key, (start, stop, o) in showing.items():
            if key not in now:
                cues.append((start, stop, o, key))
        showing = now
    for key, (start, stop, o) in showing.items():
        cues.append((start, stop, o, key))
    cues.sort(key=lambda c: (c[0], c[2]))
    return [(ns(start), ns(stop), key[1], key[2]) for start, stop, _, key in cues], damaged


def write_cues(path, name, cues, damaged):
    lines = [f"# {name}: ttconv's reading of its samples, said as videocodec says it "
             "(generate_subtitle_fixtures.py)."]
    if damaged:
        lines.append(f"damaged {damaged}")
    for start, end, an, runs in cues:
        lines.append(f"cue {start} {end} {an}")
        for r in runs:
            if r[0] == "break":
                lines.append("break")
            else:
                lines.append(f"text {r[2]} {r[3]} {json.dumps(r[1], ensure_ascii=False)}")
    write(path, "\n".join(lines) + "\n")


def make_ttml(name, source, cut=None):
    """NAME.mp4 of the TTML `source`: MP4Box's (`cut` None) -- its own
    rewriting of the document, in samples cut where what shows changes --
    or written here, the whole document in every sample: samples of `cut`
    milliseconds, or ("one") a single sample to the document's last end.
    NAME.cues is ttconv's reading of the MP4's samples, said as the crate
    says it; for samples written here, it is checked to be the document's."""
    from fractions import Fraction

    out = os.path.join(HERE, f"{name}.mp4")
    document = source.encode("utf-8")
    whole, _ = ttml_cues([(Fraction(0), None, document)])
    if cut is None:
        src = os.path.join(HERE, f"{name}.source.ttml")
        write(src, source)
        try:
            mp4box(out, src)
        finally:
            os.remove(src)
    else:
        last = -(-max(end for _, end, _, _ in whole) // 10**6)
        step = last if cut == "one" else cut
        edges = list(range(0, last + step, step))
        stpp_mp4(out, [(a, b, document) for a, b in zip(edges, edges[1:])])
    cues, damaged = ttml_cues(stpp_samples(out))
    if (cues, damaged) != (whole, 0):
        if cut is not None:
            sys.exit(f"{out}: its samples, each the whole document, read otherwise than the document")
        print(f"note: {out}: MP4Box's samples show {len(cues)} cues of the document's {len(whole)}, "
              f"and {damaged} of its samples no XML (see the TTML notes above)")
    write_cues(os.path.join(HERE, f"{name}.cues"), f"{name}.mp4", cues, damaged)
    print("wrote", out, f"{len(cues)} cues")


def ttml_main():
    """The TTML fixtures (`python generate_subtitle_fixtures.py --ttml` makes
    them alone)."""
    make_ttml("ttml", TTML_BASIC)
    make_ttml("ttml_split", TTML_BASIC, cut=2000)
    make_ttml("ttml_timing", TTML_TIMING)
    make_ttml("ttml_timing_one", TTML_TIMING, cut="one")
    make_ttml("ttml_timing_split", TTML_TIMING, cut=1500)
    make_ttml("ttml_regions", TTML_REGIONS)
    make_ttml("ttml_default", TTML_DEFAULT)


def webvtt_main():
    """The WebVTT fixtures, in WebM, Matroska and MP4 (`python
    generate_subtitle_fixtures.py --webvtt` makes them alone)."""
    cues = [("", text) for text in WEBVTT] + WEBVTT_PLACED
    vtt = "WEBVTT\n\n" + "".join(f"{vtt_time(1000 + 1500 * i)} --> {vtt_time(2000 + 1500 * i)}"
                                  f"{' ' + settings if settings else ''}\n{text}\n\n"
                                  for i, (settings, text) in enumerate(cues))
    make("webvtt", "vtt", vtt, "webm")
    make("webvtt_mkv", "vtt", vtt, "mkv", muxer="mkvmerge", answer_from="webvtt")
    # WebVTT in MP4 (`wvtt`), which ffmpeg does not read: MP4Box's, the
    # format's reference implementation, from the same .vtt -- the answer
    # ffmpeg's for the same cues in WebM.
    make("webvtt_mp4", "vtt", vtt, "mp4", muxer="mp4box", answer_from="webvtt")
    overlap = "WEBVTT\n\n" + "".join(f"{ident + chr(10) if ident else ''}{vtt_time(start)} --> {vtt_time(end)}"
                                      f"{' ' + settings if settings else ''}\n{text}\n\n"
                                      for start, end, ident, settings, text in WEBVTT_OVERLAP)
    make("webvtt_overlap", "vtt", overlap, "webm")
    make("webvtt_overlap_mp4", "vtt", overlap, "mp4", muxer="mp4box", answer_from="webvtt_overlap")
    # Fragmented, as DASH and HLS segments carry it: a fragment every two
    # seconds, cues cut at none of them.
    make("webvtt_overlap_fragments", "vtt", overlap, "mp4", muxer="mp4box", answer_from="webvtt_overlap",
         options=("-frag", "2000"))


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

    webvtt_main()
    ttml_main()

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

    shown, sd, cropped = pgs_scenes()
    make_pgs("pgs", PGS_CANVAS, shown)
    make_pgs("pgs_sd", (720, 480), sd)
    make_pgs("pgs_cropped", PGS_CANVAS, cropped, departures={1000, 2000, 4000})
    make_pgs("pgs_damage", PGS_CANVAS, pgs_damage(), drawn=False)
    main_spus, dvd, dvd_departures, grey, pal = vob_scenes()
    make_vobsub("vobsub", VOB_CANVAS, VOB_PALETTE, main_spus)
    make_vobsub("vobsub_dvd", (720, 576), VOB_PALETTE, dvd, departures=dvd_departures)
    make_vobsub("vobsub_grey", VOB_CANVAS, None, grey)
    make_vobsub("vobsub_pal", None, VOB_PALETTE, pal)
    make_vobsub("vobsub_damage", VOB_CANVAS, VOB_PALETTE, [], drawn=False, raw=vob_damage())
    dvb_main()
    make_mixed()


if __name__ == "__main__":
    if "--dvb" in sys.argv:
        dvb_main()
    elif "--webvtt" in sys.argv:
        webvtt_main()
    elif "--ttml" in sys.argv:
        ttml_main()
    else:
        main()
