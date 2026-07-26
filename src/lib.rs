#![warn(missing_docs)]
//! Rust bindings to FreeBSD's `DIOCZONECMD` ioctl for zoned block devices.
//!
//! Zoned devices conform to the SCSI Zoned Block Commands (ZBC) or ATA Zoned ATA
//! Command Set (ZAC) specifications. This crate provides bindings equivalent to
//! what the [zonectl(8)](https://man.freebsd.org/cgi/man.cgi?query=zonectl) utility
//! provides, but in idiomatic Rust.
//!
//! The main entry point is the [`ZonedDevice`] extension trait, implemented for any type that
//! implements [`AsFd`], which exposes methods for querying zone parameters, reporting zones,
//! and managing zone state.

#![cfg_attr(docsrs, feature(doc_cfg))]

use std::{
    io,
    os::fd::{AsFd, AsRawFd},
};

use nix::ioctl_readwrite;

cfg_if::cfg_if! {
    if #[cfg(target_pointer_width = "64")] {
        mod ffi64;
        use ffi64 as ffi;
    } else if #[cfg(target_pointer_width = "32")] {
        mod ffi32;
        use ffi32 as ffi;
    }
}

// Nix's ioctl macros create `pub` functions. Put them into a module to hide them from public
// consumers.
mod ioctl {
    use super::*;

    ioctl_readwrite!(dioczonecmd, 'd', 143, ffi::disk_zone_args);
}

/// Sentinel value for a conventional zone's write pointer, meaning "not applicable".
pub const WRITE_POINTER_NA: u64 = u64::MAX;

/// Default number of zone entries to request per `DIOCZONECMD` call.
///
/// The kernel may return fewer entries than requested, based on `maxphys`.
pub const DEFAULT_CHUNKSIZE: u32 = 16384;

fn zonecmd_ioctl<F: AsFd>(fd: &F, args: &mut ffi::disk_zone_args) -> io::Result<()> {
    unsafe { ioctl::dioczonecmd(fd.as_fd().as_raw_fd(), args) }?;
    Ok(())
}

fn rwp_cmd<F: AsFd>(fd: &F, cmd: u8, id: u64, all: bool) -> io::Result<()> {
    let mut args = ffi::disk_zone_args {
        zone_cmd:    cmd,
        zone_params: ffi::disk_zone_params {
            rwp: ffi::disk_zone_rwp {
                id,
                flags: if all {
                    ffi::DISK_ZONE_RWP_FLAG_ALL as u8
                } else {
                    0
                },
            },
        },
    };
    zonecmd_ioctl(fd, &mut args)
}

fn report_zones_with_chunk<F: AsFd>(
    fd: &F,
    options: ReportOptions,
    starting_id: u64,
    chunksize: u32,
) -> io::Result<ReportZones<'_, F>> {
    let mut zones = ReportZones {
        device: fd,
        options,
        starting_id,
        chunksize,
        chunk_entries: Vec::new(),
        chunk_index: 0,
        header: ReportHeader::default(),
        entries_available: 0,
        header_set: false,
        exhausted: false,
    };
    zones.fetch_chunk()?;
    Ok(zones)
}

/// Extension trait for `DIOCZONECMD` operations on zoned block devices.
pub trait ZonedDevice: AsFd + Sized {
    /// Return zone device parameters.
    fn get_params(&self) -> io::Result<DiskParams> {
        let mut args = ffi::disk_zone_args {
            zone_cmd: ffi::DISK_ZONE_GET_PARAMS as u8,
            ..Default::default()
        };
        zonecmd_ioctl(self, &mut args)?;
        Ok(DiskParams::from(unsafe { args.zone_params.disk_params }))
    }

    /// Open the zone at `id`, or all zones if `all` is true.
    fn open_zone(&self, id: u64, all: bool) -> io::Result<()> {
        rwp_cmd(self, ffi::DISK_ZONE_OPEN as u8, id, all)
    }

