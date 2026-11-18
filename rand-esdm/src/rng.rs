//! [`EsdmRng`] and its `rand_core` trait implementations

use libc::EAGAIN;
use rand_core::{TryCryptoRng, TryRng};
use zeroize::Zeroizing;

use esdm_sys::esdm;

use std::io::Error;

use crate::connection::EsdmConnection;
use crate::rpc::{rpc_error, should_retry};

/// which DRNG of the ESDM serves an [`EsdmRng`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EsdmRngType {
    /// output of the prediction resistant DRNG, which is reseeded with fresh
    /// entropy for every request (`getrandom(GRND_RANDOM)` semantics)
    ///
    /// The output never exceeds the entropy the ESDM collected, so it is
    /// limited by the rate of the entropy sources. Depending on them this can
    /// be as slow as a few KiB per second, and a request blocks until enough
    /// entropy is available. Use it for long term secrets only, where its
    /// stronger guarantee is worth the cost.
    PredictionResistant,

    /// output of the DRNG instances, which only produce random numbers once
    /// the ESDM is fully seeded (`getrandom()` semantics)
    ///
    /// Fast and the right choice in almost all cases. Before the ESDM is
    /// fully seeded, requests fail after a few internal tries.
    FullySeeded,
}

/// cryptographically secure RNG backed by the ESDM
///
/// It implements the fallible [`TryRng`] and [`TryCryptoRng`] traits of
/// `rand_core`. Wrap it into [`rand_core::UnwrapErr`] for the infallible
/// `Rng`/`CryptoRng` traits, which panic on errors instead.
///
/// The RNG keeps no state besides its client connection, every request is
/// served by the ESDM. Large requests are split up and short reads are
/// continued, so [`TryRng::try_fill_bytes`] either fills the whole buffer or
/// fails.
///
/// ```
/// use rand::rand_core::TryRng;
/// use rand_esdm::{EsdmRng, EsdmRngType};
///
/// # fn main() -> std::io::Result<()> {
/// let mut rng = EsdmRng::try_new(EsdmRngType::FullySeeded)?;
/// let nonce = rng.try_next_u64()?;
/// # let _ = nonce;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct EsdmRng {
    rng_type: EsdmRngType,
    // keeps the client connection alive as long as the RNG exists
    _conn: EsdmConnection,
}

impl EsdmRng {
    /// sets up the client connection to the ESDM server for this RNG
    ///
    /// # Panics
    ///
    /// if no connection to the ESDM server can be set up, see [`Self::try_new`]
    #[must_use]
    pub fn new(rng_type: EsdmRngType) -> Self {
        Self::try_new(rng_type).expect("unable to initialize ESDM connection")
    }

    /// sets up the client connection to the ESDM server for this RNG
    ///
    /// Fails if the client library cannot set up its connection, e.g.
    /// because it runs out of memory. A server which is not running is only
    /// noticed by the first request.
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
