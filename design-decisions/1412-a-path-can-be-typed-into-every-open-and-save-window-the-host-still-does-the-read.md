## 1412. A path can be typed into every Open and Save window; the host still does the reading, and says yes or no

**Date:** 2026-09-27 &middot; **Decided by:** Claude (autonomous) &middot; **Lane:** C

**In short:** The Open and Save window every program uses showed its folder
as plain text nobody could edit, so the only way anywhere was clicking. Its
address bar is now the toolkit's own: click a folder in the path to go there,
or click past the path (or press Ctrl+L, or type `/` where there is no name
field) and type one, with the names in the folder offered as you type. The
window still reads nothing from the disk itself -- the program behind it does,
as it already did for the listing -- so the window asks, and the program
answers. A typed folder that is not there is refused, and everything stays
where it was, the typed text in a red edge to be corrected.

| Question | Chosen | The alternative | Why |
|---|---|---|---|
| Who reads the folder for completions, and checks a typed one exists | the host: `take_completion_request` / `set_completions`, and a `NavigatedTo` answered by `set_entries` or the new `refuse_navigation`; `FilePicker` does all of it | the dialog, calling `read_dir` itself | the dialog does no I/O by design (tests without a disk; a host decides what it may read), and the listing already works this way -- a second channel for the same kind of answer would be a second protocol |
| A new `DialogAction` for "is there a folder here?" | no: the existing `NavigatedTo`, with a way to take it back | a variant such as `Validate(path)` | seven programs -- the desktop's Run box among them -- match `DialogAction` exhaustively, so a variant would break every one of them in one commit across two lanes; `refuse_navigation` is additive, and a host that never calls it behaves as before |
| A refused navigation | undone entirely -- path, both histories, selection, scroll | left in place, listing nothing | an empty listing under the name of a folder that is not there reads as a folder that is there and empty; the same now holds for a shortcut to a missing folder, which used to show one |
| Which keys start a path | Ctrl+L, and `/` where no name is being typed (Open, choose-a-folder) | `/` everywhere, or Ctrl+L only | in a Save window `/` has always gone to the name field, and taking it would change what a typed name means; elsewhere a `/` has nowhere else to go and is how a path starts |
| Narrowing the completions | the bar narrows the host's whole-folder answer, case aside, hidden names only after a dot | the host narrows | `design.txt` asks tab-completion to match in any case, and one place deciding it means the explorer and every dialog match alike; the host's answer then stays right while the typing goes on |
| A typed path's crumbs | shown only once the host has listed the folder | shown on Enter | the bar used to show them on Enter, so a refused path left crumbs of a folder nobody was in above the listing of the one they were |

`FileDialog` keeps what a navigation changed until the host answers it
(`NavigationUndo`), and lets it go on `set_entries`.
