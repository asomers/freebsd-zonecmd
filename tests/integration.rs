use std::{
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
};

use freebsd_zonecmd::*;
use rstest::{fixture, rstest};

const DEFAULT_ZONESIZE: u32 = 65536; // Default zone size in sectors
const SECTORSIZE: u32 = 4096;

mod gzoned {
    use std::{
        io,
        ops::RangeInclusive,
        path::{Path, PathBuf},
        process::Command,
    };

    use mdconfig::Md;
    use tempfile::NamedTempFile;

    use super::{DEFAULT_ZONESIZE, SECTORSIZE};

    /// Used to construct a [`Gzoned`];
    #[derive(Debug)]
    pub struct Builder {
        conventional_zones: Vec<RangeInclusive<u64>>,
        sectors: u64,
        /// Size of a zone, in bytes
        zonesize: u32,
    }

    impl Builder {
        pub fn build(self) -> io::Result<Gzoned> {
            let mut tf = NamedTempFile::new()?;
            tf.as_file_mut()
                .set_len(u64::from(SECTORSIZE) * self.sectors)?;
            let md = mdconfig::Builder::vnode(tf.path())
                .sectorsize(SECTORSIZE)
                .create()?;

            let mut builder = Command::new("gzoned");
            builder
                .args(["create", "-s"])
                .arg(format!("{}", self.zonesize));
            if !self.conventional_zones.is_empty() {
                let conventional_zones = self
                    .conventional_zones
                    .into_iter()
                    .map(|r| {
                        if 1 + r.end() - r.start() == 1 {
                            format!("{}", r.start())
                        } else {
                            format!("{}-{}", r.start(), r.end())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                builder.arg("-r").arg(conventional_zones);
            };
            builder.arg(md.path()).output()?;
            let pb = Path::new("/dev").join(format!("{}.zoned", md.path().display()));
            Ok(Gzoned {
                pb,
                _md: md,
                _tf: tf,
            })
        }

        /// Add an inclusive range of zones that should be treated as conventional, not sequential
        pub fn conventional_zones(mut self, zones: RangeInclusive<u64>) -> Self {
            assert!(
                self.conventional_zones.len() < 16,
                "gzoned has a maximum of 16 conventional zone ranges"
            );
            self.conventional_zones.push(zones);
            self
        }

        /// Set the total size of the device, in 4k sectors
        pub fn sectors(mut self, sectors: u64) -> Self {
            self.sectors = sectors;
            self
        }

        /// Set the simulated zone size, in sectors
        pub fn zonesize(mut self, size: u32) -> Self {
            self.zonesize = size * SECTORSIZE;
            self
        }
    }

    impl Default for Builder {
        fn default() -> Self {
            let zonesize = DEFAULT_ZONESIZE * SECTORSIZE;
            Builder {
                zonesize,
                sectors: 524288,
                conventional_zones: Default::default(),
            }
        }
    }

    /// A temporary gzoned(8) device that will clean up after itself on Drop
    #[derive(Debug)]
    pub struct Gzoned {
        pb: PathBuf,
        _md: Md,
        _tf: NamedTempFile,
    }

    impl Gzoned {
        pub fn path(&self) -> &Path {
            self.pb.as_path()
        }
    }

    impl Drop for Gzoned {
        fn drop(&mut self) {
            Command::new("gzoned")
                .args(["stop", "-f"])
                .arg(&self.pb)
                .output()
                .expect("failed to tear down the gzoned device");
        }
    }
}

#[derive(Debug)]
struct Harness {
    _dev: gzoned::Gzoned,
    f: File,
}

#[fixture]
fn harness() -> Harness {
    let dev = gzoned::Builder::default()
        .conventional_zones(0u64..=1)
        .build()
        .expect("gzoned create failed");
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .open(dev.path())
        .unwrap();
    Harness { _dev: dev, f }
}

#[rstest]
fn close_zone(harness: Harness) {
    let start_lba = 2 * u64::from(DEFAULT_ZONESIZE);
    let data = vec![42u8; SECTORSIZE as usize];
    harness
        .f
        .write_all_at(&data, start_lba * u64::from(SECTORSIZE))
        .unwrap();
    harness.f.close_zone(start_lba, false).unwrap();
    let zones = harness
        .f
        .report_zones(ReportOptions::All, 0)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(zones[2].zone_condition, ZoneCondition::Closed);
}

#[rstest]
fn finish_zone(harness: Harness) {
    let start_lba = 2 * u64::from(DEFAULT_ZONESIZE);
    let data = vec![42u8; SECTORSIZE as usize];
    harness
        .f
        .write_all_at(&data, start_lba * u64::from(SECTORSIZE))
        .unwrap();
    harness
        .f
        .finish_zone(start_lba, false)
        .unwrap();
    let zones = harness
        .f
        .report_zones(ReportOptions::All, 0)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(zones[2].zone_condition, ZoneCondition::Full);
}

#[rstest]
fn get_params(harness: Harness) {
    let params = harness.f.get_params().unwrap();
    assert_eq!(params.zone_mode, ZoneMode::HostManaged);
    assert!(params.supports_open());
    assert!(params.supports_close());
    assert!(params.supports_finish());
    assert!(params.supports_reset_write_pointer());
    assert!(params.unrestricted_read_in_seq_required());
}

#[rstest]
fn open_zone(harness: Harness) {
    let zdev = &harness.f;
    let start_lba = 2 * u64::from(DEFAULT_ZONESIZE);
    zdev.open_zone(start_lba, false).unwrap();
    let zones = zdev
        .report_zones(ReportOptions::All, 0)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(zones[2].zone_condition, ZoneCondition::ExplicitOpen);
}

#[allow(clippy::needless_range_loop)]   // In this case, I don't like Clippy's suggestion
#[rstest]
fn report_zones(harness: Harness) {
    let zones = harness
        .f
        .report_zones(ReportOptions::All, 0)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    for i in 0..2 {
        assert_eq!(zones[i].zone_type, ZoneType::Conventional);
        assert_eq!(zones[i].zone_condition, ZoneCondition::NotWritePointer);
        assert!(!zones[i].needs_reset());
        assert!(!zones[i].non_sequential());
        assert_eq!(
            zones[i].zone_start_lba,
            i as u64 * u64::from(DEFAULT_ZONESIZE)
        );
        assert_eq!(zones[i].zone_length, u64::from(DEFAULT_ZONESIZE));
        // Disable this assertion.  It fails due to a bug in gzoned.
        //assert!(zones[i].write_pointer_lba.is_none() );
    }
    for i in 2..zones.len() {
        assert_eq!(zones[i].zone_type, ZoneType::SeqRequired);
        assert_eq!(zones[i].zone_condition, ZoneCondition::Empty);
        assert!(!zones[i].needs_reset());
        assert!(!zones[i].non_sequential());
        let start_lba = i as u64 * u64::from(DEFAULT_ZONESIZE);
        assert_eq!(zones[i].zone_start_lba, start_lba);
        assert_eq!(zones[i].zone_length, u64::from(DEFAULT_ZONESIZE));
        assert_eq!(zones[i].write_pointer_lba, Some(start_lba));
    }
}

#[rstest]
fn reset_write_pointer(harness: Harness) {
    let start_lba = 2 * u64::from(DEFAULT_ZONESIZE);
    let data = vec![42u8; SECTORSIZE as usize];
    harness
        .f
        .write_all_at(&data, start_lba * u64::from(SECTORSIZE))
        .unwrap();
    harness
        .f
        .reset_write_pointer(start_lba, false)
        .unwrap();
    let zones = harness
        .f
        .report_zones(ReportOptions::All, 0)
        .unwrap()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(zones[2].zone_condition, ZoneCondition::Empty);
    assert_eq!(zones[2].write_pointer_lba, Some(start_lba));
}
