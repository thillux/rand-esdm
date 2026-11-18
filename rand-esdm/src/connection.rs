//! setup and teardown of the (privileged and unprivileged) ESDM RPC client
//! connections
//!
//! Since ESDM 1.2.4 the client library reference counts its services under a
//! lock: only the first init sets a service up and the matching last fini
//! releases it, a fini without a reference is a no-op. The guards below
//! therefore map directly onto init and fini.
//! Mixing them with the deprecated free functions is memory safe, but breaks
//! the reference counting, see [`EsdmConnection`].

use std::io::Error;

use esdm_sys::esdm;

use crate::rpc::rpc_error;

fn init_unpriv() -> Result<(), Error> {
    // SAFETY: no interrupt callback is passed, so no pointer crosses the FFI
    // boundary, see the notes in `rpc.rs`.
    let ret = unsafe { esdm::esdm_rpcc_init_unpriv_service(None) };
    if ret != 0 {
        return Err(rpc_error(ret, "unable to initialize ESDM connection"));
    }
    Ok(())
}

fn fini_unpriv() {
    // SAFETY: drops one reference, a no-op if none is held. Calls still
    // running keep their connection, see `rpc.rs`.
    unsafe { esdm::esdm_rpcc_fini_unpriv_service() };
}

fn init_priv() -> Result<(), Error> {
    // SAFETY: no interrupt callback is passed, so no pointer crosses the FFI
    // boundary, see the notes in `rpc.rs`.
    let ret = unsafe { esdm::esdm_rpcc_init_priv_service(None) };
    if ret != 0 {
        return Err(rpc_error(
            ret,
            "unable to initialize privileged ESDM connection",
        ));
    }
    Ok(())
}

fn fini_priv() {
    // SAFETY: drops one reference, a no-op if none is held. Calls still
    // running keep their connection, see `rpc.rs`.
    unsafe { esdm::esdm_rpcc_fini_priv_service() };
}

/// unprivileged client connection to the ESDM, released when dropped
///
/// The free functions of this crate that talk to the unprivileged interface
/// (e.g. [`crate::esdm_status`]) need one to be alive. Several connections
/// may exist at once, they share the client service of the library.
/// [`crate::EsdmRng`] holds its own.
///
/// The deprecated free functions (e.g. [`esdm_rng_fini`]) take and drop the
/// same references as the connections. A surplus `esdm_rng_fini*` call drops
/// a reference a live connection relies on: the connection is torn down under
/// it, and the connection later releases whatever reference was taken after
/// that. Do not mix both.
#[derive(Debug)]
#[must_use = "the connection is released again when dropped"]
pub struct EsdmConnection {
    // keeps the handle from being built without `try_new`, which takes the
    // reference that `Drop` releases
    _private: (),
}

impl EsdmConnection {
    /// # Panics
    ///
    /// if the connection cannot be set up, see [`Self::try_new`]
    // no `Default`: it would panic without a reachable ESDM server
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::try_new().expect("unable to initialize ESDM connection")
    }

    /// sets up the unprivileged client connection to the ESDM
    pub fn try_new() -> Result<Self, Error> {
        init_unpriv()?;
        Ok(Self { _private: () })
    }
}

impl Drop for EsdmConnection {
    fn drop(&mut self) {
        fini_unpriv();
    }
}

/// privileged client connection to the ESDM, released when dropped
///
/// The privileged functions of this crate (e.g. [`crate::esdm_add_entropy`])
/// need one to be alive and the process to run as root.
#[derive(Debug)]
#[must_use = "the connection is released again when dropped"]
pub struct EsdmPrivConnection {
    // see `EsdmConnection`
    _private: (),
}

impl EsdmPrivConnection {
    /// # Panics
    ///
    /// if the connection cannot be set up, see [`Self::try_new`]
    // no `Default`: it would panic without a reachable ESDM server
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::try_new().expect("unable to initialize privileged ESDM connection")
    }

    /// sets up the privileged client connection to the ESDM
    pub fn try_new() -> Result<Self, Error> {
        init_priv()?;
        Ok(Self { _private: () })
    }
}

impl Drop for EsdmPrivConnection {
    fn drop(&mut self) {
        fini_priv();
    }
}

/// Returns if the client connection to ESDM was initialized successfully
///
/// Do not mix with [`EsdmConnection`], see its notes.
#[deprecated(
    since = "0.5.0",
    note = "use `EsdmConnection`, which is released when dropped"
)]
#[must_use]
pub fn esdm_rng_init() -> bool {
    init_unpriv().is_ok()
}

/// initializes the client connection to ESDM, asserts if something goes wrong
///
/// Do not mix with [`EsdmConnection`], see its notes.
#[deprecated(since = "0.5.0", note = "use `EsdmConnection::new`")]
pub fn esdm_rng_init_checked() {
    if let Err(e) = init_unpriv() {
        panic!("{e}");
    }
}

/// releases a reference taken by [`esdm_rng_init`], a no-op if none is held
///
/// Do not mix with [`EsdmConnection`], see its notes.
#[deprecated(since = "0.5.0", note = "drop the `EsdmConnection` instead")]
pub fn esdm_rng_fini() {
    fini_unpriv();
}

/// Limits the number of parallel RPC connections the client library uses.
///
/// Since ESDM 1.2.4 the limit can only be lowered and has to be set before the
/// RPC connection is initialized (e.g. by [`EsdmConnection::new`] or
/// [`crate::EsdmRng::new`]), as connections of an initialized service are not
/// resized.
pub fn esdm_set_max_online_nodes(nodes: u32) -> Result<(), Error> {
    // SAFETY: takes a plain integer, the library stores it atomically.
    let ret = unsafe { esdm::esdm_rpcc_set_max_online_nodes(nodes) };
    if ret < 0 {
        return Err(rpc_error(ret, "ESDM error set max online nodes"));
    }

    Ok(())
}

/// Returns if the privileged client connection to ESDM was initialized
/// successfully
///
/// Do not mix with [`EsdmPrivConnection`], see the notes of [`EsdmConnection`].
#[deprecated(
    since = "0.5.0",
    note = "use `EsdmPrivConnection`, which is released when dropped"
)]
#[must_use]
pub fn esdm_rng_init_priv() -> bool {
    init_priv().is_ok()
}

/// initializes the privileged client connection to ESDM, asserts if something
/// goes wrong
///
/// Do not mix with [`EsdmPrivConnection`], see the notes of [`EsdmConnection`].
#[deprecated(since = "0.5.0", note = "use `EsdmPrivConnection::new`")]
pub fn esdm_rng_init_priv_checked() {
    if let Err(e) = init_priv() {
        panic!("{e}");
    }
}

/// releases a reference taken by [`esdm_rng_init_priv`], a no-op if none is
/// held
///
/// Do not mix with [`EsdmPrivConnection`], see the notes of [`EsdmConnection`].
#[deprecated(since = "0.5.0", note = "drop the `EsdmPrivConnection` instead")]
pub fn esdm_rng_fini_priv() {
    fini_priv();
}
