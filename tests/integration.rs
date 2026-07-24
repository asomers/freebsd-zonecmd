
mod gzoned {
    use std::{
        ffi::OsStr,
        io,
        os::unix::ffi::OsStrExt,
        path::{Path, PathBuf},
        process::Command,
        range::RangeInclusive
    };

    use mdconfig::Md;
    use tempfile::NamedTempFile;

    const SECTORSIZE: u32 = 4096;

    /// Used to construct a [`Gzoned`];
    #[derive(Debug)]
    pub struct Builder {
        conventional_zones: Vec<RangeInclusive<u64>>,
        sectors: u64,
        zonesize: u32
    }

    impl Builder {
        pub fn build(self) -> io::Result<Gzoned> {
            let mut tf = NamedTempFile::new()?;
            tf.as_file_mut().set_len(u64::from(SECTORSIZE) * self.sectors)?;
            let md = mdconfig::Builder::vnode(tf.path()).create()?;

            let conventional_zones = self.conventional_zones.into_iter()
                .map(|r| {
                     if 1 + r.last - r.start == 1 {
                         format!("{}", r.start)
                     } else {
                         format!("{}-{}", r.start, r.last)
                     }
                }).collect::<Vec<_>>()
                .join(",");
            let output = Command::new("gzoned")
                .args(["create", "-s"])
                .arg(format!("{}", self.zonesize))
                .arg("-r")
                .arg(conventional_zones)
                .output()?;
            let l = output.stdout.len() - 1;    // Strip the trailing "\n"
            let gzoned_dev = OsStr::from_bytes(&output.stdout[0..l]);
            let pb = Path::new("/dev").join(gzoned_dev);
            Ok(Gzoned{pb, md, tf})
        }

        /// Add an inclusive range of zones that should be treated as conventional, not sequential
        pub fn conventional_zones(&mut self, zones: RangeInclusive<u64>) -> &mut Self {
            assert!(self.conventional_zones.len() < 16,
                "gzoned has a maximum of 16 conventional zone ranges");
            self.conventional_zones.push(zones);
            self
        }

        /// Set the total size of the device, in 4k sectors
        pub fn sectors(&mut self, sectors: u64) -> &mut Self {
            self.sectors = sectors;
            self
        }

        /// Set the simulated zone size, in sectors
        pub fn zonesize(&mut self, size: u32) -> &mut Self {
            self.zonesize = size;
            self
        }
    }

    impl Default for Builder {
        fn default() -> Self {
            Builder { zonesize: 65536 , sectors: 262144, conventional_zones: Default::default() }
        }
    }

    /// A temporary gzoned(8) device that will clean up after itself on Drop
    pub struct Gzoned {
        pb: PathBuf,
        md: Md,
        tf: NamedTempFile
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

