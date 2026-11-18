//! feeding and controlling the entropy pool of the ESDM
//!
//! The functions mirror the `ioctl`s of the Linux `/dev/random` device, see
//! `random(4)`. The unprivileged ones are methods of [`EsdmConnection`], the
//! privileged ones of [`EsdmPrivConnection`].

use std::ffi::CString;
use std::io::{Error, ErrorKind};

use esdm_sys::esdm;

use crate::connection::{EsdmConnection, EsdmPrivConnection};
use crate::rpc::{once_rpc, retry_rpc};

impl EsdmConnection {
    /// writes data into the ESDM auxiliary pool without crediting entropy
    /// for it
    ///
    /// Like writing to `/dev/random`: the data is mixed in, but does not raise
    /// the entropy level. To credit entropy, see
    /// [`EsdmPrivConnection::add_entropy`].
    pub fn write_data(&self, data: &[u8]) -> Result<(), Error> {
        once_rpc(
            // SAFETY: `data` is readable for `data.len()` bytes, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_write_data(data.as_ptr(), data.len()) },
            "ESDM error write",
        )
    }

    /// entropy count of the auxiliary pool in bits
    ///
    /// Like the `RNDGETENTCNT` `ioctl`.
    pub fn entropy_count(&self) -> Result<u32, Error> {
        let mut ent_cnt: u32 = 0;
        retry_rpc(
            // SAFETY: `ent_cnt` is a writable `unsigned int` for the whole call, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_rnd_get_ent_cnt(&raw mut ent_cnt) },
            "ESDM error get entropy",
        )?;

        Ok(ent_cnt)
    }

    /// entropy level in bits below which the ESDM asks for more entropy
    ///
    /// Like `/proc/sys/kernel/random/write_wakeup_threshold`: below it,
    /// waiters of [`crate::EsdmNotification`] are woken up.
    pub fn write_wakeup_thresh(&self) -> Result<u32, Error> {
        let mut write_wakeup_thresh: u32 = 0;
        retry_rpc(
            // SAFETY: `write_wakeup_thresh` is a writable `unsigned int` for the whole call, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_get_write_wakeup_thresh(&raw mut write_wakeup_thresh) },
            "ESDM error write wakeup thresh",
        )?;

        Ok(write_wakeup_thresh)
    }
}

impl EsdmPrivConnection {
    /// adds data to the ESDM auxiliary pool and credits `entropy_count` bits
    /// of entropy for it
    ///
    /// Like the `RNDADDENTROPY` `ioctl`. Only credit what the data really
    /// carries: overestimating weakens every random number the ESDM produces.
    ///
    /// Fails with [`ErrorKind::InvalidInput`] if `entropy_count` exceeds the 8
    /// bits per byte the data can carry at most.
    pub fn add_entropy(&self, entropy_bytes: &[u8], entropy_count: u32) -> Result<(), Error> {
        let max_bits = u64::try_from(entropy_bytes.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(8);
        if u64::from(entropy_count) > max_bits {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                format!(
                    "cannot credit {entropy_count} bits of entropy to {} bytes",
                    entropy_bytes.len()
                ),
            ));
        }

        once_rpc(
            // SAFETY: `entropy_bytes` is readable for `entropy_bytes.len()` bytes, see `rpc.rs`.
            || unsafe {
                esdm::esdm_rpcc_rnd_add_entropy(
                    entropy_bytes.as_ptr(),
                    entropy_bytes.len(),
                    entropy_count,
                )
            },
            "ESDM error add entropy",
        )
    }

    /// credits `entropy_increment` bits of entropy to the auxiliary pool
    /// without adding data
    ///
    /// Like the `RNDADDTOENTCNT` `ioctl`. Meant for testing: crediting entropy
    /// which was never added weakens the ESDM.
    pub fn add_to_entropy_count(&self, entropy_increment: u32) -> Result<(), Error> {
        once_rpc(
            // SAFETY: takes a plain integer, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_rnd_add_to_ent_cnt(entropy_increment) },
            "ESDM error add entropy count",
        )
    }

    /// clears the auxiliary pool and resets its entropy count to 0
    ///
    /// Like the `RNDCLEARPOOL` `ioctl`.
    pub fn clear_pool(&self) -> Result<(), Error> {
        once_rpc(
            // SAFETY: takes no arguments, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_rnd_clear_pool() },
            "ESDM error clear pool",
        )
    }

    /// triggers a reseed of the DRNGs from the entropy sources
    ///
    /// Like the `RNDRESEEDCRNG` `ioctl`.
    pub fn crng_reseed(&self) -> Result<(), Error> {
        once_rpc(
            // SAFETY: takes no arguments, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_rnd_reseed_crng() },
            "ESDM error reseed crng",
        )
    }

    /// sets the entropy rate of an entropy source
    ///
    /// `es` is the name of the source as listed in
    /// [`crate::EsdmEntropySourceStatus::name`]. Only supported by ESDM
    /// servers built in test mode or as debug build, meant for testing.
    ///
    /// Fails with [`ErrorKind::InvalidInput`] if `es` contains a NUL byte.
    pub fn set_es_rate(&self, es: &str, rate: u32) -> Result<(), Error> {
        let es = CString::new(es)
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "entropy source name contains NUL"))?;
        once_rpc(
            // SAFETY: `es` is a NUL-terminated string outliving the call, see `rpc.rs`.
            || unsafe { esdm::esdm_rpcc_set_es_rate(es.as_ptr(), rate) },
            "ESDM error set entropy source rate",
        )
    }
}
