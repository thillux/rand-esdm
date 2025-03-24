use libc::ETIMEDOUT;
use rand_core::RngCore;
use regex::Regex;
use std::ffi::{c_char, CString};
use std::mem::MaybeUninit;

use std::io::Error;
use std::sync::Mutex;
use std::time::Duration;

use esdm_sys::esdm;
use esdm_sys::esdm_aux;

/*
 * private ESDM RPC client function definitions
 */

// how often to retry RPC calls before returning an error
const ESDM_RETRY_COUNT: u32 = 5;

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

/// Returns if the client connection to ESDM was initialized succesfully
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

/// Call in order to free ressources needed for ESDM client connection
pub fn esdm_rng_fini() {
    let mut guard = LIB_MUTEX_UNPRIV.lock().unwrap();
    assert_ne!(*guard, 0);

    if *guard == 1 {
        unsafe { esdm::esdm_rpcc_fini_unpriv_service() };
    }

    *guard -= 1;
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

/// Call in order to free ressources needed for ESDM client connection (privileged mode)
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
impl RngCore for EsdmRng {
    fn next_u32(&mut self) -> u32 {
        u32::try_from(self.next_u64() & 0xFF_FF_FF_FF).unwrap()
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes: [u8; 8] = [0; 8];
        self.fill_bytes(&mut bytes);

        u64::from_ne_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for _ in 0..ESDM_RETRY_COUNT {
            let ret_size = match self.rng_type {
                EsdmRngType::FullySeeded => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_full(dest.as_mut_ptr(), dest.len())
                },
                EsdmRngType::PredictionResistant => unsafe {
                    esdm::esdm_rpcc_get_random_bytes_pr(dest.as_mut_ptr(), dest.len())
                },
            };
            if ret_size == isize::try_from(dest.len()).unwrap() {
                return;
            } else {
                eprintln!("esdm ret_size = {ret_size}");
            }
        }
        panic!("cannot get random bytes from ESDM!");
    }
}

/*
 * ESDM specific or privileged functions
 */
/// returns true, if write of data was a success
pub fn esdm_write_data(data: &[u8]) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe { esdm::esdm_rpcc_write_data(data.as_ptr(), data.len()) };
        if ret == 0 {
            return Ok(());
        }
    }

    Err(Error::other("ESDM error write"))
}

pub fn esdm_get_entropy_count() -> Result<u32, Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ent_cnt: u32 = 0;
        let ret =
            unsafe { esdm::esdm_rpcc_rnd_get_ent_cnt(std::ptr::addr_of!(ent_cnt).cast_mut()) };
        if ret == 0 {
            return Ok(ent_cnt);
        }
    }
    Err(Error::other("ESDM error get entropy"))
}

pub fn esdm_add_entropy(entropy_bytes: &[u8], entropy_count: u32) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm::esdm_rpcc_rnd_add_entropy(
                entropy_bytes.as_ptr(),
                entropy_bytes.len(),
                entropy_count,
            )
        };
        if ret == 0 {
            return Ok(());
        }
    }

    Err(Error::other("ESDM error add entropy"))
}

pub fn esdm_add_to_entropy_count(entropy_increment: u32) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe { esdm::esdm_rpcc_rnd_add_to_ent_cnt(entropy_increment) };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::other("ESDM error add entropy count"))
}

pub fn esdm_reseed_crng() -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe { esdm::esdm_rpcc_rnd_reseed_crng() };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::other("ESDM error reseed crng"))
}

pub fn esdm_clear_pool() -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe { esdm::esdm_rpcc_rnd_clear_pool() };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::other("ESDM error clear pool"))
}

pub fn esdm_status_str() -> Result<String, Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let mut status_bytes = vec![0; 8192];
        let ret = unsafe {
            esdm::esdm_rpcc_status(
                status_bytes.as_mut_ptr().cast::<c_char>(),
                status_bytes.len(),
            )
        };
        if ret == 0 {
            for i in 0..status_bytes.len() {
                if status_bytes[i] == 0u8 {
                    status_bytes.resize(i + 1, 0);
                    break;
                }
            }
            let str = CString::from_vec_with_nul(status_bytes).unwrap();
            return Ok(str.into_string().unwrap());
        }
    }
    Err(Error::other("ESDM error clear pool"))
}

pub fn esdm_is_fully_seeded() -> Option<bool> {
    if !esdm_rng_init() {
        return None;
    }

    if let Ok(status) = esdm_status_str() {
        if status.contains("ESDM fully seeded: true") {
            esdm_rng_fini();
            return Some(true);
        }
        if status.contains("ESDM fully seeded: false") {
            esdm_rng_fini();
            return Some(false);
        }
    }

    esdm_rng_fini();

    None
}

pub fn esdm_get_entropy_level() -> Option<u32> {
    if !esdm_rng_init() {
        return None;
    }

    if let Ok(status) = esdm_status_str() {
        let entropy_level_regex = Regex::new(r"^ESDM entropy level: (?<level>\d+)$").unwrap();
        for line in status.split("\n") {
            if let Some(caps) = entropy_level_regex.captures(line) {
                let level_str = caps.get(1).unwrap().as_str();
                let level = level_str.parse::<u32>().unwrap();
                esdm_rng_fini();
                return Some(level);
            }
        }
    }

    esdm_rng_fini();

    None
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
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } != 0 {
            return Err(Error::other("get entropy clock failed"));
        }

        let mut ts_esdm = esdm_aux::timespec {
            tv_sec: ts.tv_sec,
            tv_nsec: ts.tv_nsec,
        };

        ts_esdm.tv_sec += i64::try_from(dur.as_secs()).unwrap();
        ts_esdm.tv_nsec += i64::from(dur.subsec_nanos());
        ts_esdm.tv_sec += ts.tv_nsec / 1_000_000_000;
        ts_esdm.tv_nsec %= 1_000_000_000;
        let ret = unsafe {
            esdm_aux::esdm_aux_timedwait_for_need_entropy(std::ptr::addr_of_mut!(ts_esdm))
        };
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
    use rand::Rng;

    #[test]
    fn test_prediction_resistant_mode() {
        let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);

        for _ in 1..1000 {
            let random_num: u64 = rng.gen();
            println!("Random Number: {random_num:?}");
        }
    }

    #[test]
    fn test_reuse() {
        for _ in 0..1000 {
            let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
            let _ = rng.next_u64();
        }
    }

    #[test]
    fn test_multithreading() {
        let mut threads = vec![];
        let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
        let _ = rng.next_u64();

        println!("Got bytes!");

        for _ in 0..10 {
            threads.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
                    let _ = rng.next_u64();
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
            let random_num: u64 = rng.gen();
            println!("Random Number: {random_num:?}");
        }
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
        esdm_reseed_crng().unwrap();

        let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);

        // don't do this in production: circular seeding
        let buf: [u8; 32] = rng.gen();
        esdm_clear_pool().unwrap();
        esdm_add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap()).unwrap();
        assert!(esdm_get_entropy_count().unwrap() >= 32 * 8);
    }
}