    /// Close the zone at `id`, or all zones if `all` is true.
    fn close_zone(&self, id: u64, all: bool) -> io::Result<()> {
        rwp_cmd(self, ffi::DISK_ZONE_CLOSE as u8, id, all)
    }

    /// Finish the zone at `id`, or all zones if `all` is true.
    fn finish_zone(&self, id: u64, all: bool) -> io::Result<()> {
        rwp_cmd(self, ffi::DISK_ZONE_FINISH as u8, id, all)
    }

    /// Reset the write pointer for the zone at `id`, or all zones if `all` is true.
    fn reset_write_pointer(&self, id: u64, all: bool) -> io::Result<()> {
        rwp_cmd(self, ffi::DISK_ZONE_RWP as u8, id, all)
    }

    /// Report zones matching `options`, starting at `starting_id`.
    ///
    /// Yields one [`ZoneEntry`] at a time. Header metadata is available from the
    /// iterator's [`ReportZones::header`] and [`ReportZones::entries_available`] methods
    /// immediately after this function returns successfully.
    fn report_zones(
        &self,
        options: ReportOptions,
        starting_id: u64,
    ) -> io::Result<ReportZones<'_, Self>> {
        report_zones_with_chunk(self, options, starting_id, DEFAULT_CHUNKSIZE)
    }
}

impl<T: AsFd> ZonedDevice for T {}

/// Zone device parameters returned by [`ZonedDevice::get_params`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskParams {
    /// How the drive manages zones.
    pub zone_mode:            ZoneMode,
    /// Raw capability flags from the device.
    pub flags:                u64,
    /// Optimal number of open sequential-write-preferred zones, if reported.
    pub optimal_seq_zones:    Option<u64>,
    /// Optimal number of non-sequentially written sequential-write-preferred zones.
    pub optimal_nonseq_zones: Option<u64>,
    /// Maximum number of open sequential-write-required zones.
    pub max_seq_zones:        Option<u64>,
}

impl DiskParams {
    /// Returns true if the device supports report zones.
    pub fn supports_report_zones(&self) -> bool {
        self.flags & ffi::DISK_ZONE_RZ_SUP as u64 != 0
    }

    /// Returns true if the device supports explicit open.
    pub fn supports_open(&self) -> bool {
        self.flags & ffi::DISK_ZONE_OPEN_SUP as u64 != 0
    }

    /// Returns true if the device supports close.
    pub fn supports_close(&self) -> bool {
        self.flags & ffi::DISK_ZONE_CLOSE_SUP as u64 != 0
    }

    /// Returns true if the device supports finish.
    pub fn supports_finish(&self) -> bool {
        self.flags & ffi::DISK_ZONE_FINISH_SUP as u64 != 0
    }

    /// Returns true if the device supports reset write pointer.
    pub fn supports_reset_write_pointer(&self) -> bool {
        self.flags & ffi::DISK_ZONE_RWP_SUP as u64 != 0
    }

    /// Returns true if unrestricted read is allowed in sequential-write-required zones.
    pub fn unrestricted_read_in_seq_required(&self) -> bool {
        self.flags & ffi::DISK_ZONE_DISK_URSWRZ as u64 != 0
    }
}

impl From<ffi::disk_zone_disk_params> for DiskParams {
    fn from(params: ffi::disk_zone_disk_params) -> Self {
        let optimal_seq_zones = if params.flags & ffi::DISK_ZONE_OPT_SEQ_SET as u64 != 0 {
            Some(params.optimal_seq_zones)
        } else {
            None
        };
        let optimal_nonseq_zones = if params.flags & ffi::DISK_ZONE_OPT_NONSEQ_SET as u64 != 0 {
            Some(params.optimal_nonseq_zones)
        } else {
            None
        };
        let max_seq_zones = if params.flags & ffi::DISK_ZONE_MAX_SEQ_SET as u64 != 0 {
            Some(params.max_seq_zones)
        } else {
            None
        };

        Self {
            zone_mode: ZoneMode::from(params.zone_mode),
            flags: params.flags,
            optimal_seq_zones,
            optimal_nonseq_zones,
            max_seq_zones,
        }
    }
}

