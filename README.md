# freebsd-zonecmd

Rust bindings to FreeBSD's `DIOCZONECMD` ioctl for zoned block devices.

Zoned devices conform to the SCSI Zoned Block Commands (ZBC) and ATA Zoned ATA
Command Set (ZAC) specifications, used for shingled magnetic recording (SMR)
hard disks and some solid state disks, too. This crate provides bindings
equivalent to what the [zonectl(8)] utility provides, but in idiomatic Rust.

[Documentation](https://docs.rs/freebsd-zonecmd)

[zonectl(8)]: https://man.freebsd.org/cgi/man.cgi?query=zonectl

# Usage

Open a block device, wrap it in a [`ZonedDevice`], and call its methods. The device
file must remain open for as long as the `ZonedDevice` is used.

```rust
use std::fs::File;

use freebsd_zonecmd::{ZonedDevice, ReportOptions};

let file = File::open("/dev/da0")?;
let dev = ZonedDevice::new(&file);

let params = dev.get_params()?;
println!("{:?}", params.zone_mode);

let zones = dev.report_zones(ReportOptions::All, 0)?;
println!("{} zones", zones.entries_available());
for entry in zones {
    println!("{:?}", entry?);
}
```

See also the [zoneinfo](examples/zoneinfo.rs) example:

```sh
cargo run --example zoneinfo -- /dev/da0
```

# Platforms

This crate only works on FreeBSD 11.0 or later.

# Minimum Supported Rust Version (MSRV)

`freebsd-zonecmd` does not guarantee any specific MSRV. Rather, it guarantees
compatibility with the oldest rustc shipped in the FreeBSD package collection.

* https://www.freshports.org/lang/rust/

# License

`freebsd-zonecmd` is primarily distributed under the terms of both the MIT
license and the Apache License (Version 2.0).

See LICENSE-APACHE, and LICENSE-MIT for details.
