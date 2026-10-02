## 858. A wallpaper history records what the user chose, not what a timer did

**Date:** 2026-09-17
**Lane:** C
**Decided by:** Claude (autonomous)

**In short:** the desktop keeps a short list of recent wallpapers so that a
"previous wallpaper" command can exist. A running slideshow was adding an entry
every time it changed picture on its own, which filled the list with pictures
nobody had chosen and pushed the user's own choices out of it. Only deliberate
acts are recorded now.

**Context.** `WallpaperHistory` holds twenty entries and exists to support
`go_back` / `go_forward`, which are implemented and tested and have never had a
caller. Nine places recorded into it. Seven were user actions -- picking a
colour, picking a picture, choosing the theme colour, switching to the
time-of-day gradient, stepping a slideshow forwards or backwards by hand,
asking for a random wallpaper. Two were not: `tick_slideshow`, the timer, and
`populate_slideshow_paths`, which is a directory listing arriving.

**What that cost, in numbers rather than in principle.** The default slideshow
interval is thirty seconds and the buffer is twenty entries, so a slideshow
left running replaced the entire history in **ten minutes**. A user who chose a
picture, started a slideshow and came back after lunch would find "previous
wallpaper" offering slideshow frames and nothing they had picked. That is not a
history with a bug in it; it is a different feature wearing the name.

**Options.**

1. **Record everything, and let the caller filter.** Honest about what
   happened, and the navigation could skip non-choices. But every caller would
   need the same filter, and the twenty-entry budget is still spent on frames
   -- a user's choice is evicted whether or not anyone skips it.
2. **Record only user actions** -- the timer and the directory listing stay
   out. The list means one thing, the budget is spent on choices, and no caller
   needs to know the difference.
3. **Two lists**, one of choices and one of everything. Nothing needs the
   second.

**Decision: (2).** The line is between *the user acting* and *the clock
acting*, not between kinds of wallpaper: stepping a slideshow by hand is
recorded, the same slideshow advancing by itself is not.

**Reasoning.**

- *A history is a record of decisions.* The question it answers is "take me
  back to what I had", and the user never had the frame the timer chose --
  they had "a slideshow".
- *The budget is the argument.* Twenty entries is generous for choices and
  meaningless for frames. Option 1 keeps the pollution and only hides it.
- *It makes the unbuilt feature buildable.* The navigation could not sensibly
  be wired while the list was mostly frames, which is a plausible reason nobody
  ever wired it.

**Cost accepted.** Someone who wants to return to a particular slideshow frame
cannot, unless they stepped to it by hand. That is the right trade: the
slideshow itself is the mechanism for moving between frames.

**Pinned by** `a_timed_advance_does_not_enter_the_history` and
`stepping_by_hand_does_enter_the_history` in `gui/desktop/src/wallpaper.rs`.
The first was written because removing the timer's push broke **no** test --
the old behaviour was unpinned, so the obvious "fix" of putting it back would
have gone unnoticed.
