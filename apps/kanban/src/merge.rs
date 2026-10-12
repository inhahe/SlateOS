//! Two windows of the kanban board saving into one file without losing each
//! other's work (design-decisions §1239).
//!
//! A save reads the file first and puts in only what this window changed
//! since it last read or wrote it -- `recordfile::merge`, a three-way merge by
//! id of the base (the boards as this window last read or wrote them), mine
//! (as it has them now) and theirs (as the file has them now).
//!
//! A board is not merged as one record. It holds columns and cards, and two
//! windows working on different cards of one board must each keep their
//! card, which taking the whole board from the later save would not do. So a
//! board both windows still have is merged inside: its own fields, each from
//! this window if it changed it and from the file otherwise; its columns,
//! cards and labels by id; a column's card list and the archived list as
//! lists of ids. Two windows changing the same card at once is the one case
//! where the later save wins, for that card alone.
//!
//! What a merge taken piece by piece can leave is mended after
//! ([`mend`]): a card in no column or in two, a column naming a card that is
//! gone, a card naming a label that is gone.

use std::collections::{HashMap, HashSet};

use crate::{Board, Card, Column, Id, Label};

impl recordfile::Record for Board {
    fn id(&self) -> u64 {
        self.id.0
    }
}

impl recordfile::Record for Column {
    fn id(&self) -> u64 {
        self.id.0
    }
}

impl recordfile::Record for Card {
    fn id(&self) -> u64 {
        self.id.0
    }
}

impl recordfile::Record for Label {
    fn id(&self) -> u64 {
        self.id.0
    }
}

/// An id in a list -- a column's cards, the archived cards -- as a record of
/// its own, so that a list of ids merges as records do: what each window
/// added and took out stands, and an order changed here is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Entry(u64);

impl recordfile::Record for Entry {
    fn id(&self) -> u64 {
        self.0
    }
}

/// A list of ids, merged.
fn merge_ids(base: &[Id], mine: &[Id], theirs: &[Id]) -> Vec<Id> {
    let entries = |list: &[Id]| list.iter().map(|id| Entry(id.0)).collect::<Vec<_>>();
    recordfile::merge(&entries(base), &entries(mine), &entries(theirs))
        .into_iter()
        .map(|entry| Id(entry.0))
        .collect()
}

/// `mine` if this window changed it since `base`, and `theirs` otherwise.
fn pick<T: PartialEq + Clone>(base: &T, mine: &T, theirs: &T) -> T {
    if mine == base {
        theirs.clone()
    } else {
        mine.clone()
    }
}

/// The record with id `id` in `list`.
fn find<R: recordfile::Record>(list: &[R], id: u64) -> Option<&R> {
    list.iter().find(|r| r.id() == id)
}

/// The boards with this window's changes since `base` put into `theirs`.
#[must_use]
pub(crate) fn merge_boards(base: &[Board], mine: &[Board], theirs: &[Board]) -> Vec<Board> {
    let mut boards = recordfile::merge(base, mine, theirs);
    for board in &mut boards {
        let id = board.id.0;
        if let (Some(b), Some(m), Some(t)) = (find(base, id), find(mine, id), find(theirs, id)) {
            *board = merge_board(b, m, t);
        }
    }
    boards
}

