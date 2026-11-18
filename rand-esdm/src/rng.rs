//! [`EsdmRng`] and its `rand_core` trait implementations

use libc::EAGAIN;
use rand_core::{TryCryptoRng, TryRng};
use zeroize::Zeroizing;

use esdm_sys::esdm;

use std::io::Error;

use crate::connection::EsdmConnection;
use crate::rpc::{rpc_error, should_retry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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
    // keeps the client connection alive as long as the RNG exists
    _conn: EsdmConnection,
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
        Ok(Self {
            rng_type,
            _conn: EsdmConnection::try_new()?,
        })
    }
}

/*
 * rand_core trait implementations
 */
impl TryRng for EsdmRng {
    type Error = Error;

    // The stack buffers below are wiped on every return path. This is best
    // effort only: the returned value itself and any copies the compiler
    // makes of it are not covered.

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut bytes = Zeroizing::new([0u8; 4]);
        self.try_fill_bytes(&mut *bytes)?;

        Ok(u32::from_ne_bytes(*bytes))
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut bytes = Zeroizing::new([0u8; 8]);
        self.try_fill_bytes(&mut *bytes)?;

        Ok(u64::from_ne_bytes(*bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let mut filled = 0;
        // consecutive failures, any progress resets them
        let mut failures = 0;

        while filled < dst.len() {
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
                Ok(n) if n > 0 => {
                    filled += n.min(remaining.len());
                    failures = 0;
                }
                _ => {
                    // no bytes without an error is treated as a temporary
                    // refusal
                    let ret = if ret == 0 {
                        -i64::from(EAGAIN)
                    } else {
                        i64::try_from(ret).unwrap_or(-i64::from(libc::EIO))
                    };
                    if !should_retry(ret, failures) {
                        return Err(rpc_error(ret, "Unable to fetch random bytes from ESDM"));
                    }
                    failures += 1;
                }
            }
        }

        Ok(())
    }
}

/// the ESDM is a cryptographically secure RNG, in both modes
impl TryCryptoRng for EsdmRng {}
