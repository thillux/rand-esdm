//! human readable and JSON status reports of the ESDM

use libc::EMSGSIZE;
use std::ffi::{c_char, c_int};
use std::io::Error;

use esdm_sys::esdm;

use crate::connection::{esdm_rng_fini, esdm_rng_init};
use crate::rpc::{ESDM_RETRY_COUNT, rpc_error};

// largest status document the ESDM RPC protocol is able to transport
const ESDM_STATUS_MAX_LEN: usize = 65536;

/// fetches a NUL-terminated status string via the given RPC call
///
/// `rpc_status` is called with a buffer writable for exactly the given length.
///
/// The buffer grows up to [`ESDM_STATUS_MAX_LEN`] if the server reports
/// `-EMSGSIZE` (ESDM >= 1.2.4 no longer truncates status reports silently).
fn fetch_status_str(
    mut rpc_status: impl FnMut(*mut c_char, usize) -> c_int,
    err_msg: &'static str,
) -> Result<String, Error> {
    let mut status_bytes = vec![0u8; 8192];
    let mut failures = 0;

    loop {
        status_bytes.fill(0);
        let ret = rpc_status(
            status_bytes.as_mut_ptr().cast::<c_char>(),
            status_bytes.len(),
        );
        match ret {
            0 => break,
            ret if ret == -EMSGSIZE && status_bytes.len() < ESDM_STATUS_MAX_LEN => {
                status_bytes.resize(ESDM_STATUS_MAX_LEN, 0);
            }
            ret => {
                failures += 1;
                if failures == ESDM_RETRY_COUNT {
                    return Err(rpc_error(ret, err_msg));
                }
            }
        }
    }

    let nul_pos = status_bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::other("ESDM status string is not NUL-terminated"))?;
    status_bytes.truncate(nul_pos);
    String::from_utf8(status_bytes).map_err(|_| Error::other("ESDM status string is not UTF-8"))
}

pub fn esdm_jent_status_str() -> Result<String, Error> {
    fetch_status_str(
        // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
        // and `rpc.rs`.
        |buf, len| unsafe { esdm::esdm_rpcc_jent_status(buf, len) },
        "ESDM error jent status",
    )
}

/// human readable status of the ESDM
///
/// Since ESDM 1.2.4 this text is rendered from the JSON status document and
/// its format is not meant to be parsed, use [`esdm_status_json_str`] instead.
pub fn esdm_status_str() -> Result<String, Error> {
    fetch_status_str(
        // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
        // and `rpc.rs`.
        |buf, len| unsafe { esdm::esdm_rpcc_status(buf, len) },
        "ESDM error status",
    )
}

/// status of the ESDM as JSON document (ESDM >= 1.2.4)
pub fn esdm_status_json_str() -> Result<String, Error> {
    fetch_status_str(
        // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
        // and `rpc.rs`.
        |buf, len| unsafe { esdm::esdm_rpcc_status_json(buf, len) },
        "ESDM error status json",
    )
}

/// status of the DRNG instance serving the given node as JSON document
/// (ESDM >= 1.2.4)
pub fn esdm_drng_status_json_str(node: u32) -> Result<String, Error> {
    fetch_status_str(
        // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
        // and `rpc.rs`.
        |buf, len| unsafe { esdm::esdm_rpcc_drng_status_json(node, buf, len) },
        "ESDM error DRNG status json",
    )
}

/// status of the prediction resistant DRNG instance as JSON document
/// (ESDM >= 1.2.4)
pub fn esdm_drng_status_pr_json_str() -> Result<String, Error> {
    fetch_status_str(
        // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
        // and `rpc.rs`.
        |buf, len| unsafe { esdm::esdm_rpcc_drng_status_pr_json(buf, len) },
        "ESDM error PR DRNG status json",
    )
}

/// initializes an ESDM connection, extracts a value from the JSON status
/// document and releases the connection again
fn with_status_json<T>(parse: impl FnOnce(&serde_json::Value) -> Option<T>) -> Option<T> {
    if !esdm_rng_init() {
        return None;
    }

    let result = esdm_status_json_str()
        .ok()
        .and_then(|status| serde_json::from_str::<serde_json::Value>(&status).ok())
        .and_then(|status| parse(&status));

    esdm_rng_fini();

    result
}

#[must_use]
pub fn esdm_is_fully_seeded() -> Option<bool> {
    with_status_json(|status| status.get("fully_seeded")?.as_bool())
}

#[must_use]
pub fn esdm_get_entropy_level() -> Option<u32> {
    with_status_json(|status| u32::try_from(status.get("entropy_level_bits")?.as_u64()?).ok())
}
