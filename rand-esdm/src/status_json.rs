//! typed model of the JSON status documents of the ESDM (ESDM >= 1.2.4)
//!
//! Unknown members are ignored, so documents of newer ESDM versions which add
//! members still parse. All types are `#[non_exhaustive]`, so this crate can
//! model such members later without breaking users. Integer types follow the
//! C types the ESDM renders.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::selftest::EsdmSelftestState;

/// status of the ESDM, see [`crate::EsdmConnection::status`]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct EsdmStatus {
    /// version of the ESDM server
    pub library_version: String,
    /// whether the ESDM was built in test mode
    pub test_mode: bool,
    /// name of the DRNG implementation
    pub drng_name: String,
    /// security strength of the DRNG in bits
    pub security_strength_bits: u32,
    /// number of DRNG instances, at most one per node (CPU)
    pub drng_instances: u32,
    /// standards the ESDM complies with, e.g. `"DRG.4"` or `"SP800-90C"`
    pub standards_compliance: Vec<String>,
    /// whether the ESDM was seeded with at least as much entropy as its
    /// security strength, i.e. [`crate::EsdmRngType::FullySeeded`] is served
    pub fully_seeded: bool,
    /// entropy in bits currently available from all entropy sources together
    pub entropy_level_bits: u32,
    /// the entropy sources compiled into the ESDM, see
    /// [`EsdmEntropySourceStatus`]
    pub entropy_sources: Vec<EsdmEntropySourceStatus>,
    /// the initial and the prediction resistant DRNG
    pub drngs: Vec<EsdmDrngStatus>,
    /// worker reseeding the DRNGs periodically
    pub reseed_worker: EsdmReseedWorkerStatus,
    /// self tests the ESDM runs periodically
    pub periodic_self_test: EsdmPeriodicSelftestStatus,
}

/// status of one entropy source
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct EsdmEntropySourceStatus {
    /// index of the source within the ESDM
    pub id: u32,
    /// name of the source, e.g. `"JitterRNG"` or `"Auxiliary"`
    ///
    /// This is the name [`crate::EsdmPrivConnection::set_es_rate`] expects.
    pub name: String,
    /// entropy currently available from this source in bits
    ///
    /// `None` if the source is disabled, see [`Self::disabled_reason`], or
    /// its state could not be rendered.
    pub available_entropy: Option<i64>,
    /// entropy in bits this source credits to 256 bits of data, `None` like
    /// [`Self::available_entropy`]
    pub entropy_rate_per_256_data_bits: Option<i64>,
    /// further members specific to this kind of entropy source
    #[serde(flatten)]
    pub details: Map<String, Value>,
}

impl EsdmEntropySourceStatus {
    /// why the source is disabled, e.g. `"missing kernel support"`, `None`
    /// if it is enabled
    #[must_use]
    pub fn disabled_reason(&self) -> Option<&str> {
        self.details.get("disabled").and_then(Value::as_str)
    }
}

/// role of a DRNG instance
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub enum EsdmDrngType {
    /// the DRNG which is seeded first and serves node 0
    #[serde(rename = "initial")]
    Initial,
    /// a DRNG serving a further node
    #[serde(rename = "node")]
    Node,
    /// the prediction resistant DRNG, see
    /// [`crate::EsdmRngType::PredictionResistant`]
    #[serde(rename = "prediction resistance")]
    PredictionResistance,
    /// a role unknown to this crate
    #[serde(other)]
    Unknown,
}

/// status of one DRNG instance, see [`crate::EsdmConnection::drng_status`]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct EsdmDrngStatus {
    /// role of this DRNG
    #[serde(rename = "type")]
    pub drng_type: EsdmDrngType,
    /// node served by this DRNG, `None` if it is not bound to one
    pub node: Option<u32>,
    /// name of the DRNG implementation, `None` if not allocated yet
    pub name: Option<String>,
    /// whether this DRNG was seeded with full security strength
    pub fully_seeded: bool,
    /// whether this DRNG was seeded at all yet
    pub initiated: bool,
    /// whether the next request reseeds this DRNG first
    pub force_reseed: bool,
    /// generate requests left before a reseed is due, may be negative
    pub requests_until_reseed: i32,
    /// zero if the reseed is due already
    pub seconds_until_reseed: i64,
    /// generate requests served since it was last fully seeded
    pub requests_since_fully_seeded: u32,
    /// bits generated since it was last fully seeded
    pub generated_bits_since_fully_seeded: u32,
    /// number of completed seeding operations
    pub seed_generation: i64,
    /// UTC timestamp of the last seeding (`YYYY-MM-DDTHH:MM:SSZ`), `None` if
    /// never seeded
    pub last_seeding_time: Option<String>,
    /// seconds since the last seeding, `None` if never seeded
    pub seconds_since_last_reseed: Option<i64>,
}

/// status of the reseed worker
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct EsdmReseedWorkerStatus {
    /// whether the worker is running
    pub running: bool,
    /// longest time in seconds a DRNG is used before it is reseeded
    pub reseed_interval_seconds: u32,
}

