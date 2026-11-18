use libc::{EAGAIN, EINTR, EMSGSIZE, ETIMEDOUT};
use rand_core::TryRng;
use std::ffi::{CString, c_char};
use std::mem::MaybeUninit;
use std::os::raw::c_int;

use std::io::Error;
use std::sync::Mutex;
use std::time::Duration;

use esdm_sys::esdm::{self, esdm_rpcc_set_max_online_nodes};
use esdm_sys::esdm_aux;

/*
 * private ESDM RPC client function definitions
 */

// how often to retry RPC calls before returning an error
const ESDM_RETRY_COUNT: u32 = 5;

// largest status document the ESDM RPC protocol is able to transport
const ESDM_STATUS_MAX_LEN: usize = 65536;

/// converts a negative errno-style ESDM return value into an [`Error`]
fn rpc_error(ret: impl Into<i64>, err_msg: &'static str) -> Error {
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
fn retry_rpc(mut rpc: impl FnMut() -> c_int, err_msg: &'static str) -> Result<(), Error> {
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
fn once_rpc(mut rpc: impl FnMut() -> c_int, err_msg: &'static str) -> Result<(), Error> {
    loop {
        match rpc() {
            0 => return Ok(()),
            ret if ret == -EINTR => {}
            ret => return Err(rpc_error(ret, err_msg)),
        }
    }
}

static LIB_MUTEX_UNPRIV: Mutex<u32> = Mutex::new(0u32);
static LIB_MUTEX_PRIV: Mutex<u32> = Mutex::new(0u32);

pub enum EsdmRngType {
    /// ESDM RNG implementation, which uses fresh entropy for every random output produced
    PredictionResistant,

    /// ESDM RNG implementation, which only produces random numbers when fully seeded
    /// otherwise it times out and returns an error after a few internal tries
    FullySeeded,
}

pub struct EsdmRng {
    rng_type: EsdmRngType,
}

/// Returns if the client connection to ESDM was initialized successfully
/// Only needed to call once globally before first usage of ESDM
#[must_use]
pub fn esdm_rng_init() -> bool {
    let mut guard = LIB_MUTEX_UNPRIV.lock().unwrap();

    let ret = if *guard == 0 {
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
    let mut guard = LIB_MUTEX_UNPRIV.lock().unwrap();
    assert_ne!(*guard, 0);

    if *guard == 1 {
        unsafe { esdm::esdm_rpcc_fini_unpriv_service() };
    }

    *guard -= 1;
}

/// Limits the number of parallel RPC connections the client library uses.
///
/// Since ESDM 1.2.4 the limit can only be lowered and has to be set before the
/// RPC connection is initialized (e.g. by [`esdm_rng_init`] or
/// [`EsdmRng::new`]), as connections of an initialized service are not resized.
pub fn esdm_set_max_online_nodes(nodes: u32) -> Result<(), Error> {
    let ret = unsafe { esdm_rpcc_set_max_online_nodes(nodes) };
    if ret < 0 {
        return Err(rpc_error(ret, "ESDM error set max online nodes"));
    }

    Ok(())
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM (privileged mode)
#[must_use]
pub fn esdm_rng_init_priv() -> bool {
    let mut guard = LIB_MUTEX_PRIV.lock().unwrap();

    let ret = if *guard == 0 {
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
    let mut guard = LIB_MUTEX_PRIV.lock().unwrap();
    assert_ne!(*guard, 0);

    if *guard == 1 {
        unsafe { esdm::esdm_rpcc_fini_priv_service() };
    }

    *guard -= 1;
}

impl EsdmRng {
    #[must_use]
    pub fn new(rng_type: EsdmRngType) -> Self {
        esdm_rng_init_checked();
        EsdmRng { rng_type }
    }
}

impl Drop for EsdmRng {
    fn drop(&mut self) {
        esdm_rng_fini();
    }
}

/*
 * rand_core trait implementations
 */
impl TryRng for EsdmRng {
    type Error = std::io::Error;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(u32::try_from(self.try_next_u64()? & 0xFF_FF_FF_FF).unwrap())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut bytes: [u8; 8] = [0; 8];
        self.try_fill_bytes(&mut bytes)?;

        Ok(u64::from_ne_bytes(bytes))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        let mut filled = 0;
        let mut failures = 0;
        let mut last_ret: isize = 0;

        while filled < dst.len() {
            if failures == ESDM_RETRY_COUNT {
                return Err(rpc_error(
                    i64::try_from(last_ret).unwrap_or(-i64::from(libc::EIO)),
                    "Unable to fetch random bytes from ESDM",
                ));
            }

            let remaining = &mut dst[filled..];
            let ret = match self.rng_type {
                EsdmRngType::FullySeeded => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_full(remaining.as_mut_ptr(), remaining.len())
                },
                EsdmRngType::PredictionResistant => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_pr(remaining.as_mut_ptr(), remaining.len())
                },
            };

            match usize::try_from(ret) {
                // short reads are possible, continue with the remainder
                Ok(n) if n > 0 => filled += n.min(remaining.len()),
                _ => {
                    last_ret = if ret == 0 {
                        -isize::try_from(EAGAIN).unwrap()
                    } else {
                        ret
                    };
                    failures += 1;
                }
            }
        }

        Ok(())
    }
}

/*
 * ESDM specific or privileged functions
 */
/// writes data into the ESDM auxiliary pool without crediting entropy for it
pub fn esdm_write_data(data: &[u8]) -> Result<(), Error> {
    once_rpc(
        || unsafe { esdm::esdm_rpcc_write_data(data.as_ptr(), data.len()) },
        "ESDM error write",
    )
}

pub fn esdm_crng_reseed() -> Result<(), Error> {
    once_rpc(
        || unsafe { esdm::esdm_rpcc_rnd_reseed_crng() },
        "ESDM error reseed crng",
    )
}

#[deprecated(since = "0.4.0", note = "use `esdm_crng_reseed` instead")]
pub fn esdm_reseed_crng() -> Result<(), Error> {
    esdm_crng_reseed()
}

pub fn esdm_get_entropy_count() -> Result<u32, Error> {
    let mut ent_cnt: u32 = 0;
    retry_rpc(
        || unsafe { esdm::esdm_rpcc_rnd_get_ent_cnt(&raw mut ent_cnt) },
        "ESDM error get entropy",
    )?;

    Ok(ent_cnt)
}

pub fn esdm_add_entropy(entropy_bytes: &[u8], entropy_count: u32) -> Result<(), Error> {
    once_rpc(
        || unsafe {
            esdm::esdm_rpcc_rnd_add_entropy(
                entropy_bytes.as_ptr(),
                entropy_bytes.len(),
                entropy_count,
            )
        },
        "ESDM error add entropy",
    )
}

pub fn esdm_add_to_entropy_count(entropy_increment: u32) -> Result<(), Error> {
    once_rpc(
        || unsafe { esdm::esdm_rpcc_rnd_add_to_ent_cnt(entropy_increment) },
        "ESDM error add entropy count",
    )
}

pub fn esdm_clear_pool() -> Result<(), Error> {
    once_rpc(
        || unsafe { esdm::esdm_rpcc_rnd_clear_pool() },
        "ESDM error clear pool",
    )
}

pub fn esdm_write_wakeup_thresh() -> Result<u32, Error> {
    let mut write_wakeup_thresh: u32 = 0;
    retry_rpc(
        || unsafe { esdm::esdm_rpcc_get_write_wakeup_thresh(&raw mut write_wakeup_thresh) },
        "ESDM error write wakeup thresh",
    )?;

    Ok(write_wakeup_thresh)
}

/// fetches a NUL-terminated status string via the given RPC call
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
        |buf, len| unsafe { esdm::esdm_rpcc_status(buf, len) },
        "ESDM error status",
    )
}

