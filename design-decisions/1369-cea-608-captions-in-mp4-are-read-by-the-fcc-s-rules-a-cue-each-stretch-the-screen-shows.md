## 1369. CEA-608 captions in MP4 are read by the FCC's rules, a cue each stretch the screen shows

**Date:** 2026-10-05
**Lane:** F
**Decided by:** Claude (autonomous)

**In short:** American television's closed captions (CEA-608) are often
kept in MP4 files alongside the picture: QuickTime, broadcast recorders and
iTunes episodes keep them as a caption track. SlateOS's video library
refused that track. It reads it now. The two programs that read such
captions -- FFmpeg, and CCExtractor, the usual caption extractor -- each
show some captions otherwise than a television does: FFmpeg ignores
backspaces, mixes a second caption channel into the first, and puts every
caption at the top of the picture. So this one follows the FCC's own rules
for caption decoders, and each test file is checked against whichever of
the two programs is right about what that file holds.

### How the format carries captions

Each frame of video carries two bytes of captions. Commands among them say
where on a 15-row, 32-column grid the next characters go, and how they
appear: *pop-on* (a caption built out of sight, then shown whole), *roll-up*
(lines scrolling up from the bottom, as live captioning does) or *paint-on*
(characters appearing where they are sent). In MP4, each sample is a frame's
bytes, in a `cdat` atom for the first field (channels CC1 and CC2) and
`cdt2` for the second.

### What was decided

1. **The rules: 47 CFR 79.101**, the FCC's requirements for decoders, for
   every command; CTA-608-E's extended characters from McPoodle's SCC tools,
   whose tables name each one in Unicode. Where both external readers
   depart from the rules, the rules win:
   - a character failing parity is a solid block (FFmpeg drops it;
     CCExtractor ignores parity);
   - a control code is ignored as redundant only when repeated in the very
     next pair (CCExtractor ignores one repeated pairs apart).
2. **Field 1's first channel, CC1**: the captions a television shows unless
   told otherwise. Another channel's commands, and what follows them, are
   not shown, nor text mode's (FFmpeg mixes CC2 into CC1's screen).
3. **A cue is a stretch of the screen.** It runs from a command that
   changes what shows (End of Caption, Erase Displayed Memory, a roll-up
   Carriage Return, Roll-Up erasing a caption, a PAC moving roll-up's
   window), or from the first character on a blank screen, to the next such
   command, or to typing that leaves the screen blank. Its text is the
   screen at the stretch's end, so a roll-up line or a paint-on caption is
   shown whole from when it began.
   - This is FFmpeg's grouping, but no stretch begins before there is
     something to show. FFmpeg begins a roll-up line at the Carriage Return
     before it, the screen still blank.
   - A command that leaves the screen as it was ends nothing: captioners
     send Roll-Up before every line.
4. **Said in SRT** as every format here:
   - each row showing anything is a line, top to bottom;
   - an empty cell before or between characters is a no-break space, so a
     row keeps its column;
   - the whole is in `<font face="Monospace">`, the grid's face, as FFmpeg
     writes it;
   - colour, italics and underline are tags;
   - `{\anN}` puts it at the left of the third of the picture its rows'
     middle falls in. FFmpeg puts every caption at the top left.
5. **A seek reads from a minute before** the time asked for, the decoder
   started afresh, so that a caption showing at the time is there when it
   was sent within the minute. A television tuned in shows nothing until
   the next caption.

### Alternatives

| | For | Against |
|---|---|---|
| **The FCC's rules, checked against both readers (chosen)** | What a television shows; every rule has its text to point at | No single external oracle: each fixture says which reader it is checked against, and the rules alone decide the few things neither keeps |
| FFmpeg's output, with departures | One oracle, as the other formats have | Departures for Backspace, channels, PACs, parity, placement, tables and paint-on timing: most of what a caption does |
| CCExtractor's output | Right about most commands | Groups roll-up lines otherwise, ignores parity, its own glyphs for quotes and corners, and crashes reading every caption MP4 tried |
| A cue for every change, as a television updates its screen | Exactly what a television shows | A cue per character pair while captions are typed; the player draws text, not a terminal |

**Held to:** seven fixtures written box by box, their answers from a
decoder in the generator (`Cea608`) written from the rules. Each is checked
on cue times and text against FFmpeg (pop-on, styles, roll-up) or
CCExtractor (pop-on, every character, paint-on: Backspace, Delete to End of
Row, tab offsets, PACs), with each departure printed and its reason given:
- FFmpeg begins a roll-up line at a blank screen.
- Six glyphs CCExtractor writes otherwise.

Two fixtures, `cea608_modes` (switching style, a window moved) and
`cea608_rules` (parity, repeats, channels, text mode, a full row), are held
to the rules alone.
