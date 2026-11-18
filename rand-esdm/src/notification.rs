//! notifications from the ESDM when it needs entropy

use libc::{EINTR, ETIMEDOUT};
use std::io::{Error, ErrorKind};
use std::sync::{Mutex, MutexGuard, PoisonError, TryLockError};
use std::thread;
use std::time::{Duration, Instant};

use esdm_sys::esdm_aux;

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

/// how often a wait checks whether the wait of another handle has finished
const AUX_WAIT_POLL: Duration = Duration::from_millis(10);

fn timed_out() -> Error {
    Error::new(
        ErrorKind::TimedOut,
        "timed out waiting for ESDM to need entropy",
    )
}

/// takes [`AUX_WAIT`], but gives up at `deadline` (`None`: never)
fn lock_aux_wait(deadline: Option<Instant>) -> Result<MutexGuard<'static, ()>, Error> {
    let Some(deadline) = deadline else {
        return Ok(AUX_WAIT.lock().unwrap_or_else(PoisonError::into_inner));
    };

    loop {
        match AUX_WAIT.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {}
        }

        let now = Instant::now();
        if now >= deadline {
            return Err(timed_out());
        }
        thread::sleep((deadline - now).min(AUX_WAIT_POLL));
    }
}

/// absolute `CLOCK_MONOTONIC` deadline `dur` from now, as the aux client
/// library expects it (it waits with `sem_clockwait()` on that clock)
// the conversions are no-ops on 64 bit targets only
#[allow(clippy::useless_conversion, clippy::unnecessary_fallible_conversions)]
fn monotonic_deadline(dur: Duration) -> Result<esdm_aux::timespec, Error> {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid, writable timespec for the whole call.
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &raw mut now) } != 0 {
        return Err(Error::last_os_error());
    }

    let nsec = i64::from(now.tv_nsec) + i64::from(dur.subsec_nanos());
    let sec = i64::from(now.tv_sec)
        .saturating_add(i64::try_from(dur.as_secs()).unwrap_or(i64::MAX))
        .saturating_add(nsec / 1_000_000_000);

    Ok(esdm_aux::timespec {
        // a deadline beyond the range of `time_t` is as good as none
        tv_sec: sec.try_into().unwrap_or(esdm_aux::__time_t::MAX),
        // always below 10^9, which fits into every `long`
        tv_nsec: (nsec % 1_000_000_000).try_into().unwrap_or_default(),
    })
}

/// waits for the ESDM server to ask for entropy
///
/// The ESDM signals a semaphore once its entropy level drops below
/// [`crate::EsdmConnection::write_wakeup_thresh`]. A process which can provide
/// entropy, e.g. from a hardware RNG, waits for that and then adds entropy
/// with [`crate::EsdmPrivConnection::add_entropy`]. Attaching to the semaphore
/// requires root.
///
/// Waiting only involves the semaphore, not the server: it neither needs a
/// connection nor notices a server which stopped or hangs. The calls which
/// follow a wake up report that.
///
/// Several handles may exist at once. They share the process wide state of the
/// aux client library and their waits take turns.
///
/// ```no_run
/// use std::time::Duration;
/// use rand_esdm::{EsdmNotification, EsdmPrivConnection};
///
/// # fn main() -> std::io::Result<()> {
/// let priv_conn = EsdmPrivConnection::try_new()?;
/// let mut notification = EsdmNotification::try_new()?;
/// loop {
///     match notification.wait_for_entropy_needed_timeout(Duration::from_secs(60)) {
///         Ok(()) => {
///             let entropy = [0u8; 32]; // read from your entropy source instead
///             priv_conn.add_entropy(&entropy, 256)?;
///         }
///         Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
///         Err(e) => return Err(e),
///     }
/// }
/// # }
/// ```
#[derive(Debug)]
pub struct EsdmNotification {
    // keeps the handle from being built without `try_new`, which initializes
    // the aux client library that `Drop` releases again
    _private: (),
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
    /// attaches to the semaphore, see [`Self::try_new`]
    ///
    /// # Panics
    ///
    /// if the aux client library cannot be initialized, see [`Self::try_new`]
    // no `Default`: it would panic without root privileges
    #[allow(clippy::new_without_default)]
    #[must_use]
    pub fn new() -> Self {
        Self::try_new().expect("unable to initialize ESDM aux library")
    }

    /// attaches to the semaphore and shared memory segment the ESDM server
    /// uses to signal that it needs entropy
    ///
    /// Fails with [`ErrorKind::PermissionDenied`] without root privileges.
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

    /// waits up to `dur` until the ESDM server asks for entropy
    ///
    /// Returns `Ok(())` once it asked, or fails with [`ErrorKind::TimedOut`]
    /// if it did not ask within `dur`, which bounds the whole call. Waits of
    /// several handles take turns, the time spent waiting for the turn counts
    /// towards `dur` as well.
    ///
    /// The ESDM keeps asking as long as its entropy level stays below the
    /// threshold, so a wait returns right away until enough entropy was
    /// added. [`crate::EsdmConnection::entropy_count`] tells the current
    /// level.
    pub fn wait_for_entropy_needed_timeout(&mut self, dur: Duration) -> Result<(), Error> {
        let mut deadline = monotonic_deadline(dur)?;

        let _wait = lock_aux_wait(Instant::now().checked_add(dur))?;
        loop {
            // SAFETY: `self` keeps the aux library initialized, as fini
            // only runs once the last handle is dropped. Init only runs
            // while no handle exists, and `AUX_WAIT` serializes the waits.
            // The library only reads `deadline`.
            let ret = unsafe { esdm_aux::esdm_aux_timedwait_for_need_entropy(&raw mut deadline) };
            if ret == 0 {
                return Ok(());
            }

            // the call follows sem_clockwait(): -1 and the reason in errno
            let err = Error::last_os_error();
            match err.raw_os_error() {
                Some(EINTR) => {}
                Some(ETIMEDOUT) => return Err(timed_out()),
                _ => return Err(err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_aux_wait_honors_deadline() {
        // stands in for the wait of another handle
        let held = AUX_WAIT.lock().unwrap_or_else(PoisonError::into_inner);

        let start = Instant::now();
        let err = lock_aux_wait(Some(start + Duration::from_millis(50))).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(5));

        drop(held);
        // generous: other tests may wait on the aux library meanwhile
        assert!(lock_aux_wait(Some(Instant::now() + Duration::from_secs(5))).is_ok());
    }

    #[test]
    fn deadline_is_normalized() {
        let deadline = monotonic_deadline(Duration::new(1, 999_999_999)).unwrap();
        assert!((0..1_000_000_000).contains(&deadline.tv_nsec));

        let far = monotonic_deadline(Duration::MAX).unwrap();
        assert_eq!(far.tv_sec, esdm_aux::__time_t::MAX);
    }
}