/// How a zoned device exposes and manages its zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneMode {
    /// Not a zoned device.
    None,
    /// Host-aware zoned device.
    HostAware,
    /// Drive-managed zoned device.
    DriveManaged,
    /// Host-managed zoned device.
    HostManaged,
    /// Unknown mode value.
    Unknown(u32),
}

impl From<u32> for ZoneMode {
    fn from(mode: u32) -> Self {
        match mode {
            x if x == ffi::DISK_ZONE_MODE_NONE => Self::None,
            x if x == ffi::DISK_ZONE_MODE_HOST_AWARE => Self::HostAware,
            x if x == ffi::DISK_ZONE_MODE_DRIVE_MANAGED => Self::DriveManaged,
            x if x == ffi::DISK_ZONE_MODE_HOST_MANAGED => Self::HostManaged,
            x => Self::Unknown(x),
        }
    }
}

/// Filter for [`ZonedDevice::report_zones`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ReportOptions {
    /// All zones.
    All = ffi::DISK_ZONE_REP_ALL as u8,
    /// Empty zones.
    Empty = ffi::DISK_ZONE_REP_EMPTY as u8,
    /// Implicitly open zones.
    ImplicitOpen = ffi::DISK_ZONE_REP_IMP_OPEN as u8,
    /// Explicitly open zones.
    ExplicitOpen = ffi::DISK_ZONE_REP_EXP_OPEN as u8,
    /// Closed zones.
    Closed = ffi::DISK_ZONE_REP_CLOSED as u8,
    /// Full zones.
    Full = ffi::DISK_ZONE_REP_FULL as u8,
    /// Read-only zones.
    ReadOnly = ffi::DISK_ZONE_REP_READONLY as u8,
    /// Offline zones.
    Offline = ffi::DISK_ZONE_REP_OFFLINE as u8,
    /// Zones needing reset write pointer.
    ResetWritePointer = ffi::DISK_ZONE_REP_RWP as u8,
    /// Non-sequentially written zones.
    NonSequential = ffi::DISK_ZONE_REP_NON_SEQ as u8,
    /// Non-write-pointer zones.
    NonWritePointer = ffi::DISK_ZONE_REP_NON_WP as u8,
}

/// Header metadata from a report-zones operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReportHeader {
    /// How zone lengths and types relate across the device.
    pub same:        ZoneSame,
    /// Maximum LBA on the device.
    pub maximum_lba: u64,
}

impl From<ffi::disk_zone_rep_header> for ReportHeader {
    fn from(header: ffi::disk_zone_rep_header) -> Self {
        Self {
            same:        ZoneSame::from(header.same),
            maximum_lba: header.maximum_lba,
        }
    }
}

/// Describes whether zone lengths and types are uniform across a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoneSame {
    /// Zone lengths and types may vary.
    #[default]
    AllDifferent,
    /// Zone lengths and types are all the same.
    AllSame,
    /// Zone types are the same; only the last zone length differs.
    LastDifferent,
    /// Zone lengths are the same; types vary.
    TypesDifferent,
    /// Unknown value.
    Unknown(u8),
}

impl From<u8> for ZoneSame {
    fn from(same: u8) -> Self {
        match same {
            x if x == ffi::DISK_ZONE_SAME_ALL_DIFFERENT as u8 => Self::AllDifferent,
            x if x == ffi::DISK_ZONE_SAME_ALL_SAME as u8 => Self::AllSame,
            x if x == ffi::DISK_ZONE_SAME_LAST_DIFFERENT as u8 => Self::LastDifferent,
            x if x == ffi::DISK_ZONE_SAME_TYPES_DIFFERENT as u8 => Self::TypesDifferent,
            x => Self::Unknown(x),
        }
    }
}