/// status of the ESDM as JSON document (ESDM >= 1.2.4)
pub fn esdm_status_json_str() -> Result<String, Error> {
    fetch_status_str(
        |buf, len| unsafe { esdm::esdm_rpcc_status_json(buf, len) },
        "ESDM error status json",
    )
}

/// status of the DRNG instance serving the given node as JSON document
/// (ESDM >= 1.2.4)
pub fn esdm_drng_status_json_str(node: u32) -> Result<String, Error> {
    fetch_status_str(
        |buf, len| unsafe { esdm::esdm_rpcc_drng_status_json(node, buf, len) },
        "ESDM error DRNG status json",
    )
}

/// status of the prediction resistant DRNG instance as JSON document
/// (ESDM >= 1.2.4)
pub fn esdm_drng_status_pr_json_str() -> Result<String, Error> {
    fetch_status_str(
        |buf, len| unsafe { esdm::esdm_rpcc_drng_status_pr_json(buf, len) },
        "ESDM error PR DRNG status json",
    )
}

/// state of a group of ESDM self tests
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EsdmSelftestState {
    Undone,
    Passed,
    Failed,
}

impl From<esdm::esdm_rpcc_selftest_state> for EsdmSelftestState {
    fn from(state: esdm::esdm_rpcc_selftest_state) -> Self {
        match state {
            esdm::esdm_rpcc_selftest_state::esdm_rpcc_selftest_undone => Self::Undone,
            esdm::esdm_rpcc_selftest_state::esdm_rpcc_selftest_passed => Self::Passed,
            esdm::esdm_rpcc_selftest_state::esdm_rpcc_selftest_failed => Self::Failed,
        }
    }
}

