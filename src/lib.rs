use rand_core::{Error, RngCore};
use std::ffi::{c_uchar, c_void, c_uint, c_char, CString};
use std::ptr::null_mut;

/*
 * private ESDM RPC client function definitions
 */

// how often to retry RPC calls before returning an error
const ESDM_RETRY_COUNT: u32 = 5;

extern "C" {
    /*
     * unprivileged calls
     */
    #[must_use]
    fn esdm_rpcc_init_unpriv_service(handler: *mut c_void) -> i32;
    fn esdm_rpcc_fini_unpriv_service();

    #[must_use]
    fn esdm_rpcc_get_random_bytes_full(buf: *mut c_uchar, buf_len: usize) -> isize;
    #[must_use]
    fn esdm_rpcc_get_random_bytes_pr(buf: *mut c_uchar, buf_len: usize) -> isize;

    // add unaccounted entropy bits to the pool
    #[must_use]
    fn esdm_rpcc_write_data(buf: *const c_uchar, buf_len: usize) -> i32;

    // get entropy count
    #[must_use]
    fn esdm_rpcc_rnd_get_ent_cnt(entcnt: *mut c_uint) -> i32;

    fn esdm_rpcc_status(buf: *mut c_char, buflen: usize) -> i32;
    /*
     * privileged calls
     */
    #[must_use]
    fn esdm_rpcc_init_priv_service(handler: *mut c_void) -> i32;
    fn esdm_rpcc_fini_priv_service();

    #[must_use]
    fn esdm_rpcc_rnd_add_entropy(entropy_buf: *const c_uchar, entropy_buf_len: usize, entropy_cnt: u32) -> i32;

    #[must_use]
    fn esdm_rpcc_rnd_reseed_crng() -> i32;

    #[must_use]
    fn esdm_rpcc_rnd_add_to_ent_cnt(entropy_cnt: u32) -> i32;

    #[must_use]
    fn esdm_rpcc_rnd_clear_pool() -> i32;
}

/// ESDM RNG implementation, which only produces random numbers when fully seeded
/// otherwise it times out and returns an error after a few internal tries
pub struct EsdmRngFullySeeded {}

/// ESDM RNG implementation, which uses fresh entropy for every random output produced
pub struct EsdmRngPredictionResistant {}

/// Returns if the client connection to ESDM was initialized succesfully
/// Only needed to call once globally before first usage of ESDM
#[must_use] pub fn esdm_rng_init() -> bool {
    let ret = unsafe { esdm_rpcc_init_unpriv_service(null_mut()) };
    ret == 0
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM
pub fn esdm_rng_init_checked() {
    let success = esdm_rng_init();
    assert!(success);
}

/// Call in order to free ressources needed for ESDM client connection
pub fn esdm_rng_fini() {
    unsafe { esdm_rpcc_fini_unpriv_service() };
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM (privileged mode)
#[must_use] pub fn esdm_rng_init_priv() -> bool {
    let ret = unsafe { esdm_rpcc_init_priv_service(null_mut()) };
    ret == 0
}

/// initializes the client connection to ESDM, asserts if something goes wrong
/// Only needed to call once globally before first usage of ESDM (privileged mode)
pub fn esdm_rng_init_priv_checked() {
    let success = esdm_rng_init_priv();
    assert!(success);
}

/// Call in order to free ressources needed for ESDM client connection (privileged mode)
pub fn esdm_rng_fini_priv() {
    unsafe { esdm_rpcc_fini_priv_service() };
}

/*
 * rand_core trait implementations
 */
impl RngCore for EsdmRngPredictionResistant {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes: [u8; 8] = [0; 8];
        self.fill_bytes(&mut bytes);
        
        u64::from_ne_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for _ in 0..ESDM_RETRY_COUNT {
            let ret_size = unsafe { esdm_rpcc_get_random_bytes_pr(dest.as_mut_ptr(), dest.len()) };
            if ret_size == dest.len() as isize {
                return;
            }
        }
        panic!("cannot get random bytes from ESDM!");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

impl RngCore for EsdmRngFullySeeded {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes: [u8; 8] = [0; 8];
        self.fill_bytes(&mut bytes);
        
