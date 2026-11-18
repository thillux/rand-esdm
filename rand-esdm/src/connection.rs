//! setup and teardown of the (privileged and unprivileged) ESDM RPC client
//! connections

use std::io::Error;
use std::sync::{Mutex, MutexGuard, PoisonError};

use esdm_sys::esdm;

use crate::rpc::rpc_error;

static LIB_MUTEX_UNPRIV: Mutex<u32> = Mutex::new(0u32);
static LIB_MUTEX_PRIV: Mutex<u32> = Mutex::new(0u32);

/// locks a connection counter, which stays consistent even if another thread
/// panicked while holding it
fn lock(counter: &Mutex<u32>) -> MutexGuard<'_, u32> {
    counter.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Returns if the client connection to ESDM was initialized successfully
/// Only needed to call once globally before first usage of ESDM
#[must_use]
pub fn esdm_rng_init() -> bool {
    let mut guard = lock(&LIB_MUTEX_UNPRIV);

    let ret = if *guard == 0 {
        // SAFETY: no interrupt callback is passed, so no pointer crosses the
        // FFI boundary, see the notes in `rpc.rs`.
        unsafe { esdm::esdm_rpcc_init_unpriv_service(None) == 0 }
    } else {
        true
    };

    if ret {
        *guard += 1;
    }

    ret
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM
pub fn esdm_rng_init_checked() {
    let success = esdm_rng_init();
    assert!(success);
}

/// Call in order to free resources needed for ESDM client connection
pub fn esdm_rng_fini() {
    let mut guard = lock(&LIB_MUTEX_UNPRIV);
    assert_ne!(*guard, 0);

    if *guard == 1 {
        // SAFETY: releases the reference taken by the init when the counter
        // left 0. Calls still running keep their connection, see `rpc.rs`.
        unsafe { esdm::esdm_rpcc_fini_unpriv_service() };
    }

    *guard -= 1;
}

/// Limits the number of parallel RPC connections the client library uses.
///
/// Since ESDM 1.2.4 the limit can only be lowered and has to be set before the
/// RPC connection is initialized (e.g. by [`esdm_rng_init`] or
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

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM (privileged mode)
#[must_use]
pub fn esdm_rng_init_priv() -> bool {
    let mut guard = lock(&LIB_MUTEX_PRIV);

    let ret = if *guard == 0 {
        // SAFETY: no interrupt callback is passed, so no pointer crosses the
        // FFI boundary, see the notes in `rpc.rs`.
        unsafe { esdm::esdm_rpcc_init_priv_service(None) == 0 }
    } else {
        true
    };

    if ret {
        *guard += 1;
    }

    ret
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM (privileged mode)
pub fn esdm_rng_init_priv_checked() {
    let success = esdm_rng_init_priv();
    assert!(success);
}

/// Call in order to free resources needed for ESDM client connection (privileged mode)
pub fn esdm_rng_fini_priv() {
    let mut guard = lock(&LIB_MUTEX_PRIV);
    assert_ne!(*guard, 0);

    if *guard == 1 {
        // SAFETY: releases the reference taken by the init when the counter
        // left 0. Calls still running keep their connection, see `rpc.rs`.
        unsafe { esdm::esdm_rpcc_fini_priv_service() };
    }

    *guard -= 1;
}