/// outcome of an ESDM self test run, see [`esdm_selftest`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EsdmSelftestResult {
    /// state of the self tests of the hash and the DRNG
    pub crypto_state: EsdmSelftestState,
    /// state of the self tests of the entropy sources
    pub es_state: EsdmSelftestState,
    /// number of entropy sources tested
    pub es_sources: u32,
    /// number of entropy sources which failed their self test
    pub es_failures: u32,
}

/// runs the ESDM self tests now (privileged interface, ESDM >= 1.2.4)
///
/// Returns the outcome and whether every self test passed (`Ok(true)`), or
/// an error if the server could not be asked.
pub fn esdm_selftest() -> Result<(EsdmSelftestResult, bool), Error> {
    let mut result = esdm::esdm_rpcc_selftest_result {
        crypto_state: esdm::esdm_rpcc_selftest_state::esdm_rpcc_selftest_undone,
        es_state: esdm::esdm_rpcc_selftest_state::esdm_rpcc_selftest_undone,
        es_sources: 0,
        es_failures: 0,
    };

    let ret = loop {
        let ret = unsafe { esdm::esdm_rpcc_selftest(&raw mut result) };
        if ret != -EINTR {
            break ret;
        }
    };

    let result = EsdmSelftestResult {
        crypto_state: result.crypto_state.into(),
        es_state: result.es_state.into(),
        es_sources: result.es_sources,
        es_failures: result.es_failures,
    };

    if ret == 0 {
        Ok((result, true))
    } else if result.crypto_state == EsdmSelftestState::Undone
        && result.es_state == EsdmSelftestState::Undone
    {
        Err(rpc_error(ret, "ESDM error selftest"))
    } else {
        Ok((result, false))
    }
}