/// One board both windows still have, merged inside.
fn merge_board(base: &Board, mine: &Board, theirs: &Board) -> Board {
    let mut columns = recordfile::merge(&base.columns, &mine.columns, &theirs.columns);
    for column in &mut columns {
        let id = column.id.0;
        if let (Some(b), Some(m), Some(t)) = (
            find(&base.columns, id),
            find(&mine.columns, id),
            find(&theirs.columns, id),
        ) {
            *column = Column {
                id: column.id,
                name: pick(&b.name, &m.name, &t.name),
                card_ids: merge_ids(&b.card_ids, &m.card_ids, &t.card_ids),
                wip_limit: pick(&b.wip_limit, &m.wip_limit, &t.wip_limit),
                sort_by: pick(&b.sort_by, &m.sort_by, &t.sort_by),
                collapsed: pick(&b.collapsed, &m.collapsed, &t.collapsed),
            };
        }
    }
    let mut board = Board {
        id: mine.id,
        name: pick(&base.name, &mine.name, &theirs.name),
        columns,
        cards: merge_cards(&base.cards, &mine.cards, &theirs.cards),
        labels: recordfile::merge(&base.labels, &mine.labels, &theirs.labels),
        archived_card_ids: merge_ids(
            &base.archived_card_ids,
            &mine.archived_card_ids,
            &theirs.archived_card_ids,
        ),
        swimlanes_enabled: pick(
            &base.swimlanes_enabled,
            &mine.swimlanes_enabled,
            &theirs.swimlanes_enabled,
        ),
        swimlane_names: pick(
            &base.swimlane_names,
            &mine.swimlane_names,
            &theirs.swimlane_names,
        ),
    };
    mend(&mut board, mine);
    board
}

/// A board's cards, merged by id. A map has no order to keep.
fn merge_cards(
    base: &HashMap<Id, Card>,
    mine: &HashMap<Id, Card>,
    theirs: &HashMap<Id, Card>,
) -> HashMap<Id, Card> {
    let listed = |map: &HashMap<Id, Card>| {
        let mut list: Vec<Card> = map.values().cloned().collect();
        list.sort_by_key(|card| card.id.0);
        list
    };
    recordfile::merge(&listed(base), &listed(mine), &listed(theirs))
        .into_iter()
        .map(|card| (card.id, card))
        .collect()
}