/// Iterator over zone entries returned by [`ZonedDevice::report_zones`].
#[derive(Debug)]
pub struct ReportZones<'a, F: AsFd> {
    device:            &'a F,
    options:           ReportOptions,
    starting_id:       u64,
    chunksize:         u32,
    chunk_entries:     Vec<ZoneEntry>,
    chunk_index:       usize,
    header:            ReportHeader,
    entries_available: u32,
    header_set:        bool,
    exhausted:         bool,
}

impl<'a, F: AsFd> ReportZones<'a, F> {
    /// Report header metadata from the first ioctl.
    pub fn header(&self) -> &ReportHeader {
        &self.header
    }

    /// Total number of zones available for the selected filter.
    pub fn entries_available(&self) -> u32 {
        self.entries_available
    }

    fn fetch_chunk(&mut self) -> io::Result<()> {
        let mut raw_entries = vec![ffi::disk_zone_rep_entry::default(); self.chunksize as usize];

        let mut args = ffi::disk_zone_args {
            zone_cmd:    ffi::DISK_ZONE_REPORT_ZONES as u8,
            zone_params: ffi::disk_zone_params {
                report: ffi::disk_zone_report {
                    starting_id: self.starting_id,
                    rep_options: self.options as u8,
                    entries_allocated: self.chunksize,
                    entries: raw_entries.as_mut_ptr(),
                    ..Default::default()
                },
            },
        };

        zonecmd_ioctl(self.device, &mut args)?;
        let report = unsafe { &args.zone_params.report };

        if !self.header_set {
            self.header = ReportHeader::from(report.header);
            self.entries_available = report.entries_available;
            self.header_set = true;
        }

        self.chunk_entries = raw_entries
            .iter()
            .take(report.entries_filled as usize)
            .map(|entry| ZoneEntry::from(*entry))
            .collect();
        self.chunk_index = 0;

        let more_data = report
            .entries_available
            .saturating_sub(report.entries_filled)
            > 0;
        if !more_data {
            self.exhausted = true;
        } else if let Some(last) = self.chunk_entries.last() {
            self.starting_id = last.zone_start_lba + last.zone_length;
        } else {
            self.exhausted = true;
        }

        Ok(())
    }
}

impl<'a, F: AsFd> Iterator for ReportZones<'a, F> {
    type Item = io::Result<ZoneEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.chunk_index < self.chunk_entries.len() {
                let entry = self.chunk_entries[self.chunk_index];
                self.chunk_index += 1;
                return Some(Ok(entry));
            }

            if self.exhausted {
                return None;
            }

            return match self.fetch_chunk() {
                Ok(()) => continue,
                Err(e) => {
                    self.exhausted = true;
                    Some(Err(e))
                }
            };
        }
    }
}
/// A single zone entry from a report-zones operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneEntry {
    /// Zone type.
    pub zone_type:         ZoneType,
    /// Zone condition.
    pub zone_condition:    ZoneCondition,
    zone_flags:            u8,
    /// Zone length in LBAs.
    pub zone_length:       u64,
    /// Starting LBA of the zone.
    pub zone_start_lba:    u64,
    /// Write pointer LBA, if applicable.
    pub write_pointer_lba: Option<u64>,
}

impl ZoneEntry {
    /// Returns true if the zone needs reset write pointer.
    pub fn needs_reset(&self) -> bool {
        self.zone_flags & ffi::DISK_ZONE_FLAG_RESET as u8 != 0
    }

    /// Returns true if the zone was accessed non-sequentially.
    pub fn non_sequential(&self) -> bool {
        self.zone_flags & ffi::DISK_ZONE_FLAG_NON_SEQ as u8 != 0
    }
}

