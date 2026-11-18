use std::{env::var, path::PathBuf};

use bindgen::Builder;

fn main() {
    let rpc_client = pkg_config::Config::new()
        .atleast_version("1.2.4")
        .probe("esdm_rpc_client")
        .unwrap();
    let aux_client = pkg_config::Config::new().probe("esdm_aux_client").unwrap();
    pkg_config::Config::new().probe("libprotobuf-c").unwrap();

    let include_args = |lib: &pkg_config::Library| -> Vec<String> {
        lib.include_paths
            .iter()
            .map(|p| format!("-I{}", p.display()))
            .collect()
    };

    let bindings = Builder::default()
        .header("esdm-include.h")
        .clang_args(include_args(&rpc_client))
        .allowlist_function("esdm_rpcc_fini_priv_service")
        .allowlist_function("esdm_rpcc_fini_unpriv_service")
        .allowlist_function("esdm_rpcc_get_random_bytes_full")
        .allowlist_function("esdm_rpcc_get_random_bytes_pr")
        .allowlist_function("esdm_rpcc_init_priv_service")
        .allowlist_function("esdm_rpcc_init_unpriv_service")
        .allowlist_function("esdm_rpcc_rnd_add_entropy")
        .allowlist_function("esdm_rpcc_rnd_add_to_ent_cnt")
        .allowlist_function("esdm_rpcc_rnd_clear_pool")
        .allowlist_function("esdm_rpcc_rnd_get_ent_cnt")
        .allowlist_function("esdm_rpcc_rnd_reseed_crng")
        .allowlist_function("esdm_rpcc_set_max_online_nodes")
        .allowlist_function("esdm_rpcc_status")
        .allowlist_function("esdm_rpcc_jent_status")
        .allowlist_function("esdm_rpcc_write_data")
        .allowlist_function("esdm_rpcc_get_write_wakeup_thresh")
        // since ESDM 1.2.4
        .allowlist_function("esdm_rpcc_force_fini_priv_service")
        .allowlist_function("esdm_rpcc_force_fini_unpriv_service")
        .allowlist_function("esdm_rpcc_get_random_bytes_full_noblock")
        .allowlist_function("esdm_rpcc_get_random_bytes_pr_noblock")
        .allowlist_function("esdm_rpcc_status_json")
        .allowlist_function("esdm_rpcc_drng_status_json")
        .allowlist_function("esdm_rpcc_drng_status_pr_json")
        .allowlist_function("esdm_rpcc_selftest")
        .allowlist_function("esdm_rpcc_set_es_rate")
        .allowlist_type("esdm_rpcc_selftest_state")
        .rustified_enum("esdm_rpcc_selftest_state")
        .generate()
        .unwrap();
    let mut bindings_path = PathBuf::from(var("OUT_DIR").unwrap());
    bindings_path.push("esdm-bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Could not write bindings to file");

    let bindings = Builder::default()
        .header("esdm-aux-include.h")
        .clang_args(include_args(&aux_client))
        .allowlist_function("esdm_aux_fini_wait_for_need_entropy")
        .allowlist_function("esdm_aux_init_wait_for_need_entropy")
        .allowlist_function("esdm_aux_timedwait_for_need_entropy")
        .generate()
        .unwrap();
    let mut bindings_path = PathBuf::from(var("OUT_DIR").unwrap());
    bindings_path.push("esdm-aux-bindings.rs");
    bindings
        .write_to_file(&bindings_path)
        .expect("Could not write bindings to file");
}
