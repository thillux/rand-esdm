//! private helpers for issuing ESDM RPC calls
//!
//! # Safety of the RPC client calls
//!
//! Every `esdm_rpcc_*` function of the ESDM >= 1.2.4 client library may be
//! called from any thread at any time. The library reference counts and locks
//! its connections, a call without an initialized service fails with
//! `-EFAULT`, and a concurrent fini waits until running calls hand back their
//! connection. Each call site therefore only has to pass valid pointers,
//! which its `SAFETY` comment states.

use libc::EINTR;
use std::ffi::c_int;
use std::io::Error;

// how often to retry RPC calls before returning an error
pub(crate) const ESDM_RETRY_COUNT: u32 = 5;

/// converts a negative errno-style ESDM return value into an [`Error`]
pub(crate) fn rpc_error(ret: impl Into<i64>, err_msg: &'static str) -> Error {
    let errno = i32::try_from(-ret.into()).unwrap_or(libc::EINVAL);
    let os_err = Error::from_raw_os_error(errno);
    Error::new(os_err.kind(), format!("{err_msg}: {os_err}"))
}

/// retries an idempotent ESDM RPC call up to [`ESDM_RETRY_COUNT`] times until
/// it reports success
///
/// Only use this for calls which can safely be issued multiple times: since
/// ESDM 1.2.4, a request without an answer may still have been processed by
/// the server.
pub(crate) fn retry_rpc(
    mut rpc: impl FnMut() -> c_int,
    err_msg: &'static str,
) -> Result<(), Error> {
    let mut ret = 0;
    for _ in 0..ESDM_RETRY_COUNT {
        ret = rpc();
        if ret == 0 {
            return Ok(());
        }
    }

    Err(rpc_error(ret, err_msg))
}

/// issues an ESDM RPC call, whose effect adds up, exactly once
///
/// It is retried only if it was interrupted before being sent to the server.
pub(crate) fn once_rpc(mut rpc: impl FnMut() -> c_int, err_msg: &'static str) -> Result<(), Error> {
    loop {
        match rpc() {
            0 => return Ok(()),
            ret if ret == -EINTR => {}
            ret => return Err(rpc_error(ret, err_msg)),
        }
    }
}
