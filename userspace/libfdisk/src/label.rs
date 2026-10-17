//! `label.c`: the label drivers, and the operations that go through them.
//!
//! Every context holds one [`Label`] per driver, so that a driver's
//! settings (GPT's "minimize", DOS's "compatible") survive switching to
//! another label and back. One of them is current; [`Context`]'s
//! label-level operations are the current driver's.
//!
//! A driver's operations are its [`LabelOps`]: a static table of function
//! pointers, as in C, `None` where the driver lacks the operation.

use crate::context::Context;
use crate::errno::{EINVAL, ENOSYS};
use crate::field::{FIELDFL_DETAIL, FIELDFL_EYECANDY, Field};
use crate::item::{ItemData, LabelItem};
use crate::partition::Partition;
use crate::parttype::{PartType, Shortcut};
use crate::{Geometry, LABEL_FL_REQUIRE_GEOMETRY, LabelType, fieldtype};

/// What `locate` answers: the item's name, its offset in bytes and its
/// size.
pub type Located = (&'static str, u64, usize);

/// `locate`'s signature.
pub type LocateFn = fn(&mut Context, i32) -> Result<Located, i32>;
/// `set_id`'s signature.
pub type SetIdFn = fn(&mut Context, Option<&[u8]>) -> i32;
/// `add_part`'s signature.
pub type AddPartFn = fn(&mut Context, Option<&mut Partition>, Option<&mut usize>) -> i32;

/// `struct fdisk_label_operations`: one driver's operations, each `None`
/// where the driver has no such operation -- C's NULL pointer, which the
/// generic layer answers with `-ENOSYS`, and checks before it acts.
#[derive(Clone, Copy)]
pub struct LabelOps {
    /// `probe`: 1 when the device holds this label, 0 when not, <0 on error.
    pub probe: Option<fn(&mut Context) -> i32>,
    /// `write`: the in-memory table to the device.
    pub write: Option<fn(&mut Context) -> i32>,
    /// `verify`: 0, or the number of problems found.
    pub verify: Option<fn(&mut Context) -> i32>,
    /// `create`: a new empty table, in memory.
    pub create: Option<fn(&mut Context) -> i32>,
    /// `locate (cxt, n, ...)`: where the `n`th part of the label is on the
    /// device: `Ok` with it, `Err(1)` past the last, `Err(<0)` on error.
    pub locate: Option<LocateFn>,
    /// `reorder`: the partitions sorted by start. 0, 1 when already sorted.
    pub reorder: Option<fn(&mut Context) -> i32>,
    /// `get_item`: a label item. 0, 1 for an item the label lacks, 2 past
    /// the last.
    pub get_item: Option<fn(&mut Context, &mut LabelItem) -> i32>,
    /// `set_id`: the disk identifier, from the string or asked for.
    pub set_id: Option<SetIdFn>,
    /// `add_part (cxt, pa, &partno)`: a new partition after `pa`'s
    /// template (asking for what it leaves out); `partno` receives its
    /// number.
    pub add_part: Option<AddPartFn>,
    /// `del_part`.
    pub del_part: Option<fn(&mut Context, usize) -> i32>,
    /// `get_part (cxt, n, pa)`: partition `n` into `pa`.
    pub get_part: Option<fn(&mut Context, usize, &mut Partition) -> i32>,
    /// `set_part (cxt, n, pa)`: partition `n` changed as `pa` says.
    pub set_part: Option<fn(&mut Context, usize, &Partition) -> i32>,
    /// `part_is_used`: 1 or 0.
    pub part_is_used: Option<fn(&mut Context, usize) -> i32>,
    /// `part_toggle_flag`.
    pub part_toggle_flag: Option<fn(&mut Context, usize, u64) -> i32>,
    /// `reset_alignment`: the label's first and last LBA and grain.
    pub reset_alignment: Option<fn(&mut Context) -> i32>,
    /// `deinit`: the label's private data back to its state before any
    /// probe.
    pub deinit: Option<fn(&mut Label)>,
}

/// A driver's private data (what C keeps past `struct fdisk_label` in the
/// driver's own struct).
#[derive(Debug)]
pub enum LabelData {
    /// No private data.
    None,
    /// The DOS driver's.
    Dos(Box<crate::dos::DosLabel>),
    /// The GPT driver's.
    Gpt(Box<crate::gpt::GptLabel>),
    /// The BSD driver's.
    Bsd(Box<crate::bsd::BsdLabel>),
    /// The SGI driver's.
    Sgi(Box<crate::sgi::SgiLabel>),
}

/// The drivers `fdisk_new_context` makes, in probing order.
#[must_use]
pub fn all_drivers() -> Vec<Label> {
    vec![
        crate::gpt::new_label(),
        crate::dos::new_label(),
        crate::bsd::new_label(),
        crate::sgi::new_label(),
        crate::sun::new_label(),
    ]
}

/// The driver a nested context of type `id` gets.
#[must_use]
pub fn nested_driver(id: LabelType) -> Option<Label> {
    match id {
        LabelType::Dos => Some(crate::dos::new_label()),
        LabelType::Bsd => Some(crate::bsd::new_label()),
        _ => None,
    }
}

/// `struct fdisk_label`: a driver, and its in-memory state.
pub struct Label {
    /// "dos", "gpt", "bsd", "sgi", "sun".
    pub name: &'static str,
    /// Which driver.
    pub id: LabelType,
    /// The types it knows.
    pub parttypes: &'static [PartType],
    /// The short names it accepts for them.
    pub parttype_cuts: &'static [Shortcut],
    /// The most partitions the label can hold now.
    pub nparts_max: usize,
    /// How many are in use.
    pub nparts_cur: usize,
    /// `FDISK_LABEL_FL_*`.
    pub flags: i32,
    /// The smallest geometry the label accepts.
    pub geom_min: Geometry,
    /// The largest.
    pub geom_max: Geometry,
    /// The in-memory table differs from the device's.
    pub changed: bool,
    /// Never probed for, never switched to.
    pub disabled: bool,
    /// The columns a partition listing can have.
    pub fields: &'static [Field],
    /// The operations.
    pub op: &'static LabelOps,
    /// The private state.
    pub data: LabelData,
}

impl std::fmt::Debug for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Label")
            .field("name", &self.name)
            .field("nparts_max", &self.nparts_max)
            .field("nparts_cur", &self.nparts_cur)
            .field("changed", &self.changed)
            .finish_non_exhaustive()
    }
}

