//! ESDM specific or privileged functions to feed and control the entropy pool

use std::ffi::CString;
use std::io::Error;

use esdm_sys::esdm;

use crate::rpc::{once_rpc, retry_rpc};

/// writes data into the ESDM auxiliary pool without crediting entropy for it
pub fn esdm_write_data(data: &[u8]) -> Result<(), Error> {
    once_rpc(
        // SAFETY: `data` is readable for `data.len()` bytes, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_write_data(data.as_ptr(), data.len()) },
        "ESDM error write",
    )
}

pub fn esdm_crng_reseed() -> Result<(), Error> {
    once_rpc(
        // SAFETY: takes no arguments, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_rnd_reseed_crng() },
        "ESDM error reseed crng",
    )
}

#[deprecated(since = "0.4.0", note = "use `esdm_crng_reseed` instead")]
pub fn esdm_reseed_crng() -> Result<(), Error> {
    esdm_crng_reseed()
}

pub fn esdm_get_entropy_count() -> Result<u32, Error> {
    let mut ent_cnt: u32 = 0;
    retry_rpc(
        // SAFETY: `ent_cnt` is a writable `unsigned int` for the whole call, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_rnd_get_ent_cnt(&raw mut ent_cnt) },
        "ESDM error get entropy",
    )?;

    Ok(ent_cnt)
}

/// adds data to the ESDM auxiliary pool and credits `entropy_count` bits of
/// entropy for it (privileged interface)
///
/// Fails with [`std::io::ErrorKind::InvalidInput`] if `entropy_count` exceeds
/// the 8 bits per byte the data can carry at most.
pub fn esdm_add_entropy(entropy_bytes: &[u8], entropy_count: u32) -> Result<(), Error> {
    let max_bits = u64::try_from(entropy_bytes.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(8);
    if u64::from(entropy_count) > max_bits {
        return Err(Error::new(
            std::io::ErrorKind::InvalidInput,
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

pub fn esdm_add_to_entropy_count(entropy_increment: u32) -> Result<(), Error> {
    once_rpc(
        // SAFETY: takes a plain integer, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_rnd_add_to_ent_cnt(entropy_increment) },
        "ESDM error add entropy count",
    )
}

pub fn esdm_clear_pool() -> Result<(), Error> {
    once_rpc(
        // SAFETY: takes no arguments, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_rnd_clear_pool() },
        "ESDM error clear pool",
    )
}

pub fn esdm_write_wakeup_thresh() -> Result<u32, Error> {
    let mut write_wakeup_thresh: u32 = 0;
    retry_rpc(
        // SAFETY: `write_wakeup_thresh` is a writable `unsigned int` for the whole call, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_get_write_wakeup_thresh(&raw mut write_wakeup_thresh) },
        "ESDM error write wakeup thresh",
    )?;

    Ok(write_wakeup_thresh)
}

/// sets the entropy rate of an entropy source (privileged interface, only
/// supported by ESDM servers built in test mode or as debug build, ESDM >= 1.2.4)
pub fn esdm_set_es_rate(es: &str, rate: u32) -> Result<(), Error> {
    let es = CString::new(es).map_err(|_| {
        Error::new(
            std::io::ErrorKind::InvalidInput,
            "entropy source name contains NUL",
        )
    })?;
    once_rpc(
        // SAFETY: `es` is a NUL-terminated string outliving the call, see `rpc.rs`.
        || unsafe { esdm::esdm_rpcc_set_es_rate(es.as_ptr(), rate) },
        "ESDM error set entropy source rate",
    )
}
