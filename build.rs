fn main() {
    pkg_config::Config::new().probe("esdm_rpc_client").unwrap();
    pkg_config::Config::new().probe("esdm-aux").unwrap();
}
