//! Rust interface to the [ESDM] (Entropy Source and DRNG Manager), a
//! user space random number server.
//!
//! The crate talks to a running `esdm-server` over the RPC client libraries
//! of the ESDM (version 1.2.4 or later), which have to be installed. It
//! offers
//!
//! - [`EsdmRng`], a cryptographically secure RNG implementing the
//!   [`rand_core`] traits, so it plugs into the [rand] ecosystem,
//! - [`EsdmConnection`], the unprivileged interface: the status of the ESDM,
//!   as text or typed ([`EsdmConnection::status`]), and the entropy count,
//! - [`EsdmPrivConnection`], the privileged interface to feed and control the
//!   entropy pool ([`EsdmPrivConnection::add_entropy`],
//!   [`EsdmPrivConnection::crng_reseed`], [`EsdmPrivConnection::selftest`],
//!   ...),
//! - [`EsdmNotification`], to wait until the ESDM asks for entropy.
//!
//! # Random numbers
//!
//! [`EsdmRng`] implements the fallible [`rand_core::TryRng`] and
//! [`rand_core::TryCryptoRng`]. Wrap it into [`rand_core::UnwrapErr`] to use
//! the infallible [`rand::Rng`](rand_core::Rng) API, which panics if the ESDM
//! cannot deliver:
//!
//! ```
//! use rand::RngExt;
//! use rand::rand_core::UnwrapErr;
//! use rand_esdm::{EsdmRng, EsdmRngType};
//!
//! let mut rng = UnwrapErr(EsdmRng::new(EsdmRngType::FullySeeded));
//! let dice = rng.random_range(1..=6);
//! assert!((1..=6).contains(&dice));
//! ```
//!
//! Or handle the errors yourself:
//!
//! ```
//! use rand::rand_core::TryRng;
//! use rand_esdm::{EsdmRng, EsdmRngType};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut rng = EsdmRng::try_new(EsdmRngType::FullySeeded)?;
//! let mut key = [0u8; 32];
//! rng.try_fill_bytes(&mut key)?;
//! # Ok(())
//! # }
//! ```
//!
//! See [`EsdmRngType`] for the difference between the two modes.
//!
//! # Connections
//!
//! Every call goes through a client connection to the server. The calls are
//! methods of the connection they need, so they cannot be made without it:
//!
//! - [`EsdmRng`] holds a connection of its own.
//! - [`EsdmConnection`] offers the unprivileged calls: status reports, the
//!   entropy count, writing data without crediting entropy.
//! - [`EsdmPrivConnection`] offers the privileged calls: crediting entropy,
//!   reseeding, clearing the pool, self tests. They fail with
//!   [`std::io::ErrorKind::PermissionDenied`] unless the process runs as
//!   root.
//!
//! Connections are cheap, several may exist at once and share the
//! connection pool of the client library:
//!
//! ```
//! use rand_esdm::EsdmConnection;
//!
//! # fn main() -> std::io::Result<()> {
//! let conn = EsdmConnection::try_new()?;
//! let status = conn.status()?;
//! println!(
//!     "ESDM {}: fully seeded: {}, entropy level: {} bits",
//!     status.library_version, status.fully_seeded, status.entropy_level_bits
//! );
//! # Ok(())
//! # }
//! ```
//!
//! # Threads
//!
//! All functions and types may be used from any thread. The client library
//! keeps a pool of connections and serves parallel requests, its size can be
//! limited with [`esdm_set_max_online_nodes`].
//!
//! # Errors
//!
//! Errors are [`std::io::Error`]s carrying the errno the ESDM reported.
//! Transient failures, like an interrupted connection, a timeout or the ESDM
//! not being fully seeded yet, are retried a few times with a short backoff
//! before they are reported. Calls which change the state of the server are
//! only repeated if they were interrupted before reaching it, so they never
//! take effect twice.
//!
//! # Examples
//!
//! The `examples` directory of the repository contains complete programs:
//! `readme` (minimal use with rand), `random` (both RNG modes, rand
//! utilities, seeding a user space RNG), `status` (typed status report) and
//! `feed_entropy` (privileged: wait until the ESDM needs entropy and provide
//! it).
//!
//! [ESDM]: https://github.com/smuellerDD/esdm
//! [rand]: https://docs.rs/rand

#![warn(missing_docs)]

mod connection;
mod entropy;
mod notification;
mod rng;
mod rpc;
mod selftest;
mod status;
mod status_json;

#[cfg(test)]
mod tests;

pub use connection::{EsdmConnection, EsdmPrivConnection, esdm_set_max_online_nodes};
pub use notification::EsdmNotification;
pub use rng::{EsdmRng, EsdmRngType};
pub use selftest::{EsdmSelftestResult, EsdmSelftestState};
pub use status::{esdm_get_entropy_level, esdm_is_fully_seeded};
pub use status_json::{
    EsdmDrngStatus, EsdmDrngType, EsdmEntropySourceStatus, EsdmPeriodicSelftestStatus,
    EsdmReseedWorkerStatus, EsdmStatus,
};
