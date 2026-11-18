//! [`EsdmRng`] and its `rand_core` trait implementations

use libc::EAGAIN;
use rand_core::TryRng;

use esdm_sys::esdm;

use std::io::Error;

use crate::connection::{esdm_rng_fini, esdm_rng_init};
use crate::rpc::{ESDM_RETRY_COUNT, rpc_error};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsdmRngType {
    /// ESDM RNG implementation, which uses fresh entropy for every random output produced
    PredictionResistant,

    /// ESDM RNG implementation, which only produces random numbers when fully seeded
    /// otherwise it times out and returns an error after a few internal tries
    FullySeeded,
}

#[derive(Debug)]
pub struct EsdmRng {
    rng_type: EsdmRngType,
}

impl EsdmRng {
    /// # Panics
    ///
    /// if no connection to the ESDM server can be set up, see [`Self::try_new`]
    #[must_use]
    pub fn new(rng_type: EsdmRngType) -> Self {
        Self::try_new(rng_type).expect("unable to initialize ESDM connection")
    }

    /// sets up the client connection to the ESDM server for this RNG
    pub fn try_new(rng_type: EsdmRngType) -> Result<Self, Error> {
        if esdm_rng_init() {
            Ok(Self { rng_type })
        } else {
            Err(Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "unable to initialize ESDM connection",
            ))
        }
    }
}

impl Drop for EsdmRng {
    fn drop(&mut self) {
        esdm_rng_fini();
    }
}

/*
 * rand_core trait implementations
 */
impl TryRng for EsdmRng {
    type Error = Error;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(u32::try_from(self.try_next_u64()? & 0xFF_FF_FF_FF).unwrap())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut bytes: [u8; 8] = [0; 8];
        self.try_fill_bytes(&mut bytes)?;

        Ok(u64::from_ne_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let mut filled = 0;
        let mut failures = 0;
        let mut last_ret: isize = 0;

        while filled < dst.len() {
            if failures == ESDM_RETRY_COUNT {
                return Err(rpc_error(
                    i64::try_from(last_ret).unwrap_or(-i64::from(libc::EIO)),
                    "Unable to fetch random bytes from ESDM",
                ));
            }

            let remaining = &mut dst[filled..];
            let ret = match self.rng_type {
                // SAFETY: `remaining` is writable for `remaining.len()` bytes
                // and the library writes at most that many, see `rpc.rs`.
                EsdmRngType::FullySeeded => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_full(remaining.as_mut_ptr(), remaining.len())
                },
                // SAFETY: `remaining` is writable for `remaining.len()` bytes
                // and the library writes at most that many, see `rpc.rs`.
                EsdmRngType::PredictionResistant => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_pr(remaining.as_mut_ptr(), remaining.len())
                },
            };

            match usize::try_from(ret) {
                // short reads are possible, continue with the remainder
                Ok(n) if n > 0 => filled += n.min(remaining.len()),
                _ => {
                    last_ret = if ret == 0 {
                        -isize::try_from(EAGAIN).unwrap()
                    } else {
                        ret
                    };
                    failures += 1;
                }
            }
        }

        Ok(())
    }
}