/// status of the periodic self tests
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct EsdmPeriodicSelftestStatus {
    /// state of the self tests of the hash and the DRNG
    pub state: EsdmSelftestState,
    /// whether the periodic self tests are running
    pub running: bool,
    /// number of self test runs completed so far
    pub completed_self_tests: i64,
    /// time in seconds between two self test runs
    pub self_test_interval_seconds: u32,
    /// state of the self tests of the entropy sources
    pub entropy_source_state: EsdmSelftestState,
    /// number of entropy sources tested in the last run
    pub entropy_sources_tested: u32,
    /// number of entropy sources which failed in the last run
    pub entropy_sources_failed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"{
      "library_version": "1.2.4",
      "test_mode": false,
      "drng_name": "ChaCha20 DRNG",
      "security_strength_bits": 256,
      "drng_instances": 4,
      "standards_compliance": ["DRG.3", "DRG.4", "SP800-90C"],
      "fully_seeded": true,
      "entropy_level_bits": 256,
      "entropy_sources": [
        {
          "id": 0,
          "name": "JitterRNG",
          "available_entropy": 256,
          "library_version": 3070000,
          "standards_compliance": "SP800-90B",
          "entropy_rate_per_256_data_bits": 256,
          "oversampling_rate": 1
        },
        { "id": 1, "name": "Auxiliary" },
        { "id": 2, "name": "Interrupt", "disabled": "missing kernel support" }
      ],
      "drngs": [
        {
          "type": "initial",
          "node": null,
          "name": "ChaCha20 DRNG",
          "fully_seeded": true,
          "initiated": true,
          "force_reseed": false,
          "requests_until_reseed": -3,
          "seconds_until_reseed": 0,
          "requests_since_fully_seeded": 12,
          "generated_bits_since_fully_seeded": 4096,
          "seed_generation": 2,
          "last_seeding_time": "2026-11-18T10:00:00Z",
          "seconds_since_last_reseed": 42
        },
        {
          "type": "prediction resistance",
          "node": null,
          "name": null,
          "fully_seeded": false,
          "initiated": false,
          "force_reseed": false,
          "requests_until_reseed": 0,
          "seconds_until_reseed": 0,
          "requests_since_fully_seeded": 0,
          "generated_bits_since_fully_seeded": 0,
          "seed_generation": 0,
          "last_seeding_time": null,
          "seconds_since_last_reseed": null,
          "future_member": 1
        }
      ],
      "reseed_worker": { "running": true, "reseed_interval_seconds": 600 },
      "periodic_self_test": {
        "state": "passed",
        "running": true,
        "completed_self_tests": 3,
        "self_test_interval_seconds": 3600,
        "entropy_source_state": "undone",
        "entropy_sources_tested": 2,
        "entropy_sources_failed": 0
      }
    }"#;

    #[test]
    fn parses_status() {
        let status: EsdmStatus = serde_json::from_str(STATUS).unwrap();
        assert!(status.fully_seeded);
        assert_eq!(status.entropy_level_bits, 256);
        assert_eq!(status.standards_compliance, ["DRG.3", "DRG.4", "SP800-90C"]);

        let jent = &status.entropy_sources[0];
        assert_eq!(jent.available_entropy, Some(256));
        assert_eq!(jent.details["oversampling_rate"], 1);
        assert!(!jent.details.contains_key("id"));
        let aux = &status.entropy_sources[1];
        assert_eq!(aux.available_entropy, None);
        assert!(aux.details.is_empty());
        assert_eq!(aux.disabled_reason(), None);
        let irq = &status.entropy_sources[2];
        assert_eq!(irq.disabled_reason(), Some("missing kernel support"));
        assert_eq!(irq.available_entropy, None);

        let [init, pr] = status.drngs.as_slice() else {
            panic!("expected two DRNGs");
        };
        assert_eq!(init.drng_type, EsdmDrngType::Initial);
        assert_eq!(init.requests_until_reseed, -3);
        assert_eq!(
            init.last_seeding_time.as_deref(),
            Some("2026-11-18T10:00:00Z")
        );
        assert_eq!(pr.drng_type, EsdmDrngType::PredictionResistance);
        assert_eq!(pr.name, None);
        assert_eq!(pr.seconds_since_last_reseed, None);

        assert_eq!(status.periodic_self_test.state, EsdmSelftestState::Passed);
        assert_eq!(
            status.periodic_self_test.entropy_source_state,
            EsdmSelftestState::Undone
        );
    }

    #[test]
    fn unknown_enum_values() {
        let drng_type: EsdmDrngType = serde_json::from_str(r#""something new""#).unwrap();
        assert_eq!(drng_type, EsdmDrngType::Unknown);
        let drng_type: EsdmDrngType = serde_json::from_str(r#""node""#).unwrap();
        assert_eq!(drng_type, EsdmDrngType::Node);

        // same as the RPC client: states unknown to this crate count as failed
        let state: EsdmSelftestState = serde_json::from_str(r#""bogus""#).unwrap();
        assert_eq!(state, EsdmSelftestState::Failed);
    }
}
