//! setup and teardown of the (privileged and unprivileged) ESDM RPC client
//! connections
//!
//! Since ESDM 1.2.4 the client library reference counts its services under a
//! lock: only the first init sets a service up and the matching last fini
//! releases it, a fini without a reference is a no-op. The guards below
//! therefore map directly onto init and fini.
//!
//! Every call which needs a service is a method of its guard, so the type
//! system guarantees that the service is initialized while it runs.

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
/// The unprivileged interface of the ESDM is available as its methods:
/// the status ([`Self::status`], [`Self::is_fully_seeded`],
/// [`Self::entropy_level`], ...), the entropy count ([`Self::entropy_count`])
/// and writing data to the auxiliary pool ([`Self::write_data`]). Random
/// numbers come from [`crate::EsdmRng`], which holds a connection of its own.
///
/// Connections are cheap: the client library connects to the server lazily,
/// on the first request, and all connections of a process share its
/// connection pool. They can be shared between threads by reference, or
/// cloned for threads which outlive the current scope.
///
/// ```
/// use rand_esdm::EsdmConnection;
///
/// # fn main() -> std::io::Result<()> {
/// let conn = EsdmConnection::try_new()?;
/// println!("auxiliary pool: {} bits", conn.entropy_count()?);
///
/// let worker = conn.clone();
/// std::thread::spawn(move || worker.status()).join().unwrap()?;
/// # Ok(())
/// # }
/// ```
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
    ///
    /// Succeeds without a running server, as the client library connects on
    /// the first request, see the [crate documentation](crate#errors).
    pub fn try_new() -> Result<Self, Error> {
        init_unpriv()?;
        Ok(Self { _private: () })
    }
}

/// takes another reference on the client service
///
/// While `self` is alive the service is set up, so the library only counts
/// the reference, which cannot fail.
impl Clone for EsdmConnection {
    fn clone(&self) -> Self {
        Self::try_new().expect("taking another reference on the initialized ESDM service failed")
    }
}

impl Drop for EsdmConnection {
    fn drop(&mut self) {
        fini_unpriv();
    }
}

/// privileged client connection to the ESDM, released when dropped
///
/// The privileged interface of the ESDM is available as its methods: adding
/// entropy ([`Self::add_entropy`]), reseeding ([`Self::crng_reseed`]),
/// clearing the pool ([`Self::clear_pool`]) and running the self tests
/// ([`Self::selftest`]). They fail with
/// [`std::io::ErrorKind::PermissionDenied`] unless the process runs as root;
/// setting up the connection itself works without.
///
/// ```no_run
/// use rand_esdm::EsdmPrivConnection;
///
/// # fn main() -> std::io::Result<()> {
/// let priv_conn = EsdmPrivConnection::try_new()?;
/// priv_conn.crng_reseed()?;
/// # Ok(())
/// # }
/// ```
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
    ///
    /// Succeeds without a running server and without root privileges, the
    /// calls report both, see the [crate documentation](crate#errors).
    pub fn try_new() -> Result<Self, Error> {
        init_priv()?;
        Ok(Self { _private: () })
    }
}

/// takes another reference on the privileged client service, see the
/// `Clone` implementation of [`EsdmConnection`]
impl Clone for EsdmPrivConnection {
    fn clone(&self) -> Self {
        Self::try_new()
            .expect("taking another reference on the initialized privileged ESDM service failed")
    }
}

impl Drop for EsdmPrivConnection {
    fn drop(&mut self) {
        fini_priv();
    }
}

/// Limits the number of parallel RPC connections the client library uses.
///
/// By default the library opens one connection per CPU, so as many requests
/// can be served in parallel. Since ESDM 1.2.4 the limit can only be lowered,
/// every call keeps the smaller of the current and the requested value, and 0
/// counts as 1. It has to be set before the first connection is set up (e.g.
/// by [`EsdmConnection::new`] or [`crate::EsdmRng::new`]): the connections of
/// an initialized service are not resized. A call coming too late still
/// succeeds, it just has no effect on the existing connections.
pub fn esdm_set_max_online_nodes(nodes: u32) -> Result<(), Error> {
    // SAFETY: takes a plain integer, the library stores it atomically.
    let ret = unsafe { esdm::esdm_rpcc_set_max_online_nodes(nodes) };
    if ret < 0 {
        return Err(rpc_error(ret, "ESDM error set max online nodes"));
    }

    Ok(())
}