        u64::from_ne_bytes(bytes)
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for _ in 0..ESDM_RETRY_COUNT {
            let ret_size =
                unsafe { esdm_rpcc_get_random_bytes_full(dest.as_mut_ptr(), dest.len()) };
            if ret_size == dest.len() as isize {
                return;
            }
        }
        panic!("cannot get random bytes from ESDM!");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}

/*
 * ESDM specific or privileged functions
 */
/// returns true, if write of data was a success
pub fn esdm_write_data(data: &[u8]) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm_rpcc_write_data(data.as_ptr(), data.len())
        };
        if ret == 0 {
            return Ok(());
        }
    }

    Err(Error::new("ESDM error write"))
}

pub fn esdm_get_entropy_count() -> Result<u32, Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let mut ent_cnt_bytes: [u8; 4] = [0;4];
        let ret = unsafe {
            esdm_rpcc_rnd_get_ent_cnt(ent_cnt_bytes.as_mut_ptr().cast::<u32>())
        };
        if ret == 0 {
            return Ok(u32::from_ne_bytes(ent_cnt_bytes));
        }
    }
    Err(Error::new("ESDM error get entropy"))
}

pub fn esdm_add_entropy(entropy_bytes: &[u8], entropy_count: u32) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm_rpcc_rnd_add_entropy(entropy_bytes.as_ptr(), entropy_bytes.len(), entropy_count)
        };
        if ret == 0 {
            return Ok(());
        }
    }

    Err(Error::new("ESDM error add entropy"))
}

pub fn esdm_add_to_entropy_count(entropy_increment: u32) -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm_rpcc_rnd_add_to_ent_cnt(entropy_increment)
        };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::new("ESDM error add entropy count"))
}

pub fn esdm_reseed_crng() -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm_rpcc_rnd_reseed_crng()
        };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::new("ESDM error reseed crng"))
}

pub fn esdm_clear_pool() -> Result<(), Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let ret = unsafe {
            esdm_rpcc_rnd_clear_pool()
        };
        if ret == 0 {
            return Ok(());
        }
    }
    Err(Error::new("ESDM error clear pool"))
}

pub fn esdm_status_str() -> Result<String, Error> {
    for _ in 0..ESDM_RETRY_COUNT {
        let mut status_bytes = Vec::<u8>::new();
        status_bytes.resize(8192, 0);
        let ret = unsafe {
            esdm_rpcc_status(status_bytes.as_mut_ptr() as *mut c_char, status_bytes.len())
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
    Err(Error::new("ESDM error clear pool"))
}

// these tests assume a running esdm-server on the system!
#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_prediction_resistant_mode() {
        esdm_rng_init_checked();

        let mut rng = EsdmRngPredictionResistant {};

        for _ in 1..1000 {
            let rnd: u64 = rng.gen();
            println!("Random Number: {rnd:?}");
        }

        esdm_rng_fini();
    }

    #[test]
    #[serial]
    fn test_fully_seeded_mode() {
        esdm_rng_init_checked();

        let mut rng = EsdmRngFullySeeded {};
        for _ in 1..1000 {
            let rnd: u64 = rng.gen();
            println!("Random Number: {rnd:?}");
        }

        esdm_rng_fini();
    }

    #[test]
    #[serial]
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
    #[serial]
    fn test_privileged_interface() {
        // also need unprivileged interface for random bytes
        esdm_rng_init_checked();
        esdm_rng_init_priv_checked();

        esdm_clear_pool().unwrap();
        assert_eq!(esdm_get_entropy_count().unwrap(), 0);
        esdm_add_to_entropy_count(64 * 8).unwrap();
        esdm_reseed_crng().unwrap();

        let mut rng = EsdmRngPredictionResistant {};
        // don't do this in production: circular seeding
        let buf: [u8; 32] = rng.gen();
        esdm_clear_pool().unwrap();
        esdm_add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap()).unwrap();
        assert_eq!(esdm_get_entropy_count().unwrap(), 32 * 8);

        esdm_rng_fini_priv();
        esdm_rng_fini();
    }
}
