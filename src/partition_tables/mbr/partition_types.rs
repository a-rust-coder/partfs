/// Empty partition, denotes free space
pub const EMPTY: u8 = 0x00;

/// Used for primary FAT12 partitions on the first 32MB of drive.
pub const FAT12_PRIMARY: u8 = 0x01;

pub const FAT16_PRIMARY: u8 = 0x06;

/// HPFS/NTFS/exFAT
pub const EXFAT: u8 = 0x07;
/// HPFS/NTFS/exFAT
pub const HPFS: u8 = 0x07;
/// HPFS/NTFS/exFAT
pub const NTFS: u8 = 0x07;

pub const FAT32_LBA: u8 = 0x0C;
