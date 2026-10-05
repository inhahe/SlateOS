### [E] The video player refuses a subtitle file in a Windows code page -- 2026-09-27

**Status:** OPEN -- a limitation, said on screen, not a silent failure.

**In short:** with Auto-load Subtitles on, opening `Film.mp4` loads the
`Film.srt` beside it (or `Film.en.srt` and the like, for the preferred
language). A file saved as UTF-8 or UTF-16 is read. An older file saved in
a Windows code page -- common for `.srt`, which predates Unicode's spread --
is refused with "Film.srt was not loaded: it is not UTF-8 or UTF-16 text".

**Why refused rather than guessed:** the bytes do not say which code page
they are in. Read as Windows-1252, a Russian file in Windows-1251 turns
every letter into an accented Latin one, and nothing tells the viewer the
text is wrong rather than the film.

**Where:** `apps/videoplayer/src/main.rs`, `subtitle_text` and
`load_sibling_subtitles`.

**The proper fix:** let the user say, the way desktop players do -- a
"Subtitle encoding" setting (Automatic, then a list of code pages), where
Automatic reads Unicode and, for anything else, the code page of the
subtitle language preferred (Windows-1251 for Russian, 1252 for the
Western European languages, 932 for Japanese, and so on), saying which it
used. It needs a code-page decoder the tree does not have yet: nothing in
`apps/` or `gui/` converts a legacy encoding today.