/// sets the entropy rate of an entropy source (privileged interface, only
/// supported by ESDM servers built in test mode or as debug build, ESDM >= 1.2.4)
pub fn esdm_set_es_rate(es: &str, rate: u32) -> Result<(), Error> {
    let es = CString::new(es).map_err(|_| Error::other("entropy source name contains NUL"))?;
    once_rpc(
        || unsafe { esdm::esdm_rpcc_set_es_rate(es.as_ptr(), rate) },
        "ESDM error set entropy source rate",
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

pub struct EsdmNotification {}

impl Default for EsdmNotification {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EsdmNotification {
    fn drop(&mut self) {
        unsafe { esdm_aux::esdm_aux_fini_wait_for_need_entropy() };
    }
}

impl EsdmNotification {
    #[must_use]
    pub fn new() -> Self {
        let ret = unsafe { esdm_aux::esdm_aux_init_wait_for_need_entropy() };
        assert!(ret == 0, "unable to initialize ESDM aux library");
        EsdmNotification {}
    }

    pub fn wait_for_entropy_needed_timeout(&mut self, dur: Duration) -> Result<u32, Error> {
        let mut ts: libc::timespec = unsafe { MaybeUninit::zeroed().assume_init() };
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut ts) } != 0 {
            return Err(Error::other("get entropy clock failed"));
        }

        let mut ts_esdm = esdm_aux::timespec {
            tv_sec: ts.tv_sec,
            tv_nsec: ts.tv_nsec,
        };

        ts_esdm.tv_sec += i64::try_from(dur.as_secs()).unwrap();
        ts_esdm.tv_nsec += i64::from(dur.subsec_nanos());
        ts_esdm.tv_sec += ts_esdm.tv_nsec / 1_000_000_000;
        ts_esdm.tv_nsec %= 1_000_000_000;
        let ret = unsafe { esdm_aux::esdm_aux_timedwait_for_need_entropy(&raw mut ts_esdm) };
        if ret == ETIMEDOUT {
            return Err(Error::other("get entropy timed out"));
        }

        match esdm_get_entropy_count() {
            Ok(cnt) => Ok(cnt),
            _ => Err(Error::other("ESDM error get entropy count")),
        }
    }
}

// these tests assume a running esdm-server on the system!
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prediction_resistant_mode() {
        let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);

        for _ in 1..1000 {
            let random_num: u64 = rng.try_next_u64().unwrap();
            println!("Random Number: {random_num:?}");
        }
    }

    #[test]
    fn test_write_wakeup_thresh() {
        esdm_rng_init_checked();

        let write_wakup_thresh = esdm_write_wakeup_thresh().unwrap();
        assert_ne!(write_wakup_thresh, 0);

        println!("write wakeup thresh: {write_wakup_thresh}");

        esdm_rng_fini();
    }

    #[test]
    fn test_reuse() {
        for _ in 0..1000 {
            let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
            let _ = rng.try_next_u64().unwrap();
        }
    }

    #[test]
    fn test_multithreading() {
        let mut threads = vec![];
        let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
        let _ = rng.try_next_u64().unwrap();

        println!("Got bytes!");

        for _ in 0..10 {
            threads.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
                    let _ = rng.try_next_u64().unwrap();
                }
            }));
        }

        for t in threads {
            let _ = t.join();
        }
    }

    #[test]
    fn test_fully_seeded_mode() {
        let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

        for _ in 1..1000 {
            let random_num: u64 = rng.try_next_u64().unwrap();
            println!("Random Number: {random_num:?}");
        }
    }

    #[test]
    fn test_status_json() {
        esdm_rng_init_checked();

        let status: serde_json::Value =
            serde_json::from_str(&esdm_status_json_str().unwrap()).unwrap();
        assert!(status.get("fully_seeded").unwrap().is_boolean());
        assert!(status.get("entropy_level_bits").unwrap().is_u64());

        let drng: serde_json::Value =
            serde_json::from_str(&esdm_drng_status_pr_json_str().unwrap()).unwrap();
        assert!(drng.is_object());

        esdm_rng_fini();

        assert!(esdm_is_fully_seeded().is_some());
        assert!(esdm_get_entropy_level().is_some());
    }

    #[test]
    fn test_large_request() {
        let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
        let mut buf = vec![0u8; 1 << 20];
        rng.try_fill_bytes(&mut buf).unwrap();
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_status() {
        esdm_rng_init_checked();

        for _ in 0..100 {
            let status = esdm_status_str().unwrap();
            println!("{status}");
        }

        esdm_rng_fini();
    }

    // need to be root to run this test
    #[test]
    #[cfg(feature = "privileged_tests")]
    fn test_privileged_interface() {
        // also need unprivileged interface for random bytes
        esdm_rng_init_checked();
        esdm_rng_init_priv_checked();

        esdm_clear_pool().unwrap();
        assert_eq!(esdm_get_entropy_count().unwrap(), 0);
        esdm_add_to_entropy_count(64 * 8).unwrap();
        esdm_crng_reseed().unwrap();

        let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

        // don't do this in production: circular seeding
        let mut buf: [u8; 32] = [42; 32];
        rng.try_fill_bytes(&mut buf).unwrap();
        esdm_clear_pool().unwrap();
        esdm_add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap()).unwrap();
        assert!(esdm_get_entropy_count().unwrap() >= 32 * 8);

        let (result, passed) = esdm_selftest().unwrap();
        println!("selftest: {result:?}");
        assert!(passed);
        assert_eq!(result.crypto_state, EsdmSelftestState::Passed);
    }
}
