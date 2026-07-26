//! Report zone parameters and zone layout for a zoned block device.
//!
//! ```sh
//! cargo run --example zoneinfo -- /dev/da0
//! ```
use std::{env, fs::File, process};

use freebsd_zonecmd::{DiskParams, ReportOptions, ZoneCondition, ZoneEntry, ZonedDevice};

fn main() {
    let devpath = match env::args().nth(1) {
        Some(path) => path,
        None => {
            eprintln!("usage: {} <device>", env::args().next().unwrap());
            process::exit(1);
        }
    };

    let file = File::open(&devpath).unwrap_or_else(|e| {
        eprintln!("{devpath}: {e}");
        process::exit(1);
    });

    let params = file.get_params().unwrap_or_else(|e| {
        eprintln!("get_params: {e}");
        process::exit(1);
    });
    print_params(&params);

    let zones = file
        .report_zones(ReportOptions::All, 0)
        .unwrap_or_else(|e| {
            eprintln!("report_zones: {e}");
            process::exit(1);
        });

    let header = zones.header();
    println!(
        "{} zones, Maximum LBA {:#x} ({})",
        zones.entries_available(),
        header.maximum_lba,
        header.maximum_lba,
    );
    println!("{:?}", header.same);
    println!();

    println!(
        "{:>11} {:>6} {:>11} {:>13} {:>13} {:>14} {:>16}",
        "Start LBA", "Length", "WP LBA", "Zone Type", "Condition", "Sequential", "Reset",
    );

    for entry in zones {
        let entry = entry.unwrap_or_else(|e| {
            eprintln!("report_zones: {e}");
            process::exit(1);
        });
        print_zone(&entry);
    }
}

fn print_params(params: &DiskParams) {
    println!("Zone Mode: {:?}", params.zone_mode);

    print!("Command support:");
    let mut commands = Vec::new();
    if params.supports_report_zones() {
        commands.push("Report Zones");
    }
    if params.supports_open() {
        commands.push("Open");
    }
    if params.supports_close() {
        commands.push("Close");
    }
    if params.supports_finish() {
        commands.push("Finish");
    }
    if params.supports_reset_write_pointer() {
        commands.push("Reset Write Pointer");
    }
    if commands.is_empty() {
        println!(" None");
    } else {
        println!(" {}", commands.join(", "));
    }

    println!(
        "Unrestricted Read in Sequential Write Required Zone (URSWRZ): {}",
        params.unrestricted_read_in_seq_required()
    );

    println!(
        "Optimal Number of Open Sequential Write Preferred Zones: {:?}",
        params.optimal_seq_zones
    );
    println!(
        "Optimal Number of Non-Sequentially Written Sequential Write Preferred Zones: {:?}",
        params.optimal_nonseq_zones
    );
    println!(
        "Maximum Number of Open Sequential Write Required Zones: {:?}",
        params.max_seq_zones
    );
    println!();
}

fn print_zone(entry: &ZoneEntry) {
    let wp = match entry.write_pointer_lba {
        None => "-1".to_string(),
        Some(lba) => format!("{lba:#x}"),
    };

    let sequential = if entry.non_sequential() {
        "Conventional"
    } else {
        "Sequential"
    };

    let reset = if entry.needs_reset() {
        "Reset Needed"
    } else {
        "No Reset Needed"
    };

    println!(
        "{:>11} {:>6} {:>11} {:>13} {:>13} {:>14} {:>16}",
        format!("{:#x}", entry.zone_start_lba),
        entry.zone_length,
        wp,
        format!("{:?}", entry.zone_type),
        zone_condition_str(entry.zone_condition),
        sequential,
        reset,
    );
}

fn zone_condition_str(condition: ZoneCondition) -> &'static str {
    match condition {
        ZoneCondition::NotWritePointer => "NWP",
        ZoneCondition::Empty => "Empty",
        ZoneCondition::ImplicitOpen => "Implicit Open",
        ZoneCondition::ExplicitOpen => "Explicit Open",
        ZoneCondition::Closed => "Closed",
        ZoneCondition::ReadOnly => "Readonly",
        ZoneCondition::Full => "Full",
        ZoneCondition::Offline => "Offline",
        ZoneCondition::Unknown(_) => "Unknown",
    }
}
