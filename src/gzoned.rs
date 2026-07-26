//! Helpers for creating temporary [gzoned(8)](https://man.freebsd.org/cgi/man.cgi?query=gzoned)
//! devices for testing and development.
use std::{
    io,
    ops::RangeInclusive,
    path::{Path, PathBuf},
    process::Command,
};

use mdconfig::Md;
use tempfile::NamedTempFile;

/// Default zone size in sectors.
pub const DEFAULT_ZONESIZE: u32 = 65536;

/// Sector size in bytes used by [`Builder`].
pub const SECTORSIZE: u32 = 4096;

/// Used to construct a [`Gzoned`].
#[derive(Debug)]
pub struct Builder {
    conventional_zones: Vec<RangeInclusive<u64>>,
    sectors:            u64,
    /// Size of a zone, in bytes.
    zonesize:           u32,
}

impl Builder {
    /// Create a temporary gzoned device from this builder's settings.
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

    /// Add an inclusive range of zones that should be treated as conventional, not sequential.
    pub fn conventional_zones(mut self, zones: RangeInclusive<u64>) -> Self {
        assert!(
            self.conventional_zones.len() < 16,
            "gzoned has a maximum of 16 conventional zone ranges"
        );
        self.conventional_zones.push(zones);
        self
    }

    /// Set the total size of the device, in 4k sectors.
    pub fn sectors(mut self, sectors: u64) -> Self {
        self.sectors = sectors;
        self
    }

    /// Set the simulated zone size, in sectors.
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

/// A temporary gzoned(8) device that will clean up after itself on drop.
#[derive(Debug)]
pub struct Gzoned {
    pb:  PathBuf,
    _md: Md,
    _tf: NamedTempFile,
}

impl Gzoned {
    /// Return the path to the gzoned device node.
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