/// Mend what a merge taken piece by piece can leave in `board`, preferring
/// `mine` -- this window's board -- where it says where something goes.
///
/// What is mended is what the boards' reader refuses (`parse_boards`): a
/// column or the archive naming a card the board does not have, and a card
/// in two places. Written so, the file could not be read again, and every
/// window would then refuse to save over it.
///
/// - A label a kept card wears, deleted by the other window, comes back from
///   this window's copy; a label nobody has any more is taken off.
/// - A card's own record says whether it is archived, and an archived card
///   is in no column: one window archiving a card while the other moved it
///   left it in the archive and a column at once.
/// - A column names only cards the board has, and a card is in one column
///   at most: each window moving it somewhere else would put it in two, and
///   this window's place wins -- the later save.
/// - A card that is not archived is in a column: one whose column the other
///   window deleted goes where this window has it, if that column is there,
///   and to the first column otherwise -- or into the archive, where it can
///   be seen and restored, when the board has no column left at all.
/// - The archive names the archived cards there are, each once.
pub(crate) fn mend(board: &mut Board, mine: &Board) {
    let present: HashSet<Id> = board.labels.iter().map(|l| l.id).collect();
    let worn: HashSet<Id> = board
        .cards
        .values()
        .flat_map(|card| card.labels.iter().copied())
        .filter(|id| !present.contains(id))
        .collect();
    board
        .labels
        .extend(mine.labels.iter().filter(|l| worn.contains(&l.id)).cloned());
    let labels: HashSet<Id> = board.labels.iter().map(|l| l.id).collect();
    for card in board.cards.values_mut() {
        card.labels.retain(|id| labels.contains(id));
    }

    // In one column at most, this window's choice first -- and in none if
    // archived, or not on the board at all.
    let open: HashSet<Id> = board
        .cards
        .values()
        .filter(|card| !card.archived)
        .map(|card| card.id)
        .collect();
    let mine_column = |card: Id| {
        mine.columns
            .iter()
            .find(|c| c.card_ids.contains(&card))
            .map(|c| c.id)
    };
    let mut placed: HashMap<Id, Id> = HashMap::new();
    for column in &board.columns {
        for card in column.card_ids.iter().filter(|card| open.contains(card)) {
            let keep = match placed.get(card) {
                None => true,
                Some(_) => mine_column(*card) == Some(column.id),
            };
            if keep {
                placed.insert(*card, column.id);
            }
        }
    }
    for column in &mut board.columns {
        let here = column.id;
        let mut seen = HashSet::new();
        column
            .card_ids
            .retain(|card| placed.get(card) == Some(&here) && seen.insert(*card));
    }

    // Every card that is not archived in a column.
    let mut homeless: Vec<Id> = open
        .into_iter()
        .filter(|card| !placed.contains_key(card))
        .collect();
    homeless.sort_by_key(|id| id.0);
    for card in homeless {
        let target = mine_column(card)
            .filter(|col| board.columns.iter().any(|c| c.id == *col))
            .or_else(|| board.columns.first().map(|c| c.id));
        match target.and_then(|target| board.columns.iter_mut().find(|c| c.id == target)) {
            Some(column) => column.card_ids.push(card),
            None => {
                if let Some(card) = board.cards.get_mut(&card) {
                    card.archived = true;
                }
            }
        }
    }

    let archived: HashSet<Id> = board
        .cards
        .values()
        .filter(|card| card.archived)
        .map(|card| card.id)
        .collect();
    let mut seen = HashSet::new();
    board
        .archived_card_ids
        .retain(|id| archived.contains(id) && seen.insert(*id));
    let mut missing: Vec<Id> = archived
        .into_iter()
        .filter(|id| !board.archived_card_ids.contains(id))
        .collect();
    missing.sort_by_key(|id| id.0);
    board.archived_card_ids.extend(missing);
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use crate::{Color, SortBy};

    /// A board with the columns `names`, and no cards.
    fn board(names: &[&str]) -> Board {
        let mut board = Board::new("Board");
        for name in names {
            board.add_column(name);
        }
        board
    }

    /// A new card `title` at the bottom of column `column`.
    fn card(board: &mut Board, title: &str, column: usize) -> Id {
        board.add_card_to_column(Card::new(title), column).unwrap()
    }

    /// The columns holding `card`, by position.
    fn homes(board: &Board, card: Id) -> Vec<usize> {
        board
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column.card_ids.contains(&card))
            .map(|(at, _)| at)
            .collect()
    }

    /// `board` written out reads back: a merged board must, or no window
    /// could read the file again.
    fn assert_readable(board: &Board) {
        let text = crate::boards_text(std::slice::from_ref(board), 0);
        if let Err(why) = crate::parse_boards(&text) {
            panic!("the merged board does not read back: {why}");
        }
    }

    /// The one board `merge_boards` gives for one board in each.
    fn merged(base: &Board, mine: &Board, theirs: &Board) -> Board {
        let boards = merge_boards(
            std::slice::from_ref(base),
            std::slice::from_ref(mine),
            std::slice::from_ref(theirs),
        );
        assert_eq!(boards.len(), 1);
        boards.into_iter().next().unwrap()
    }

    /// Each window's change to a different card stands, and the column keeps
    /// both.
    #[test]
    fn each_windows_change_to_a_different_card_stands() {
        let mut base = board(&["Todo"]);
        let x = card(&mut base, "X", 0);
        let y = card(&mut base, "Y", 0);
        let mut mine = base.clone();
        mine.cards.get_mut(&x).unwrap().title = String::from("X here");
        let mut theirs = base.clone();
        theirs.cards.get_mut(&y).unwrap().title = String::from("Y there");

        let merged = merged(&base, &mine, &theirs);
        assert_eq!(merged.cards[&x].title, "X here");
        assert_eq!(merged.cards[&y].title, "Y there");
        assert_eq!(merged.columns[0].card_ids, [x, y]);
        assert_readable(&merged);
    }

    /// A board both windows changed keeps each window's change to its own
    /// fields and its column's: this one renamed the board and the column,
    /// the other limited, sorted and folded the column and turned swimlanes
    /// on.
    #[test]
    fn each_windows_change_to_a_boards_own_fields_stands() {
        let base = board(&["Todo"]);
        let mut mine = base.clone();
        mine.name = String::from("Renamed here");
        mine.columns[0].name = String::from("Backlog");
        let mut theirs = base.clone();
        theirs.columns[0].wip_limit = Some(3);
        theirs.columns[0].sort_by = SortBy::Title;
        theirs.columns[0].collapsed = true;
        theirs.swimlanes_enabled = true;
        theirs.swimlane_names = vec![String::from("Lane")];
        theirs
            .labels
            .push(Label::new("New", Color::rgb(0, 0xff, 0)));

        let merged = merged(&base, &mine, &theirs);
        assert_eq!(merged.name, "Renamed here");
        assert_eq!(merged.columns[0].name, "Backlog");
        assert_eq!(merged.columns[0].wip_limit, Some(3));
        assert_eq!(merged.columns[0].sort_by, SortBy::Title);
        assert!(merged.columns[0].collapsed);
        assert!(merged.swimlanes_enabled);
        assert_eq!(merged.swimlane_names, ["Lane"]);
        let labels: Vec<&str> = merged.labels.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(labels, ["New"]);
    }

    /// A card the other window put in a column other than the first is in
    /// that column, beside this window's card in its own.
    #[test]
    fn a_card_the_other_window_added_is_in_its_column() {
        let base = board(&["Todo", "Doing"]);
        let mut mine = base.clone();
        let y = card(&mut mine, "Y", 0);
        let mut theirs = base.clone();
        let x = card(&mut theirs, "X", 1);

        let merged = merged(&base, &mine, &theirs);
        assert_eq!(homes(&merged, x), [1]);
        assert_eq!(homes(&merged, y), [0]);
        assert_readable(&merged);
    }

    /// The archive keeps the order the other window archived in, which is
    /// the order the archive is shown in.
    #[test]
    fn the_archive_keeps_the_other_windows_order() {
        let mut base = board(&["Todo"]);
        // Ids against the order, so an archive put in id order is caught.
        let mut first = Card::new("Archived first");
        first.id = Id(20);
        let mut second = Card::new("Archived second");
        second.id = Id(10);
        let first = base.add_card_to_column(first, 0).unwrap();
        let second = base.add_card_to_column(second, 0).unwrap();
        let mine = base.clone();
        let mut theirs = base.clone();
        assert!(theirs.archive_card(first));
        assert!(theirs.archive_card(second));

        let merged = merged(&base, &mine, &theirs);
        assert_eq!(merged.archived_card_ids, [first, second]);
        assert_readable(&merged);
    }

    /// A label the other window deleted while this one put it on a card
    /// comes back, so the card keeps it.
    #[test]
    fn a_label_deleted_elsewhere_comes_back_for_a_card_that_wears_it() {
        let mut base = board(&["Todo"]);
        base.labels.push(Label::new("Bug", Color::rgb(0xff, 0, 0)));
        let bug = base.labels[0].id;
        let x = card(&mut base, "X", 0);
        let mut mine = base.clone();
        mine.cards.get_mut(&x).unwrap().labels.push(bug);
        let mut theirs = base.clone();
        theirs.labels.clear();

        let merged = merged(&base, &mine, &theirs);
        let labels: Vec<Id> = merged.labels.iter().map(|l| l.id).collect();
        assert_eq!(labels, [bug]);
        assert_eq!(merged.cards[&x].labels, [bug]);
        assert_readable(&merged);
    }

    /// A label this window deleted while the other put it on a card is taken
    /// off the card: no card wears a label the board does not have.
    #[test]
    fn a_label_neither_window_has_is_taken_off_the_card() {
        let mut base = board(&["Todo"]);
        base.labels.push(Label::new("Bug", Color::rgb(0xff, 0, 0)));
        let bug = base.labels[0].id;
        let x = card(&mut base, "X", 0);
        let mut mine = base.clone();
        mine.labels.clear();
        let mut theirs = base.clone();
        theirs.cards.get_mut(&x).unwrap().labels.push(bug);

        let merged = merged(&base, &mine, &theirs);
        assert!(merged.labels.is_empty());
        assert!(
            merged.cards[&x].labels.is_empty(),
            "a card wears a label the board does not have"
        );
    }

    /// One window archived a card while the other moved it: the card is
    /// archived and in no column. It was left in the archive and a column at
    /// once -- a card in two places, which the boards' reader refuses, so no
    /// window could read the file again.
    #[test]
    fn a_card_archived_in_one_window_and_moved_in_the_other_is_only_archived() {
        let mut base = board(&["Todo", "Doing", "Done"]);
        let x = card(&mut base, "X", 0);
        let mut archived = base.clone();
        assert!(archived.archive_card(x));
        let mut moved = base.clone();
        assert!(moved.move_card(x, 0, 2, 0));

        // Whichever window saves last.
        for (mine, theirs) in [(&moved, &archived), (&archived, &moved)] {
            let merged = merged(&base, mine, theirs);
            assert!(merged.cards[&x].archived);
            assert!(
                homes(&merged, x).is_empty(),
                "an archived card is in a column"
            );
            assert_eq!(merged.archived_card_ids, [x]);
            assert_readable(&merged);
        }
    }

    /// A card in two columns after a merge -- each window moved it somewhere
    /// else -- stays in this window's; one this window does not have stays in
    /// the first it is in.
    #[test]
    fn a_card_in_two_columns_stays_in_this_windows() {
        let mut mine = board(&["Todo", "Doing", "Done"]);
        let x = card(&mut mine, "X", 2);
        let mut board = mine.clone();
        board.columns[0].card_ids.push(x);
        let y = card(&mut board, "Y", 0);
        board.columns[1].card_ids.push(y);

        mend(&mut board, &mine);
        assert_eq!(homes(&board, x), [2]);
        assert_eq!(homes(&board, y), [0]);
        assert_readable(&board);
    }

    /// A column names each card the board has once, and no card it does not
    /// have.
    #[test]
    fn a_column_names_each_card_the_board_has_once() {
        let mut mine = board(&["Todo"]);
        let x = card(&mut mine, "X", 0);
        let mut board = mine.clone();
        // No card has id 0: `recordfile::fresh_id` never gives it.
        board.columns[0].card_ids = vec![x, Id(0), x];

        mend(&mut board, &mine);
        assert_eq!(board.columns[0].card_ids, [x]);
        assert_readable(&board);
    }

    /// A card left in no column -- the other window deleted its column, or
    /// took it out of the one it was in -- goes where this window has it if
    /// that column is there, and to the first column if not.
    #[test]
    fn a_card_in_no_column_goes_where_this_window_has_it_or_to_the_first() {
        let mut mine = board(&["Todo", "Doing", "Gone"]);
        let x = card(&mut mine, "X", 1);
        let y = card(&mut mine, "Y", 2);
        let mut board = mine.clone();
        board.columns[1].card_ids.clear();
        board.columns.remove(2);

        mend(&mut board, &mine);
        assert_eq!(homes(&board, x), [1]);
        assert_eq!(homes(&board, y), [0]);
        assert!(!board.cards[&x].archived && !board.cards[&y].archived);
        assert_readable(&board);
    }

    /// A card with no column left on its board to go to is archived, where
    /// it can be seen and restored, rather than kept where nothing shows it.
    #[test]
    fn a_card_on_a_board_with_no_column_left_is_archived() {
        let mut mine = board(&["Todo"]);
        let x = card(&mut mine, "X", 0);
        let mut board = mine.clone();
        board.columns.clear();

        mend(&mut board, &mine);
        assert!(board.cards[&x].archived);
        assert_eq!(board.archived_card_ids, [x]);
        assert_readable(&board);
    }

    /// The archive names each archived card the board has, once: not a card
    /// that is not archived or not there, and not one twice.
    #[test]
    fn the_archive_names_each_archived_card_once() {
        let mut mine = board(&["Todo"]);
        let x = card(&mut mine, "X", 0);
        let y = card(&mut mine, "Y", 0);
        let z = card(&mut mine, "Z", 0);
        assert!(mine.archive_card(x));
        assert!(mine.archive_card(z));
        let mut board = mine.clone();
        // Z archived and not listed; Y listed and not archived.
        board.archived_card_ids = vec![x, y, Id(0), x];

        mend(&mut board, &mine);
        assert_eq!(board.archived_card_ids, [x, z]);
        assert_eq!(homes(&board, y), [0]);
        assert_readable(&board);
    }
}
