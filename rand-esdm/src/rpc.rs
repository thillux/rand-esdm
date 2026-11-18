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
    let errno = ret
        .into()
        .checked_neg()
        .and_then(|errno| i32::try_from(errno).ok())
        .unwrap_or(libc::EINVAL);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn rpc_error_maps_errno() {
        let err = rpc_error(-libc::ETIMEDOUT, "msg");
        assert_eq!(err.kind(), ErrorKind::TimedOut);
        assert!(err.to_string().starts_with("msg: "));

        assert_eq!(
            rpc_error(-libc::EACCES, "msg").kind(),
            ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn rpc_error_out_of_range_falls_back_to_einval() {
        let err = rpc_error(i64::MIN, "msg");
        assert_eq!(err.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn retry_rpc_succeeds_after_failures() {
        let mut calls = 0;
        let res = retry_rpc(
            || {
                calls += 1;
                if calls < ESDM_RETRY_COUNT {
                    -libc::EAGAIN
                } else {
                    0
                }
            },
            "msg",
        );
        assert!(res.is_ok());
        assert_eq!(calls, ESDM_RETRY_COUNT);
    }

    #[test]
    fn retry_rpc_gives_up_with_last_error() {
        let mut calls = 0;
        let err = retry_rpc(
            || {
                calls += 1;
                if calls < ESDM_RETRY_COUNT {
                    -libc::EAGAIN
                } else {
                    -libc::ETIMEDOUT
                }
            },
            "msg",
        )
        .unwrap_err();
        assert_eq!(calls, ESDM_RETRY_COUNT);
        assert_eq!(err.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn once_rpc_retries_only_on_eintr() {
        let mut calls = 0;
        let res = once_rpc(
            || {
                calls += 1;
                if calls < 3 { -EINTR } else { 0 }
            },
            "msg",
        );
        assert!(res.is_ok());
        assert_eq!(calls, 3);
    }

    #[test]
    fn once_rpc_does_not_retry_other_errors() {
        let mut calls = 0;
        let err = once_rpc(
            || {
                calls += 1;
                -libc::EAGAIN
            },
            "msg",
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(err.kind(), ErrorKind::WouldBlock);
    }
}
