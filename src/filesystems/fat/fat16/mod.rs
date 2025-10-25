pub use super::fat12::bpb;

use crate::{
    Disk, DiskErr, Permissions,
    filesystems::fat::{FatEntry, FatError, FatFS, fat16::bpb::BiosParameterBlock},
    wrappers::{DiskWrapper, FragmentedSubDisk, SubDisk},
};
use alloc::{sync::Arc, vec, vec::Vec};

pub struct Fat16 {
    bpb: BiosParameterBlock,
    disk: Arc<DiskWrapper>,
    sector_size: usize,
}

impl Fat16 {
    /// # Errors
    ///
    /// TODO:
    pub fn read_from_disk<T: Disk + 'static>(
        disk: T,
        sector_size: Option<usize>,
    ) -> Result<Option<Self>, DiskErr> {
        let disk = DiskWrapper::new(disk);
        let sector_size = sector_size.unwrap_or_else(|| {
            let Ok(infos) = disk.disk_infos() else {
                return 0;
            };
            for i in [512, 1024, 2048, 4096] {
                if infos.sector_size.is_supported(i, infos.disk_size) {
                    return i;
                }
            }
            0
        });

        if sector_size < 512 || !sector_size.is_power_of_two() || sector_size > 4096 {
            return Err(DiskErr::UnsupportedDiskSectorSize);
        }

        let mut first_sector = vec![0; sector_size];
        disk.read_sector(0, &mut first_sector)?;

        let mut bs = [0; 512];
        bs.copy_from_slice(&first_sector[..512]);

        let bpb = BiosParameterBlock::from_bytes(bs);

        if !bpb.is_valid() || bpb.bytes_per_sector() != sector_size {
            return Ok(None);
        }

        Ok(Some(Self {
            bpb,
            disk,
            sector_size,
        }))
    }

    /// # Errors
    ///
    /// TODO:
    pub fn new<T: Disk + 'static>(
        disk: T,
        root_dir_entries: usize,
        number_of_fats: usize,
        hidden_sectors: usize,
        sector_size: Option<usize>,
        sectors_per_cluster: Option<usize>,
    ) -> Result<Option<Self>, DiskErr> {
        let disk = DiskWrapper::new(disk);
        let disk_infos = disk.disk_infos()?;

        let sector_size = sector_size.unwrap_or_else(|| {
            for i in [512, 1024, 2048, 4096] {
                if disk_infos.sector_size.is_supported(i, disk_infos.disk_size) {
                    return i;
                }
            }
            0
        });

        if sector_size < 512
            || sector_size.count_ones() != 1
            || sector_size > 0xFFFF
            || (root_dir_entries * 32).is_multiple_of(sector_size)
            || number_of_fats > 0xFF
            || root_dir_entries > 0xFFFF
            || hidden_sectors > 0xFFFF_FFFF
        {
            return Ok(None);
        }

        let root_dir_sectors = (root_dir_entries * 32) / sector_size;
        let total_sectors = disk_infos.disk_size / sector_size;

        let sectors_per_cluster = sectors_per_cluster.unwrap_or_else(|| {
            ((total_sectors - root_dir_sectors - 1).div_ceil(65525)).next_power_of_two()
        });

        if !sectors_per_cluster.is_power_of_two()
            || sectors_per_cluster > 0xFF
            || total_sectors > 0xFFFF_FFFF
        {
            return Ok(None);
        }

        let mut count_of_clusters = (total_sectors - root_dir_sectors - 1) / sectors_per_cluster;
        let fat_size = (count_of_clusters + count_of_clusters / 2).div_ceil(sector_size);
        count_of_clusters = (total_sectors - root_dir_sectors - fat_size * number_of_fats - 1)
            / sectors_per_cluster;
        let reserved_sectors = total_sectors
            - count_of_clusters * sectors_per_cluster
            - fat_size * number_of_fats
            - root_dir_sectors;

        if count_of_clusters < 4085 || count_of_clusters > 65525 {
            return Ok(None);
        }

        // All checks have already been performed
        #[allow(clippy::cast_possible_truncation)]
        let (total_sectors_16, total_sectors_32) = if total_sectors < 0x10000 {
            (total_sectors as u16, 0)
        } else {
            (0, total_sectors as u32)
        };

        // All checks have already been performed
        #[allow(clippy::cast_possible_truncation)]
        let bpb = BiosParameterBlock {
            jmp_boot: [0xEB, 0xFE, 0x90],
            oem_name: [0; 8],
            bytes_per_sector: sector_size as u16,
            sectors_per_cluster: sectors_per_cluster as u8,
            reserved_sectors_count: reserved_sectors as u16,
            number_of_fats: number_of_fats as u8,
            root_entries_count: root_dir_entries as u16,
            total_sectors_16,
            media: 0xF8,
            fat_size: fat_size as u16,
            sectors_per_track: 0,
            number_of_heads: 0,
            total_sectors_32,
            hidden_sectors: hidden_sectors as u32,
            drive_number: 0x80,
            reserved0: 0,
            boot_signature: 0x29,
            volume_id: 0,
            volume_label: *b"NO NAME    ",
            fs_type: *b"FAT16   ",
            boot_code: [0; 448],
            signature: 0xAA55,
        };

        if !bpb.is_valid() {
            return Ok(None);
        }

        let bytes = bpb.to_bytes();
        let mut sector = vec![0; sector_size];

        for i in 0..(reserved_sectors + number_of_fats * fat_size + root_dir_sectors) {
            disk.write_sector(i, &sector)?;
        }

        sector[..512].copy_from_slice(&bytes);
        disk.write_sector(0, &sector)?;

        Ok(Some(Self {
            bpb,
            disk,
            sector_size,
        }))
    }

    #[must_use]
    pub const fn bios_parameter_block(&self) -> BiosParameterBlock {
        self.bpb
    }
}

