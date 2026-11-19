//! human readable and JSON status reports of the ESDM

use libc::EMSGSIZE;
use std::ffi::{c_char, c_int};
use std::io::{Error, ErrorKind};

use esdm_sys::esdm;

use crate::connection::EsdmConnection;
use crate::rpc::{rpc_error, should_retry};
use crate::status_json::{EsdmDrngStatus, EsdmStatus};

// largest status document the ESDM RPC protocol is able to transport
const ESDM_STATUS_MAX_LEN: usize = 65536;

/// fetches a NUL-terminated status string via the given RPC call
///
/// `rpc_status` is called with a buffer writable for exactly the given length.
///
/// The buffer grows up to [`ESDM_STATUS_MAX_LEN`] if the server reports
/// `-EMSGSIZE` (ESDM >= 1.2.4 no longer truncates status reports silently).
/// Transient failures are retried, see [`crate::rpc::is_transient`].
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
            // -EMSGSIZE at the maximum size is not transient either
            ret => {
                if !should_retry(ret.into(), failures) {
                    return Err(rpc_error(ret, err_msg));
                }
                failures += 1;
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

/// parses a JSON status document into its typed model
fn parse_status_json<T: serde::de::DeserializeOwned>(doc: &str) -> Result<T, Error> {
    serde_json::from_str(doc).map_err(|e| Error::new(ErrorKind::InvalidData, e))
}

impl EsdmConnection {
    /// status of the ESDM
    ///
    /// Typed form of [`Self::status_json`].
    ///
    /// ```
    /// use rand_esdm::EsdmConnection;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let conn = EsdmConnection::try_new()?;
    /// for es in conn.status()?.entropy_sources {
    ///     println!("{}: {:?} bits available", es.name, es.available_entropy);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn status(&self) -> Result<EsdmStatus, Error> {
        parse_status_json(&self.status_json()?)
    }

    // The ESDM has no calls of their own for the values below: each accessor
    // fetches and parses the whole status document.

    /// whether the ESDM is fully seeded, i.e. serves
    /// [`crate::EsdmRngType::FullySeeded`]
    ///
    /// Like the other status accessors it fetches the whole status, use
    /// [`Self::status`] to read several values at once. Unlike
    /// [`crate::esdm_is_fully_seeded`] it reports why it failed.
    ///
    /// ```
    /// use rand_esdm::EsdmConnection;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let conn = EsdmConnection::try_new()?;
    /// if !conn.is_fully_seeded()? {
    ///     println!("ESDM is still collecting entropy");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn is_fully_seeded(&self) -> Result<bool, Error> {
        Ok(self.status()?.fully_seeded)
    }

    /// entropy in bits currently available from all entropy sources together
    ///
    /// Unlike [`Self::entropy_count`], which covers the auxiliary pool only.
    pub fn entropy_level(&self) -> Result<u32, Error> {
        Ok(self.status()?.entropy_level_bits)
    }

    /// security strength of the DRNGs in bits, the entropy the ESDM needs to
    /// be fully seeded
    pub fn security_strength(&self) -> Result<u32, Error> {
        Ok(self.status()?.security_strength_bits)
    }

    /// version of the ESDM server, e.g. `"1.2.4"`
    pub fn server_version(&self) -> Result<String, Error> {
        Ok(self.status()?.library_version)
    }

    /// status of the ESDM as JSON document
    ///
    /// [`Self::status`] parses it into [`EsdmStatus`].
    pub fn status_json(&self) -> Result<String, Error> {
        fetch_status_str(
            // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
            // and `rpc.rs`.
            |buf, len| unsafe { esdm::esdm_rpcc_status_json(buf, len) },
            "ESDM error status json",
        )
    }

    /// human readable status of the ESDM
    ///
    /// Since ESDM 1.2.4 this text is rendered from the JSON status document
    /// and its format is not meant to be parsed, use [`Self::status`]
    /// instead.
    pub fn status_text(&self) -> Result<String, Error> {
        fetch_status_str(
            // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
            // and `rpc.rs`.
            |buf, len| unsafe { esdm::esdm_rpcc_status(buf, len) },
            "ESDM error status",
        )
    }

    /// status of the DRNG instance serving the given node
    ///
    /// Typed form of [`Self::drng_status_json`].
    pub fn drng_status(&self, node: u32) -> Result<EsdmDrngStatus, Error> {
        parse_status_json(&self.drng_status_json(node)?)
    }

    /// status of the DRNG instance serving the given node as JSON document
    ///
    /// Fails with `ENODEV` if there is no such node.
    pub fn drng_status_json(&self, node: u32) -> Result<String, Error> {
        fetch_status_str(
            // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
            // and `rpc.rs`.
            |buf, len| unsafe { esdm::esdm_rpcc_drng_status_json(node, buf, len) },
            "ESDM error DRNG status json",
        )
    }

    /// status of the prediction resistant DRNG instance
    ///
    /// Typed form of [`Self::drng_status_pr_json`].
    pub fn drng_status_pr(&self) -> Result<EsdmDrngStatus, Error> {
        parse_status_json(&self.drng_status_pr_json()?)
    }

    /// status of the prediction resistant DRNG instance as JSON document
    pub fn drng_status_pr_json(&self) -> Result<String, Error> {
        fetch_status_str(
            // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
            // and `rpc.rs`.
            |buf, len| unsafe { esdm::esdm_rpcc_drng_status_pr_json(buf, len) },
            "ESDM error PR DRNG status json",
        )
    }

    /// human readable status of the Jitter RNG entropy source
    ///
    /// Empty if the server runs without it.
    pub fn jent_status_text(&self) -> Result<String, Error> {
        fetch_status_str(
            // SAFETY: `buf` is writable for `len` bytes, see `fetch_status_str`
            // and `rpc.rs`.
            |buf, len| unsafe { esdm::esdm_rpcc_jent_status(buf, len) },
            "ESDM error jent status",
        )
    }
}

