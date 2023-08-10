use rand_core::{Error, RngCore};
use std::ffi::{c_uchar, c_void};
use std::ptr::null_mut;

/*
 * private ESDM RPC client function definitions
 */
extern "C" {
    fn esdm_rpcc_init_unpriv_service(handler: *mut c_void) -> i32;
    fn esdm_rpcc_fini_unpriv_service();

    fn esdm_rpcc_get_random_bytes_full(buf: *mut c_uchar, buflen: usize) -> isize;
    fn esdm_rpcc_get_random_bytes_pr(buf: *mut c_uchar, buflen: usize) -> isize;
}

/// ESDM RNG implementation, which only produces random numbers when fully seeded
/// otherwise it times out and returns an error after a few internal tries
pub struct EsdmRngFullySeeded {}

/// ESDM RNG implementation, which uses fresh entropy for every random output produced
pub struct EsdmRngPredictionResistant {}

/// Returns if the client connection to ESDM was initialized succesfully
/// Only needed to call once globally before first usage of ESDM
pub fn esdm_rng_init() -> bool {
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
        for _ in 1..10 {
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
        for _ in 1..10 {
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
        esdm_rng_init();

        let mut rng = EsdmRngFullySeeded {};
        for _ in 1..1000 {
            let rnd: u64 = rng.gen();
            println!("Random Number: {rnd:?}");
        }

        esdm_rng_fini();
    }
}
