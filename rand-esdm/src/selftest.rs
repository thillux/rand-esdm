//! ESDM self tests (privileged interface)

use libc::EINTR;
use std::io::Error;

use esdm_sys::esdm;

use crate::rpc::rpc_error;

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
            // also covers states unknown to this crate, as the client library
            // passes on whatever the server sends
            _ => Self::Failed,
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
        // SAFETY: `result` is a writable `esdm_rpcc_selftest_result`. Its
        // states are newtypes over `c_uint`, so any value the server sends is
        // valid, see `rpc.rs`.
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

#[cfg(test)]
mod tests {
    use super::*;
    use esdm::esdm_rpcc_selftest_state as State;

    #[test]
    fn state_conversion() {
        assert_eq!(
            EsdmSelftestState::from(State::esdm_rpcc_selftest_undone),
            EsdmSelftestState::Undone
        );
        assert_eq!(
            EsdmSelftestState::from(State::esdm_rpcc_selftest_passed),
            EsdmSelftestState::Passed
        );
        assert_eq!(
            EsdmSelftestState::from(State::esdm_rpcc_selftest_failed),
            EsdmSelftestState::Failed
        );
        // values unknown to this crate count as failure
        assert_eq!(
            EsdmSelftestState::from(State(0xdead)),
            EsdmSelftestState::Failed
        );
    }
}