impl From<ffi::disk_zone_rep_entry> for ZoneEntry {
    fn from(entry: ffi::disk_zone_rep_entry) -> Self {
        let write_pointer_lba = if entry.write_pointer_lba == WRITE_POINTER_NA {
            None
        } else {
            Some(entry.write_pointer_lba)
        };

        Self {
            zone_type: ZoneType::from(entry.zone_type),
            zone_condition: ZoneCondition::from(entry.zone_condition),
            zone_flags: entry.zone_flags,
            zone_length: entry.zone_length,
            zone_start_lba: entry.zone_start_lba,
            write_pointer_lba,
        }
    }
}

/// Zone type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneType {
    /// Conventional random-access zone.
    Conventional,
    /// Sequential-write-required zone (host-managed).
    SeqRequired,
    /// Sequential-write-preferred zone (host-aware).
    SeqPreferred,
    /// Unknown zone type.
    Unknown(u8),
}

impl From<u8> for ZoneType {
    fn from(zone_type: u8) -> Self {
        match zone_type {
            x if x == ffi::DISK_ZONE_TYPE_CONVENTIONAL as u8 => Self::Conventional,
            x if x == ffi::DISK_ZONE_TYPE_SEQ_REQUIRED as u8 => Self::SeqRequired,
            x if x == ffi::DISK_ZONE_TYPE_SEQ_PREFERRED as u8 => Self::SeqPreferred,
            x => Self::Unknown(x),
        }
    }
}

/// Zone condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneCondition {
    /// Not a write-pointer zone.
    NotWritePointer,
    /// Empty.
    Empty,
    /// Implicitly open.
    ImplicitOpen,
    /// Explicitly open.
    ExplicitOpen,
    /// Closed.
    Closed,
    /// Read-only.
    ReadOnly,
    /// Full.
    Full,
    /// Offline.
    Offline,
    /// Unknown condition.
    Unknown(u8),
}

impl From<u8> for ZoneCondition {
    fn from(condition: u8) -> Self {
        match condition {
            x if x == ffi::DISK_ZONE_COND_NOT_WP as u8 => Self::NotWritePointer,
            x if x == ffi::DISK_ZONE_COND_EMPTY as u8 => Self::Empty,
            x if x == ffi::DISK_ZONE_COND_IMPLICIT_OPEN as u8 => Self::ImplicitOpen,
            x if x == ffi::DISK_ZONE_COND_EXPLICIT_OPEN as u8 => Self::ExplicitOpen,
            x if x == ffi::DISK_ZONE_COND_CLOSED as u8 => Self::Closed,
            x if x == ffi::DISK_ZONE_COND_READONLY as u8 => Self::ReadOnly,
            x if x == ffi::DISK_ZONE_COND_FULL as u8 => Self::Full,
            x if x == ffi::DISK_ZONE_COND_OFFLINE as u8 => Self::Offline,
            _ => Self::Unknown(condition),
        }
    }
}

/// Re-export raw FFI constants and types for advanced use.
pub mod raw {
    pub use super::ffi::*;
}

/// Helpers for creating temporary gzoned(8) devices.
#[cfg(feature = "gzoned")]
pub mod gzoned;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_params_support_flags() {
        let params = DiskParams {
            zone_mode:            ZoneMode::HostManaged,
            flags:                ffi::DISK_ZONE_RZ_SUP as u64 | ffi::DISK_ZONE_OPEN_SUP as u64,
            optimal_seq_zones:    None,
            optimal_nonseq_zones: None,
            max_seq_zones:        None,
        };
        assert!(params.supports_report_zones());
        assert!(params.supports_open());
        assert!(!params.supports_close());
    }

    #[test]
    fn zone_entry_write_pointer_na() {
        let entry = ZoneEntry::from(ffi::disk_zone_rep_entry {
            zone_type: ffi::DISK_ZONE_TYPE_CONVENTIONAL as u8,
            zone_length: 1,
            write_pointer_lba: WRITE_POINTER_NA,
            ..Default::default()
        });
        assert_eq!(entry.write_pointer_lba, None);
    }
}
