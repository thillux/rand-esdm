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

use libc::{EAGAIN, ECONNRESET, EFAULT, EINTR, ETIMEDOUT};
use std::ffi::c_int;
use std::io::{Error, ErrorKind};
use std::thread;
use std::time::Duration;

// how often to try RPC calls before returning an error
pub(crate) const ESDM_RETRY_COUNT: u32 = 5;

/// whether a negative errno-style ESDM return value reports a failure which
/// may go away when the call is issued again
///
/// These are the interrupted, timed out or reset connections and the
/// temporary refusals (`-EAGAIN`, e.g. while the ESDM is not fully seeded).
/// Everything else, like missing privileges or an uninitialized service
/// (`-EFAULT`), fails again the same way.
pub(crate) fn is_transient(ret: i64) -> bool {
    ret.checked_neg()
        .and_then(|errno| i32::try_from(errno).ok())
        .is_some_and(|errno| matches!(errno, EINTR | EAGAIN | ETIMEDOUT | ECONNRESET))
}

/// decides whether to issue a call again after its `attempt`-th failure
/// (counting from 0) returned `ret`
///
/// Returns `false` if the retry budget is used up or the failure is not
/// transient. Otherwise it backs off briefly before returning `true`, unless
/// the call was merely interrupted.
pub(crate) fn should_retry(ret: i64, attempt: u32) -> bool {
    if attempt + 1 >= ESDM_RETRY_COUNT || !is_transient(ret) {
        return false;
    }

    if ret != -i64::from(EINTR) {
        thread::sleep(Duration::from_millis(1 << attempt.min(6)));
    }

    true
}

/// converts a negative errno-style ESDM return value into an [`Error`]
///
/// `-EFAULT` is what the client library reports for a call without an
/// initialized service. The connection guards rule that out, but it becomes
/// [`ErrorKind::NotConnected`] rather than the misleading "Bad address" in
/// any case.
pub(crate) fn rpc_error(ret: impl Into<i64>, err_msg: &'static str) -> Error {
    let errno = ret
        .into()
        .checked_neg()
        .and_then(|errno| i32::try_from(errno).ok())
        .unwrap_or(libc::EINVAL);
    if errno == EFAULT {
        return Error::new(
            ErrorKind::NotConnected,
            format!("{err_msg}: ESDM client service not initialized"),
        );
    }
    let os_err = Error::from_raw_os_error(errno);
    Error::new(os_err.kind(), format!("{err_msg}: {os_err}"))
}

/// issues an idempotent ESDM RPC call up to [`ESDM_RETRY_COUNT`] times until
/// it reports success or a failure which is not transient, see
/// [`is_transient`]
///
/// Only use this for calls which can safely be issued multiple times: since
/// ESDM 1.2.4, a request without an answer may still have been processed by
/// the server.
pub(crate) fn retry_rpc(
    mut rpc: impl FnMut() -> c_int,
    err_msg: &'static str,
) -> Result<(), Error> {
    let mut attempt = 0;
    loop {
        let ret = rpc();
        if ret == 0 {
            return Ok(());
        }
        if !should_retry(ret.into(), attempt) {
            return Err(rpc_error(ret, err_msg));
        }
        attempt += 1;
    }
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

    #[test]
    fn rpc_error_maps_errno() {
        let err = rpc_error(-ETIMEDOUT, "msg");
        assert_eq!(err.kind(), ErrorKind::TimedOut);
        assert!(err.to_string().starts_with("msg: "));

        assert_eq!(
            rpc_error(-libc::EACCES, "msg").kind(),
            ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn rpc_error_explains_missing_connection() {
        let err = rpc_error(-EFAULT, "msg");
        assert_eq!(err.kind(), ErrorKind::NotConnected);
        assert!(err.to_string().contains("not initialized"), "{err}");
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
                if calls < ESDM_RETRY_COUNT { -EAGAIN } else { 0 }
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
                    -EAGAIN
                } else {
                    -ETIMEDOUT
                }
            },
            "msg",
        )
        .unwrap_err();
        assert_eq!(calls, ESDM_RETRY_COUNT);
        assert_eq!(err.kind(), ErrorKind::TimedOut);
    }

    #[test]
    fn retry_rpc_does_not_retry_permanent_errors() {
        for errno in [libc::EACCES, EFAULT, libc::EOPNOTSUPP] {
            let mut calls = 0;
            let err = retry_rpc(
                || {
                    calls += 1;
                    -errno
                },
                "msg",
            )
            .unwrap_err();
            assert_eq!(calls, 1);
            assert!(err.to_string().starts_with("msg: "));
        }
    }

    #[test]
    fn transient_errors() {
        for errno in [EINTR, EAGAIN, ETIMEDOUT, ECONNRESET] {
            assert!(is_transient(-i64::from(errno)));
        }
        for ret in [0, 1, -i64::from(libc::EACCES), -i64::from(EFAULT), i64::MIN] {
            assert!(!is_transient(ret));
        }
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
                -EAGAIN
            },
            "msg",
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(err.kind(), ErrorKind::WouldBlock);
    }
}
