use std::{
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
};

use freebsd_zonecmd::{gzoned, *};
use gzoned::{DEFAULT_ZONESIZE, SECTORSIZE};
use rstest::{fixture, rstest};

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
