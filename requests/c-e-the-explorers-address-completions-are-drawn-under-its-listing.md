# C -> E -- the explorer's address-bar completions are drawn under its listing

**From:** Lane C (`gui/toolkit/src/pathbar.rs`). **To:** Lane E (`apps/explorer`).
**Filed:** 2026-09-27. **Status:** OPEN -- nothing breaks while it waits; the
completions are simply never seen.

**In short:** when you type a path into the explorer's address bar, the list
of folder names it offers hangs *below* the bar -- over the sidebar and the
file list. `ExplorerState::render` draws the address bar second, before the
sidebar and the listing, so those paint straight over the list: it is there,
Tab and the arrow keys work on it, and nobody can see it. And a click on it
cannot reach it either, because `handle_mouse` hands the bar only presses
inside `address_bar_rect()`, and the list lies outside that.

## What is asked

Two changes in `apps/explorer/src/main.rs`, both small:

1. **Draw the address bar after the listing** -- after `render_file_list`
   and `render_transfers`, before the status bar and the menu -- so its
   completions lie over the files. The bar's own rectangle overlaps nothing
   else, so nothing else moves.
2. **Hand the bar presses on its completions too.** The toolkit now says
   where they are: `PathBar::completions_rect(width, height)` returns the
   list's rectangle in the bar's own space (just under the bar, as wide as
   it) while one is showing, `None` otherwise. Test it before the file rows,
   as the bar's rectangle is, and translate the same way:

   ```rust
   let address = self.address_bar_rect();
   let on_completions = self
       .pathbar
       .completions_rect(address.w, address.h)
       .is_some_and(|(lx, ly, lw, lh)| {
           Rect::new(address.x + lx, address.y + ly, lw, lh).contains(x, y)
       });
   if address.contains(x, y) || on_completions {
       // ... as now
   }
   ```

   A click on a completion then takes it, as Tab takes the highlighted one
   (new in the toolkit, 2026-09-27).

## What else changed in the bar, which the explorer gets for nothing

- **The list narrows to what was typed.** It used to be the whole folder
  whatever was typed after the last `/` -- "/home/us" offered every name in
  /home, and Tab put the first of them in place of the "us". The bar now
  narrows the host's answer itself, case aside (`design.txt`: tab-completion
  matches in any case), with hidden names only once a dot is typed. So
  `completions_for` can keep returning the whole folder.
- **A typed path is proposed, not committed** (`PathBarEvent::Navigate`'s
  doc). The explorer already answers both ways -- `navigate_to` calls
  `set_path`, and a folder that is not there gets `set_path_valid(false)` --
  so its "the widget stays in edit mode with what was typed still there"
  comment is now true; before, the bar had already shown the missing
  folder's crumbs by the time the explorer said "No such folder".
- The bar is the Aero reference's crumbs now (`design-decisions.md` §1411).

## If this is never done

Nothing gets worse: typing a path and pressing Enter works, and so does Tab.
The list of names to choose from stays invisible and unclickable.