impl FatFS for Fat16 {
    fn get_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
    ) -> Result<Result<FatEntry, FatError>, DiskErr> {
        if index >= self.bpb.count_of_clusters() || fat_index >= self.bpb.number_of_fats() {
            return Ok(Err(FatError::IndexOutOfRange));
        }

        let fat_offset = index * 2;
        let sector_number = self.bpb.reserved_sectors_count()
            + (fat_offset / self.sector_size)
            + fat_index * self.bpb.fat_size();
        let fat_entry_offset = fat_offset % self.sector_size;

        let mut sector = vec![0; self.sector_size];
        self.disk.read_sector(sector_number, &mut sector)?;
        let entry = u16::from_le_bytes([sector[fat_entry_offset], sector[fat_entry_offset + 1]]);

        Ok(FatEntry::from_fat16(entry as usize))
    }

    fn set_fat_entry(
        &self,
        index: usize,
        fat_index: usize,
        value: FatEntry,
    ) -> Result<Result<(), FatError>, DiskErr> {
        if index >= self.bpb.count_of_clusters() || fat_index >= self.bpb.number_of_fats() {
            return Ok(Err(FatError::IndexOutOfRange));
        }

        let fat_offset = 2 * index;
        let sector_number = self.bpb.reserved_sectors_count()
            + (fat_offset / self.sector_size)
            + fat_index * self.bpb.fat_size();
        let fat_entry_offset = fat_offset % self.sector_size;

        let mut sector = vec![0; self.sector_size];
        self.disk.read_sector(sector_number, &mut sector)?;

        // `to_fat12()` already performs the check
        #[allow(clippy::cast_possible_truncation)]
        let entry = match value.to_fat16() {
            Ok(v) => v as u16,
            Err(e) => return Ok(Err(e)),
        }
        .to_le_bytes();

        sector[fat_entry_offset] |= entry[0];
        sector[fat_entry_offset + 1] |= entry[1];

        self.disk.write_sector(sector_number, &sector)?;

        Ok(Ok(()))
    }

    fn get_cluster(
        &self,
        index: usize,
        permissions: Permissions,
    ) -> Result<Result<SubDisk, FatError>, DiskErr> {
        if index >= self.bpb.count_of_clusters() || index < 2 {
            return Ok(Err(FatError::IndexOutOfRange));
        }

        let first_sector =
            self.bpb.data_start_sector() + (index - 2) * self.bpb.sectors_per_cluster();

        self.disk
            .subdisk(
                first_sector * self.sector_size,
                (first_sector + self.bpb.sectors_per_cluster()) * self.sector_size,
                permissions,
            )
            .map(Ok)
    }

    fn sector_size(&self) -> usize {
        self.sector_size
    }

    fn get_root_dir(&self, permissions: crate::Permissions) -> Result<SubDisk, DiskErr> {
        let root_dir_start = (self.bpb.reserved_sectors_count()
            + self.bpb.fat_size() * self.bpb.number_of_fats())
            * self.sector_size;
        let root_dir_end = root_dir_start
            + (self.bpb.root_entries_count() * 32).div_ceil(self.sector_size) * self.sector_size;

        self.disk.subdisk(root_dir_start, root_dir_end, permissions)
    }

    fn create_frangemented_subdisk(
        &self,
        clusters: Vec<usize>,
        permissions: Permissions,
    ) -> Result<Result<FragmentedSubDisk, FatError>, DiskErr> {
        let mut parts = Vec::with_capacity(clusters.len());

        for i in 0..clusters.len() {
            if clusters[i] >= self.bpb.count_of_clusters() || clusters[i] < 2 {
                return Ok(Err(FatError::IndexOutOfRange));
            }

            for j in i + 1..clusters.len() {
                if clusters[i] == clusters[j] {
                    return Ok(Err(FatError::InfiniteLoop));
                }
            }

            let start = self.bpb.reserved_sectors_count()
                + self.bpb.fat_size() * self.bpb.number_of_fats()
                + (self.bpb.root_entries_count() * 32) / self.sector_size
                + (clusters[i] - 2) * self.bpb.sectors_per_cluster();

            parts.push((
                start * self.sector_size,
                (start + self.bpb.sectors_per_cluster()) * self.sector_size,
            ));
        }

        self.disk.fragmented_subdisk(parts, permissions).map(Ok)
    }
}
