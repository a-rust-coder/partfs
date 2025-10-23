use alloc::{string::String, sync::Arc, vec::Vec};
use time_units::fat::FatTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirEntryRaw {
    short_name: [u8; 11],
    attributes: u8,
    reserved: u8,
    creation_time_cents: u8,
    creation_time: u16,
    creation_date: u16,
    last_access_date: u16,
    first_cluster_high: u16,
    write_time: u16,
    write_date: u16,
    first_cluster_low: u16,
    file_size: u32,
}

impl From<[u8; 32]> for DirEntryRaw {
    fn from(value: [u8; 32]) -> Self {
        Self {
            short_name: {
                let mut short_name = [0; 11];
                short_name.copy_from_slice(&value[..11]);
                short_name
            },
            attributes: value[11],
            reserved: value[12],
            creation_time_cents: value[13],
            creation_time: u16::from_le_bytes([value[14], value[15]]),
            creation_date: u16::from_le_bytes([value[16], value[17]]),
            last_access_date: u16::from_le_bytes([value[18], value[19]]),
            first_cluster_high: u16::from_le_bytes([value[20], value[21]]),
            write_time: u16::from_le_bytes([value[22], value[23]]),
            write_date: u16::from_le_bytes([value[24], value[25]]),
            first_cluster_low: u16::from_le_bytes([value[26], value[27]]),
            file_size: u32::from_le_bytes([value[28], value[29], value[30], value[31]]),
        }
    }
}

impl From<DirEntryRaw> for [u8; 32] {
    fn from(val: DirEntryRaw) -> Self {
        let mut output = [0; 32];

        output[..11].copy_from_slice(&val.short_name);
        output[11] = val.attributes;
        output[12] = val.reserved;
        output[13] = val.creation_time_cents;
        output[14..16].copy_from_slice(&val.creation_time.to_le_bytes());
        output[16..18].copy_from_slice(&val.creation_date.to_le_bytes());
        output[18..20].copy_from_slice(&val.last_access_date.to_le_bytes());
        output[20..22].copy_from_slice(&val.first_cluster_high.to_le_bytes());
        output[22..24].copy_from_slice(&val.write_time.to_le_bytes());
        output[24..26].copy_from_slice(&val.write_date.to_le_bytes());
        output[26..28].copy_from_slice(&val.first_cluster_low.to_le_bytes());

        output
    }
}

impl DirEntryRaw {
    #[must_use]
    pub const fn first_cluster(&self) -> usize {
        ((self.first_cluster_high as usize) << 16) | (self.first_cluster_low as usize)
    }

    #[must_use]
    pub const fn creation_time(&self) -> Option<FatTime> {
        FatTime::new(
            self.creation_date,
            self.creation_time,
            Some(self.creation_time_cents),
        )
    }

    #[must_use]
    pub const fn write_time(&self) -> Option<FatTime> {
        FatTime::new(self.write_date, self.write_time, None)
    }

    #[must_use]
    pub const fn last_access_time(&self) -> Option<FatTime> {
        FatTime::new(self.last_access_date, 0, None)
    }

    #[must_use]
    pub const fn file_size(&self) -> usize {
        self.file_size as usize
    }

    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.attributes & 0x01 > 0
    }

    #[must_use]
    pub const fn is_hidden(&self) -> bool {
        self.attributes & 0x02 > 0
    }

    #[must_use]
    pub const fn is_system(&self) -> bool {
        self.attributes & 0x04 > 0
    }

    #[must_use]
    pub const fn is_colume_id(&self) -> bool {
        self.attributes & 0x08 > 0
    }

    #[must_use]
    pub const fn is_directory(&self) -> bool {
        self.attributes & 0x10 > 0
    }

    #[must_use]
    pub const fn is_archive(&self) -> bool {
        self.attributes & 0x20 > 0
    }

    #[must_use]
    pub const fn is_long_name(&self) -> bool {
        (self.attributes & 0b1111) == 0b1111
    }

    #[must_use]
    pub const fn is_free(&self) -> bool {
        self.short_name[0] == 0xE5 || self.short_name[0] == 0
    }

    #[must_use]
    pub fn is_valid_entry(&self) -> bool {
        for i in self.short_name {
            if i < b' '
                || !(i.is_ascii_lowercase()
                    || i.is_ascii_digit()
                    || b"$%'-_@~`!(){}^#&".contains(&i))
            {
                return false;
            }
        }
        self.short_name[0] != b' ' && self.attributes & 0xC0 == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// Content of the entry
    pub(super) raw: DirEntryRaw,

    /// Handle to the parent directory
    pub(super) parent: Arc<Directory>,

    /// Index of the entry on the parent directory structure
    pub(super) parent_index: usize,

    /// `name` may differ from `short_name`, it comes from the long file name entries if they
    /// exist.
    pub(super) name: String,
}

impl DirEntry {
    #[must_use]
    pub fn parent(&self) -> Arc<Directory> {
        self.parent.clone()
    }

    #[must_use]
    pub const fn parent_index(&self) -> usize {
        self.parent_index
    }

    #[must_use]
    pub fn name(&self) -> String {
        self.name.clone()
    }

    #[must_use]
    pub const fn first_cluster(&self) -> usize {
        self.raw.first_cluster()
    }

    #[must_use]
    pub const fn creation_time(&self) -> Option<FatTime> {
        self.raw.creation_time()
    }

    #[must_use]
    pub const fn write_time(&self) -> Option<FatTime> {
        self.raw.write_time()
    }

    #[must_use]
    pub const fn last_access_time(&self) -> Option<FatTime> {
        self.raw.last_access_time()
    }

    #[must_use]
    pub const fn file_size(&self) -> usize {
        self.raw.file_size()
    }

    #[must_use]
    pub fn rev_path(&self) -> Vec<usize> {
        let mut rev_path = Vec::from([self.parent_index]);
        let mut current = self.parent();

        while let Directory::Other(dir_entry) = (*current).clone() {
            rev_path.push(dir_entry.parent_index);
            current = dir_entry.parent();
        }

        rev_path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Directory {
    Root,
    Other(DirEntry),
}

impl Directory {
    #[must_use]
    pub fn is_root(&self) -> bool {
        *self == Self::Root
    }

    #[must_use]
    pub fn rev_path(&self) -> Vec<usize> {
        match self {
            Self::Root => Vec::new(),
            Self::Other(v) => v.rev_path(),
        }
    }
}