impl Label {
    /// `fdisk_label_get_name`.
    #[must_use]
    pub fn get_name(&self) -> &'static str {
        self.name
    }

    /// `fdisk_label_get_type`.
    #[must_use]
    pub fn get_type(&self) -> LabelType {
        self.id
    }

    /// `fdisk_label_require_geometry`.
    #[must_use]
    pub fn require_geometry(&self) -> bool {
        self.flags & LABEL_FL_REQUIRE_GEOMETRY != 0
    }

    /// `fdisk_label_get_field (lb, id)`.
    #[must_use]
    pub fn get_field(&self, id: i32) -> Option<&'static Field> {
        self.fields.iter().find(|f| f.id == id)
    }

    /// `fdisk_label_get_field_by_name (lb, name)`: in any case.
    #[must_use]
    pub fn get_field_by_name(&self, name: &[u8]) -> Option<&'static Field> {
        self.fields
            .iter()
            .find(|f| crate::utils::strcasecmp(f.name.as_bytes(), name) == 0)
    }

    /// `fdisk_label_set_changed`.
    pub fn set_changed(&mut self, changed: bool) {
        self.changed = changed;
    }

    /// `fdisk_label_is_changed`.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.changed
    }

    /// `fdisk_label_set_disabled`.
    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    /// `fdisk_label_is_disabled`.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// `fdisk_label_get_geomrange_sectors`: -ENOSYS when the label sets
    /// no range.
    pub fn get_geomrange_sectors(&self) -> Result<(u64, u64), i32> {
        if self.geom_min.sectors == 0 {
            return Err(-ENOSYS);
        }
        Ok((self.geom_min.sectors, self.geom_max.sectors))
    }

    /// `fdisk_label_get_geomrange_heads`.
    pub fn get_geomrange_heads(&self) -> Result<(u32, u32), i32> {
        if self.geom_min.heads == 0 {
            return Err(-ENOSYS);
        }
        Ok((self.geom_min.heads, self.geom_max.heads))
    }

    /// `fdisk_label_get_geomrange_cylinders`.
    pub fn get_geomrange_cylinders(&self) -> Result<(u64, u64), i32> {
        if self.geom_min.cylinders == 0 {
            return Err(-ENOSYS);
        }
        Ok((self.geom_min.cylinders, self.geom_max.cylinders))
    }
}

