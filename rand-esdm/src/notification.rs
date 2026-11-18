//! notifications from the ESDM when it needs entropy

use libc::{EINTR, ETIMEDOUT};
use std::io::{Error, ErrorKind};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use esdm_sys::esdm_aux;

use crate::entropy::esdm_get_entropy_count;
use crate::rpc::rpc_error;

/// number of live [`EsdmNotification`] handles
///
/// The aux client library keeps its semaphore and shared memory mapping in
/// process global variables, without locking or reference counting: a second
/// init leaks the first mapping and every fini tears both down for all users.
/// The library is therefore initialized when this counter leaves 0 and
/// released when it returns to 0, both while holding the lock.
static AUX_USERS: Mutex<u32> = Mutex::new(0);

/// serializes waits, as the aux client library is not thread-safe
static AUX_WAIT: Mutex<()> = Mutex::new(());

/// waits for the ESDM server to ask for entropy
///
/// Several handles may exist at once. They share the process wide state of the
/// aux client library and their waits take turns.
#[derive(Debug)]
pub struct EsdmNotification {
    // keeps the handle from being built without `try_new`, which initializes
    // the aux client library that `Drop` releases again
    _private: (),
}

impl Default for EsdmNotification {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EsdmNotification {
    fn drop(&mut self) {
        let mut users = AUX_USERS.lock().unwrap_or_else(PoisonError::into_inner);
        *users -= 1;
        if *users == 0 {
            // SAFETY: this was the last handle, so no wait is in progress, and
            // holding `AUX_USERS` keeps a concurrent init out. It releases the
            // state set up by the init for the first handle.
            unsafe { esdm_aux::esdm_aux_fini_wait_for_need_entropy() };
        }
    }
}

impl EsdmNotification {
    /// # Panics
    ///
    /// if the aux client library cannot be initialized, see [`Self::try_new`]
    #[must_use]
    pub fn new() -> Self {
        Self::try_new().expect("unable to initialize ESDM aux library")
    }

    /// attaches to the semaphore and shared memory segment the ESDM server
    /// uses to signal that it needs entropy
    pub fn try_new() -> Result<Self, Error> {
        let mut users = AUX_USERS.lock().unwrap_or_else(PoisonError::into_inner);
        if *users == 0 {
            // SAFETY: no handle exists and `AUX_USERS` is held, so no other
            // thread is inside the aux library. The call takes no arguments.
            let ret = unsafe { esdm_aux::esdm_aux_init_wait_for_need_entropy() };
            if ret != 0 {
                // SAFETY: as above. A failed init may leave the semaphore
                // open; fini closes whatever part was set up.
                unsafe { esdm_aux::esdm_aux_fini_wait_for_need_entropy() };
                return Err(rpc_error(ret, "unable to initialize ESDM aux library"));
            }
        }
        *users += 1;

        Ok(Self { _private: () })
    }

    /// waits up to `dur` until the ESDM server needs entropy and returns the
    /// entropy count of its auxiliary pool
    ///
    /// Fails with [`ErrorKind::TimedOut`] if the server did not ask for
    /// entropy in time. Fetching the entropy count requires an unprivileged
    /// connection, see [`crate::esdm_rng_init`].
    pub fn wait_for_entropy_needed_timeout(&mut self, dur: Duration) -> Result<u32, Error> {
        let mut now = libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        // SAFETY: `now` is a valid, writable timespec for the whole call.
        if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut now) } != 0 {
            return Err(Error::last_os_error());
        }

        let nsec = now.tv_nsec + i64::from(dur.subsec_nanos());
        let mut deadline = esdm_aux::timespec {
            tv_sec: now
                .tv_sec
                .saturating_add(i64::try_from(dur.as_secs()).unwrap_or(i64::MAX))
                .saturating_add(nsec / 1_000_000_000),
            tv_nsec: nsec % 1_000_000_000,
        };

        {
            let _wait = AUX_WAIT.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                // SAFETY: `self` keeps the aux library initialized, as fini
                // only runs once the last handle is dropped. Init only runs
                // while no handle exists, and `AUX_WAIT` serializes the waits.
                // The library only reads `deadline`.
                let ret =
                    unsafe { esdm_aux::esdm_aux_timedwait_for_need_entropy(&raw mut deadline) };
                if ret == 0 {
                    break;
                }

                // the call follows sem_clockwait(): -1 and the reason in errno
                let err = Error::last_os_error();
                match err.raw_os_error() {
                    Some(EINTR) => {}
                    Some(ETIMEDOUT) => {
                        return Err(Error::new(
                            ErrorKind::TimedOut,
                            "timed out waiting for ESDM to need entropy",
                        ));
                    }
                    _ => return Err(err),
                }
            }
        }

        esdm_get_entropy_count()
    }
}