/// whether the ESDM is fully seeded, sets up a connection of its own
///
/// Returns `None` on any error: if no connection can be set up or the status
/// cannot be fetched or parsed. Use [`EsdmConnection::is_fully_seeded`] to
/// learn the reason.
#[must_use]
pub fn esdm_is_fully_seeded() -> Option<bool> {
    EsdmConnection::try_new().ok()?.is_fully_seeded().ok()
}

/// entropy level of the ESDM in bits, sets up a connection of its own
///
/// Returns `None` on any error, [`EsdmConnection::entropy_level`] reports the
/// reason.
#[must_use]
pub fn esdm_get_entropy_level() -> Option<u32> {
    EsdmConnection::try_new().ok()?.entropy_level().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::ESDM_RETRY_COUNT;

    /// copies `content` to the start of the buffer handed out by `fetch_status_str`
    fn write_buf(buf: *mut c_char, len: usize, content: &[u8]) {
        assert!(content.len() <= len);
        // SAFETY: `fetch_status_str` hands out a buffer writable for `len`
        // bytes and `content` fits into it, as asserted above.
        unsafe { std::ptr::copy_nonoverlapping(content.as_ptr(), buf.cast::<u8>(), content.len()) };
    }

    #[test]
    fn returns_string_up_to_nul() {
        let status = fetch_status_str(
            |buf, len| {
                write_buf(buf, len, b"hello\0garbage");
                0
            },
            "msg",
        )
        .unwrap();
        assert_eq!(status, "hello");
    }

    #[test]
    fn grows_buffer_on_emsgsize() {
        let mut lens = vec![];
        let status = fetch_status_str(
            |buf, len| {
                lens.push(len);
                if len < ESDM_STATUS_MAX_LEN {
                    return -EMSGSIZE;
                }
                let big = vec![b'x'; 10_000];
                write_buf(buf, len, &big);
                0
            },
            "msg",
        )
        .unwrap();
        assert_eq!(lens, [8192, ESDM_STATUS_MAX_LEN]);
        assert_eq!(status.len(), 10_000);
    }

    #[test]
    fn emsgsize_at_max_len_is_an_error() {
        let mut calls = 0;
        let err = fetch_status_str(
            |_, _| {
                calls += 1;
                -EMSGSIZE
            },
            "msg",
        )
        .unwrap_err();
        // one call to grow, then no retries at the maximum size
        assert_eq!(calls, 2);
        assert!(err.to_string().starts_with("msg: "));
    }

    #[test]
    fn does_not_retry_permanent_errors() {
        let mut calls = 0;
        let err = fetch_status_str(
            |_, _| {
                calls += 1;
                -libc::EACCES
            },
            "msg",
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(err.kind(), ErrorKind::PermissionDenied);
    }

    #[test]
    fn retries_transient_errors() {
        let mut calls = 0;
        let status = fetch_status_str(
            |buf, len| {
                calls += 1;
                if calls < ESDM_RETRY_COUNT {
                    return -libc::EAGAIN;
                }
                write_buf(buf, len, b"ok\0");
                0
            },
            "msg",
        )
        .unwrap();
        assert_eq!(status, "ok");
        assert_eq!(calls, ESDM_RETRY_COUNT);
    }

    #[test]
    fn rejects_missing_nul() {
        let err = fetch_status_str(
            |buf, len| {
                write_buf(buf, len, &vec![b'x'; len]);
                0
            },
            "msg",
        )
        .unwrap_err();
        assert!(err.to_string().contains("NUL"));
    }

    #[test]
    fn rejects_invalid_utf8() {
        let err = fetch_status_str(
            |buf, len| {
                write_buf(buf, len, b"\xff\xfe\0");
                0
            },
            "msg",
        )
        .unwrap_err();
        assert!(err.to_string().contains("UTF-8"));
    }
}
