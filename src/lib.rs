use std::ffi::{c_uchar, c_void};
use std::ptr::null_mut;
use rand_core::{RngCore, Error};

extern "C" {
    fn esdm_rpcc_init_unpriv_service(handler: *mut c_void) -> i32;
    fn esdm_rpcc_fini_unpriv_service();
    fn esdm_rpcc_get_random_bytes_full(buf: *mut c_uchar, buflen: usize) -> isize;
    fn esdm_rpcc_get_random_bytes_pr(buf: *mut c_uchar, buflen: usize) -> isize;
}

pub struct EsdmRngFullySeeded { }
pub struct EsdmRngPredictionResistant { }

pub fn esdm_rng_init() {
    let ret = unsafe { esdm_rpcc_init_unpriv_service(null_mut()) };
    assert_eq!(ret, 0);
}

pub fn esdm_rng_fini() {
    unsafe { esdm_rpcc_fini_unpriv_service() };
}

impl RngCore for EsdmRngPredictionResistant {
    fn next_u32(&mut self) -> u32 {
        return self.next_u64() as u32;
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes: [u8; 8] = [0; 8];
        self.fill_bytes(&mut bytes);
        let i = u64::from_ne_bytes(bytes);
        return i;
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for _ in 1..10 {
            let ret_size = unsafe {
                esdm_rpcc_get_random_bytes_pr(dest.as_mut_ptr(), dest.len())
            };
            if ret_size == dest.len() as isize {
                return;
            }
        }
        panic!("cannot get random bytes from ESDM!");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        Ok(self.fill_bytes(dest))
    }
}

impl RngCore for EsdmRngFullySeeded {
    fn next_u32(&mut self) -> u32 {
        return self.next_u64() as u32;
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes: [u8; 8] = [0; 8];
        self.fill_bytes(&mut bytes);
        let i = u64::from_ne_bytes(bytes);
        return i;
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for _ in 1..10 {
            let ret_size = unsafe {
                esdm_rpcc_get_random_bytes_full(dest.as_mut_ptr(), dest.len())
            };
            if ret_size == dest.len() as isize {
                return;
            }
        }
        panic!("cannot get random bytes from ESDM!");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        Ok(self.fill_bytes(dest))
    }
}
