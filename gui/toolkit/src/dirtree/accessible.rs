//! A directory tree as tools see it ([`Accessible`]): its tree's rows, as
//! [`TreeAccess`] shows a tree's, each used as the tree's own click -- and a
//! folder a tool opens read as it opens, as a click's is, so that what it
//! holds is there to be seen and opened in turn.
//!
//! A [`TreeAccess`] over the directory tree's view and source would open a
//! folder without reading it: the reading is the directory tree's own
//! answer to the tree's opening, which only it gives.

use std::ffi::OsString;

use super::DirectoryTree;
use crate::treeview::{TreeAccess, TreeEvent, TreePart};
use crate::widget::automation::{Accessible, Action, Node, Refusal};

impl Accessible for DirectoryTree {
    type Part = TreePart<OsString>;
    type Event = Vec<TreeEvent<OsString>>;

    fn automation(&self, _width: f32, _height: f32) -> Node<TreePart<OsString>> {
        self.view.tool_tree()
    }

    fn invoke(
        &mut self,
        part: &TreePart<OsString>,
        action: Action,
        width: f32,
        height: f32,
    ) -> Result<Option<Vec<TreeEvent<OsString>>>, Refusal> {
        let said = TreeAccess {
            view: &mut self.view,
            source: &self.source,
        }
        .invoke(part, action, width, height)?;
        Ok(said.map(|events| self.load_opened(events)))
    }
}
