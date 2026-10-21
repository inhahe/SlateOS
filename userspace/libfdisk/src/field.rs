//! `field.c`: how a column of a partition listing looks.

/// `FDISK_FIELDFL_DETAIL`: only in details mode (`fdisk -x`).
pub const FIELDFL_DETAIL: i32 = 1 << 1;
/// `FDISK_FIELDFL_EYECANDY`: not in details mode.
pub const FIELDFL_EYECANDY: i32 = 1 << 2;
/// `FDISK_FIELDFL_NUMBER`: the column holds numbers (right-aligned).
pub const FIELDFL_NUMBER: i32 = 1 << 3;

/// `struct fdisk_field`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Field {
    /// `FDISK_FIELD_*`.
    pub id: i32,
    /// The column's heading.
    pub name: &'static str,
    /// Its width hint, as libsmartcols takes it: a fraction of the
    /// terminal below 1, columns from 1 up.
    pub width: f64,
    /// `FDISK_FIELDFL_*`.
    pub flags: i32,
}

impl Field {
    /// A field, as the label tables write it.
    #[must_use]
    pub const fn new(id: i32, name: &'static str, width: f64, flags: i32) -> Self {
        Self {
            id,
            name,
            width,
            flags,
        }
    }

    /// `fdisk_field_get_id`.
    #[must_use]
    pub fn get_id(&self) -> i32 {
        self.id
    }

    /// `fdisk_field_get_name`.
    #[must_use]
    pub fn get_name(&self) -> &'static str {
        self.name
    }

    /// `fdisk_field_get_width`.
    #[must_use]
    pub fn get_width(&self) -> f64 {
        self.width
    }

    /// `fdisk_field_is_number`.
    #[must_use]
    pub fn is_number(&self) -> bool {
        self.flags & FIELDFL_NUMBER != 0
    }
}