/// `fdisk_deinit_label (lb)`.
pub fn deinit_label(lb: &mut Label) {
    if let Some(f) = lb.op.deinit {
        f(lb);
    }
}

impl Context {
    /// The current label, if any.
    #[must_use]
    pub fn label_ref(&self) -> Option<&Label> {
        self.labels.get(self.label?)
    }

    /// The current label, if any, for changing.
    pub fn label_mut(&mut self) -> Option<&mut Label> {
        let i = self.label?;
        self.labels.get_mut(i)
    }

    /// The current label's operations.
    fn op(&self) -> Option<&'static LabelOps> {
        self.label_ref().map(|l| l.op)
    }

    /// `fdisk_probe_labels (cxt)`: tries each enabled driver in order and
    /// switches to the first that finds its label. 0, or 1 when none did.
    pub fn probe_labels(&mut self) -> i32 {
        self.label = None;
        for i in 0..self.labels.len() {
            let Some(lb) = self.labels.get(i) else {
                continue;
            };
            let Some(probe) = lb.op.probe else { continue };
            if lb.disabled {
                continue;
            }
            let org = self.label;
            self.label = Some(i);
            let rc = probe(self);
            self.label = org;
            if rc != 1 {
                if let Some(lb) = self.labels.get_mut(i) {
                    deinit_label(lb);
                }
                continue;
            }
            self.switch_label(i);
            return 0;
        }
        1
    }

    /// `fdisk_label_get_fields_ids (NULL, cxt, &ids, &nids)`: the current
    /// label's default columns, as details mode and the unit select them.
    pub fn label_get_fields_ids(&self) -> Result<Vec<i32>, i32> {
        let lb = self.label_ref().ok_or(-EINVAL)?;
        if lb.fields.is_empty() {
            return Err(-ENOSYS);
        }
        let details = self.is_details();
        let cyl = self.use_cylinders();
        Ok(lb
            .fields
            .iter()
            .filter(|f| {
                !((details && f.flags & FIELDFL_EYECANDY != 0)
                    || (!details && f.flags & FIELDFL_DETAIL != 0)
                    || (f.id == fieldtype::SECTORS && cyl)
                    || (f.id == fieldtype::CYLINDERS && !cyl))
            })
            .map(|f| f.id)
            .collect())
    }

    /// `fdisk_label_get_fields_ids_all (NULL, cxt, ...)`: all its columns.
    pub fn label_get_fields_ids_all(&self) -> Result<Vec<i32>, i32> {
        let lb = self.label_ref().ok_or(-EINVAL)?;
        if lb.fields.is_empty() {
            return Err(-ENOSYS);
        }
        Ok(lb.fields.iter().map(|f| f.id).collect())
    }

    /// `fdisk_write_disklabel (cxt)`: wipes what is to be wiped, then
    /// writes the table.
    pub fn write_disklabel(&mut self) -> i32 {
        if self.readonly {
            return -EINVAL;
        }
        let Some(op) = self.op() else { return -EINVAL };
        let Some(write) = op.write else {
            return -ENOSYS;
        };
        self.do_wipe();
        write(self)
    }

    /// `fdisk_verify_disklabel (cxt)`: 0, the number of problems, or <0.
    pub fn verify_disklabel(&mut self) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(verify) = op.verify else {
            return -ENOSYS;
        };
        if self.missing_geometry() {
            return -EINVAL;
        }
        verify(self)
    }

    /// `fdisk_list_disklabel (cxt)`: every label item, as `name: value`
    /// info messages, in details mode only.
    pub fn list_disklabel(&mut self) -> i32 {
        if self.label.is_none() {
            return -EINVAL;
        }
        if !self.display_details {
            return 0;
        }
        let mut id = 0;
        let mut item = LabelItem::default();
        loop {
            // rc: <0 error, 0 success, 1 unknown item, 2 out of range.
            let rc = self.get_disklabel_item(id, &mut item);
            id = id.wrapping_add(1);
            if rc == 0 {
                let name = item.name.unwrap_or("");
                match &item.data {
                    ItemData::U64(n) => {
                        self.info(format!("{name}: {n}").as_bytes());
                    }
                    ItemData::Str(s) => {
                        if item.name.is_some() {
                            let mut m = format!("{name}: ").into_bytes();
                            m.extend_from_slice(s);
                            self.info(&m);
                        }
                    }
                    ItemData::None => {}
                }
            }
            item.reset();
            if rc != 0 && rc != 1 {
                return if rc < 0 { rc } else { 0 };
            }
        }
    }

    /// `fdisk_create_disklabel (cxt, name)`: a new empty table of type
    /// `name` (DOS by default) in memory, switching to its driver.
    pub fn create_disklabel(&mut self, name: Option<&[u8]>) -> i32 {
        let name = name.unwrap_or(b"dos");
        let mut haslabel = false;
        if let Some(lb) = self.label_mut() {
            deinit_label(lb);
            haslabel = true;
        }
        let Some(i) = self.get_label(Some(name)) else {
            return -EINVAL;
        };
        let Some(lb) = self.labels.get(i) else {
            return -EINVAL;
        };
        if lb.disabled {
            return -EINVAL;
        }
        let create = lb.op.create;
        if !haslabel || self.label != Some(i) {
            let _ = self.check_collisions();
        }
        let Some(create) = create else { return -ENOSYS };
        self.switch_label(i);
        if haslabel && self.parent.is_none() {
            let _ = self.reset_device_properties();
        }
        create(self)
    }

    /// `fdisk_locate_disklabel (cxt, n, ...)`.
    pub fn locate_disklabel(&mut self, n: i32) -> Result<Located, i32> {
        let op = self.op().ok_or(-EINVAL)?;
        let locate = op.locate.ok_or(-ENOSYS)?;
        locate(self, n)
    }

    /// `fdisk_get_disklabel_id (cxt, &id)`: the disk identifier, `None`
    /// when the label has none.
    pub fn get_disklabel_id(&mut self) -> Result<Option<Vec<u8>>, i32> {
        if self.label.is_none() {
            return Err(-EINVAL);
        }
        let mut item = LabelItem::default();
        let rc = self.get_disklabel_item(crate::LABELITEM_ID, &mut item);
        match rc {
            0 => Ok(match item.data {
                ItemData::Str(s) => Some(s),
                _ => None,
            }),
            rc if rc > 0 => Ok(None),
            rc => Err(rc),
        }
    }

    /// `fdisk_get_disklabel_item (cxt, id, item)`: 0, <0, 1 for an item the
    /// label lacks, 2 past the last.
    pub fn get_disklabel_item(&mut self, id: i32, item: &mut LabelItem) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        item.reset();
        item.id = id;
        let Some(get_item) = op.get_item else {
            return -ENOSYS;
        };
        get_item(self, item)
    }

    /// `fdisk_set_disklabel_id (cxt)`: asks for a new identifier.
    pub fn set_disklabel_id(&mut self) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(set_id) = op.set_id else {
            return -ENOSYS;
        };
        set_id(self, None)
    }

    /// `fdisk_set_disklabel_id_from_string (cxt, str)`.
    pub fn set_disklabel_id_from_string(&mut self, s: &[u8]) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(set_id) = op.set_id else {
            return -ENOSYS;
        };
        set_id(self, Some(s))
    }

    /// `fdisk_set_partition_type (cxt, partnum, t)`.
    pub fn set_partition_type(&mut self, partnum: usize, t: &PartType) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(set_part) = op.set_part else {
            return -ENOSYS;
        };
        let mut pa = Partition::new();
        pa.set_type(Some(t.clone()));
        set_part(self, partnum, &pa)
    }

    /// `fdisk_toggle_partition_flag (cxt, partnum, flag)`.
    pub fn toggle_partition_flag(&mut self, partnum: usize, flag: u64) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(toggle) = op.part_toggle_flag else {
            return -ENOSYS;
        };
        toggle(self, partnum, flag)
    }

    /// `fdisk_reorder_partitions (cxt)`: sorts the partitions by start,
    /// and says what came of it.
    pub fn reorder_partitions(&mut self) -> i32 {
        let Some(op) = self.op() else { return -EINVAL };
        let Some(reorder) = op.reorder else {
            return -ENOSYS;
        };
        let rc = reorder(self);
        match rc {
            0 => self.info(b"Partitions order fixed."),
            1 => self.info(b"Nothing to do. Ordering is correct already."),
            _ => self.warnx(b"Failed to fix partitions order."),
        };
        rc
    }
}
