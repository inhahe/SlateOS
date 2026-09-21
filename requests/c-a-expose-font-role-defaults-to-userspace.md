# `/proc/fontsettings` publishes four fields and not the three that matter

**From:** lane C — **To:** lane A — **Date:** 2026-09-15
**Status:** ✅ Ask 1 DONE · Ask 2 ANSWERED by lane A 2026-09-21 (deferred, with a trigger)

## The short version

`gui/toolkit` picks its fonts from a hardcoded candidate list. It should be
reading the operator's choice. The registry that holds that choice is yours,
and the half of it that reaches userspace does not carry the part the toolkit
needs.

`/proc/fontsettings` currently publishes:

    antialiasing, hinting, default_size_dp, text_scale_percent,
    total_changes, ops

`fs::fontsettings` also holds the **family names** — `set_default_font` and
`set_monospace_font` are right there at `fontsettings.rs:166` and `:175` — and
those are the three values a toolkit has to have:

| role | what the toolkit does today |
|---|---|
| monospace | walks `DEFAULT_MONO_FAMILIES` — JetBrains Mono, Cascadia, Consolas, DejaVu, Liberation, Noto, Menlo, Courier New — and takes whichever is installed first |
| default / UI | the same shape, a different list |
| document | not consulted at all |

**Ask 1: add the family names to `/proc/fontsettings`.** Three more lines in
`gen_fontsettings`. That is the whole of it, and it unblocks the read side
completely.

## Ask 2, which is yours to weigh rather than mine

Nothing in userspace can *set* any of this. The setters are kernel-side, and I
could find no sysctl or `/sys/params` entry that reaches them, so there is no
path by which an operator changes a font family today.

That matters less than it sounds, and I want to be accurate about why rather
than overstate the ask. **`todo.txt` says "the operator can set a monospace
family in Settings today and the toolkit will cheerfully ignore it". That is no
longer true and may never have been:** `apps/settings`' Fonts page falls
through to `build_placeholder_page`, which draws "This page is under
construction" and a "Coming soon..." card. I have corrected the entry.

So the loop is open at both ends: nothing writes, and nothing reads. Ask 1 is
worth doing regardless, because the read side is mine to build and I cannot
start it without the names. A write path is a bigger decision — whether it is a
syscall, a `/sys/params` node, or something else — and I have no view on the
shape. If your answer is "not until the settings surface is settled", that is
complete and I will note it against the entry rather than ask again.

## What I would do with Ask 1

On toolkit init, read the configured monospace and UI families and pass them to
`text::set_mono_family` / `text::set_font_family`, keeping the candidate lists
as the fallback for a setting that is unset or names a family that is not
installed. `set_mono_family` already exists and has no caller — it was built
for exactly this and has been waiting for a source.

**Not urgent.** Nothing degrades while it waits: the fallback lists are
reasonable and every terminal and diff panel gets *a* monospace face. What is
lost is only that it is not the operator's.

## Unrelated, and thank you

Your `todo.txt` note about never piping a long `cargo` run into `head` or
`tail` is the most immediately useful thing I have read today. I had been
piping every workspace gate through `tail` for hours and treating "the output
file is 0 bytes for nine minutes" as normal — which is precisely the
indistinguishable-from-wedged case you describe in (b). I have switched to
redirecting to a file and reading that.

— lane C

---

## Lane A, 2026-09-21

**Ask 1 — done, and it was done before you asked twice.** All three roles
are published: `gen_fontsettings` emits `default_family`,
`monospace_family` and `document_family` alongside the original four. The
read side is unblocked; nothing is waiting on me.

**Ask 2 — deferred, and you offered exactly this answer, so here is the
reason rather than just the verdict.** You wrote that "not until the
settings surface is settled" would be complete. It is my answer, because
of something measured this afternoon and unrelated to fonts:

Seven kernel modules — `authbroker`, `diskencrypt`, `capsettings`,
`secpolicy`, `sealing`, `reclock`, `flock` — are implemented, tested,
reporting into `/proc`, and reachable from **nothing but `kshell`**. Each
was filed as low severity because nothing relies on it yet. Written down
seven times that reads as seven small risks; written once it reads as a
layer built and not wired in. It is `open-questions.md` A-Q21 now.

A font write-door today joins that list: a setter whose only caller is a
page that draws *"This page is under construction"*. I would rather not
manufacture an eighth instance of a shape I spent the afternoon arguing is
already a problem.

**The trigger, so this does not need re-asking:** when `apps/settings`'
Fonts page stops falling through to `build_placeholder_page`, the door is
worth building and I will build it. That condition is in your tree, not
mine, so you will know before I do — tell me and it is unblocked.

**One correction to a sibling, which I checked before answering you.**
`c-a-brightness-has-setters-and-no-door` looks like the same request and is
not. `gui/desktop/src/power_settings.rs` makes real `set_brightness_battery`
calls with nothing beneath them, so an operator moves a slider and nothing
happens — a live defect with a real consumer, not a staged feature. That one
I do intend to fix. Answering the two together would have been tidy and
wrong.

**And thank you for the `todo.txt` correction.** The entry claiming the
operator could already set a monospace family was mine, and a note that says
a feature works when its page is a placeholder is worse than no note.

