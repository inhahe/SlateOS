//! `item.c`: a label item -- a fact about the table as a whole, like the
//! disk identifier or GPT's first usable LBA.

/// An item's value (`type` and `data`): C's `s` (string) or `j`
/// (`uint64_t`), or nothing yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ItemData {
    /// Not set.
    #[default]
    None,
    /// `'s'`.
    Str(Vec<u8>),
    /// `'j'`.
    U64(u64),
}

/// `struct fdisk_labelitem`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelItem {
    /// `FDISK_LABELITEM_*` or a label's own `*_LABELITEM_*`.
    pub id: i32,
    /// Its human-readable name.
    pub name: Option<&'static str>,
    /// Its value.
    pub data: ItemData,
}

impl LabelItem {
    /// `fdisk_reset_labelitem`.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// `fdisk_labelitem_get_name`.
    #[must_use]
    pub fn get_name(&self) -> Option<&'static str> {
        self.name
    }

    /// `fdisk_labelitem_get_id`.
    #[must_use]
    pub fn get_id(&self) -> i32 {
        self.id
    }

    /// `fdisk_labelitem_get_data_u64`: -EINVAL for a string item.
    pub fn get_data_u64(&self) -> Result<u64, i32> {
        match self.data {
            ItemData::U64(n) => Ok(n),
            _ => Err(-crate::errno::EINVAL),
        }
    }

    /// `fdisk_labelitem_get_data_string`: -EINVAL for a number item.
    pub fn get_data_string(&self) -> Result<&[u8], i32> {
        match &self.data {
            ItemData::Str(s) => Ok(s),
            _ => Err(-crate::errno::EINVAL),
        }
    }

    /// `fdisk_labelitem_is_string`.
    #[must_use]
    pub fn is_string(&self) -> bool {
        matches!(self.data, ItemData::Str(_))
    }

    /// `fdisk_labelitem_is_number`.
    #[must_use]
    pub fn is_number(&self) -> bool {
        matches!(self.data, ItemData::U64(_))
    }
}
