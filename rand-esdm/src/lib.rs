//! Rust interface to the ESDM (Entropy Source and DRNG Manager) RNG server

mod connection;
mod entropy;
mod notification;
mod rng;
mod rpc;
mod selftest;
mod status;
mod status_json;

#[cfg(test)]
mod tests;

pub use connection::{
    esdm_rng_fini, esdm_rng_fini_priv, esdm_rng_init, esdm_rng_init_checked, esdm_rng_init_priv,
    esdm_rng_init_priv_checked, esdm_set_max_online_nodes,
};
#[expect(deprecated)]
pub use entropy::esdm_reseed_crng;
pub use entropy::{
    esdm_add_entropy, esdm_add_to_entropy_count, esdm_clear_pool, esdm_crng_reseed,
    esdm_get_entropy_count, esdm_set_es_rate, esdm_write_data, esdm_write_wakeup_thresh,
};
pub use notification::EsdmNotification;
pub use rng::{EsdmRng, EsdmRngType};
pub use selftest::{EsdmSelftestResult, EsdmSelftestState, esdm_selftest};
pub use status::{
    esdm_drng_status, esdm_drng_status_json_str, esdm_drng_status_pr, esdm_drng_status_pr_json_str,
    esdm_get_entropy_level, esdm_is_fully_seeded, esdm_jent_status_str, esdm_status,
    esdm_status_json_str, esdm_status_str,
};
pub use status_json::{
    EsdmDrngStatus, EsdmDrngType, EsdmEntropySourceStatus, EsdmPeriodicSelftestStatus,
    EsdmReseedWorkerStatus, EsdmStatus,
};
