"""Mutation test for System Restore's real restore points.

Breaks one piece of production code at a time and checks that the test which
claims to cover it is the one that fails.  A test that passes against a broken
program is not testing the program.

The table covers what changed on 2026-09-27, when the window stopped showing
five invented restore points and began keeping real ones in `apps/snapstore`:
what a restore point holds and how the list of them is kept, what a restore
does (keep the folder first, then make it match), what deleting does to the
tree, the schedule and its retention policy, and the window's refusals -- a
damaged list is never written over, running work is never abandoned, a
close waits for it, a locked point is not deleted.

Run it with no arguments to sweep everything, or with substrings of the
mutation names to run only those.
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))

from mutation_harness import sweep  # noqa: E402  (path set above)

SRC = Path(__file__).parent / "src"

RESTORE = "a_restore_puts_the_files_back_and_keeps_the_ones_it_replaced"
REFUSED = "a_list_that_is_not_one_is_refused"
KEPT = "every_component_says_whether_it_can_be_kept"
TAKEN = "a_restore_point_is_taken_kept_and_listed"
DELETE = "deleting_a_restore_point_keeps_the_branches"
RETENTION = "retention_removes_only_scheduled_restore_points"
FIRST = "a_first_run_opens_on_nothing"
DAMAGED = "a_damaged_list_is_never_written_over"
CLOSE = "closing_waits_for_the_work"
OVERLAY = "the_overlay_holds_until_the_work_has_ended"
LOCKED = "a_locked_restore_point_is_not_deleted"
SCHEDULE = "the_schedule_is_changed_from_its_view"

POINTS = [
    (
        "a restore leaves what was added since",
        "                        mirror: true,",
        "                        mirror: false,",
        [RESTORE],
    ),
    (
        "a list is read in part",
        '        points.push(point_from_json(v).ok_or_else(|| bad("a point is incomplete"))?);',
        "        if let Some(p) = point_from_json(v) {\n            points.push(p);\n        }",
        [REFUSED],
    ),
    (
        "a current point not in the list is accepted",
        "    if current.is_some_and(|id| tree.get_snapshot(id).is_none()) {",
        "    if false {",
        [REFUSED],
    ),
    (
        "the desktop is captured as a component of its own",
        "            Self::DesktopConfig => Err(KEPT_WITH_PROGRAMS),",
        "            Self::DesktopConfig => loc.settings.clone().ok_or(NO_HOME),",
        [KEPT],
    ),
]

MAIN = [
    (
        "a new restore point hangs from nothing",
        "            .filter(|id| self.manager.tree.get_snapshot(*id).is_some());\n"
        "        let components = captured.stored.keys().copied().collect();",
        "            .filter(|_| false);\n"
        "        let components = captured.stored.keys().copied().collect();",
        [TAKEN],
    ),
    (
        "deleting keeps no branches",
        "        for child in tree.children_of(id).to_vec() {",
        "        for child in Vec::<u64>::new() {",
        [DELETE],
    ),
    (
        "retention removes restore points somebody took",
        "            .filter(|s| s.snapshot_type == SnapshotType::Scheduled)\n",
        "",
        [RETENTION],
    ),
    (
        "retention removes the current restore point",
        ".filter(|id| Some(*id) != self.current && self.tree.children_of(*id).is_empty())",
        ".filter(|id| self.tree.children_of(*id).is_empty())",
        [RETENTION],
    ),
    (
        "a schedule that is off still runs",
        "        if !self.enabled {\n            return false;\n        }\n",
        "",
        [FIRST],
    ),
    (
        "the list is not saved",
        "        match points::save(&store, &self.manager) {",
        "        match Ok::<(), std::io::Error>(()) {",
        [TAKEN],
    ),
    (
        "a list that did not read is written over",
        "        if let Some(why) = &self.load_error {\n            return Some(format!(",
        "        if let Some(why) = None::<&String> {\n            return Some(format!(",
        [DAMAGED],
    ),
    (
        "a close does not wait for the work",
        "            if self.work.is_some() {\n                self.close_when_done = true;",
        "            if false {\n                self.close_when_done = true;",
        [CLOSE],
    ),
    (
        "running work can be abandoned",
        "            if progress.complete && matches!(key.key, Key::Escape | Key::Enter) {",
        "            if matches!(key.key, Key::Escape | Key::Enter) {",
        [OVERLAY],
    ),
    (
        "a locked restore point is deleted",
        "        } else if point.locked {",
        "        } else if false {",
        [LOCKED],
    ),
    (
        "Space does nothing in the Schedule view",
        "                Key::Space => return self.schedule_control(ScheduleControl::Toggle),\n",
        "",
        [SCHEDULE],
    ),
]

TABLES = {
    "points.rs": POINTS,
    "main.rs": MAIN,
}

if __name__ == "__main__":
    only = sys.argv[1:]
    names = [name for rows in TABLES.values() for name, *_ in rows]
    unmatched = [o for o in only if not any(o in n for n in names)]
    if unmatched:
        print(f"{len(unmatched)} filter(s) name no row in any table:")
        for o in unmatched:
            print(f"  {o!r}")
        raise SystemExit(2)
    worst = 0
    for file, rows in TABLES.items():
        mine = [o for o in only if any(o in name for name, *_ in rows)]
        if only and not mine:
            continue
        print(f"\n######## {file} ########")
        worst = max(worst, sweep(SRC / file, rows, "systemrestore", timeout=900, only=mine or None))
    raise SystemExit(worst)
