use std::time::Duration;

use rand_esdm::{
    esdm_add_entropy, esdm_add_to_entropy_count, esdm_rng_fini, esdm_rng_fini_priv,
    esdm_rng_init_checked, esdm_rng_init_priv_checked, EsdmNotification,
};

fn main() {
    esdm_rng_init_checked();
    esdm_rng_init_priv_checked();

    let mut notifier = EsdmNotification::new();

    esdm_add_entropy(&[0; 32], 256).unwrap();
    esdm_add_to_entropy_count(256).unwrap();
    loop {
        let res = notifier.wait_for_entropy_needed_timeout(Duration::from_secs(10));
        match res {
            Ok(entropy) => {
                println!("Entropy count: {entropy}")
            }
            _ => {
                println!("error or timeout during entropy fetch");
            }
        }
        esdm_add_entropy(&[0; 32], 256).unwrap();
        esdm_add_to_entropy_count(256).unwrap();
    }

    esdm_rng_fini();
    esdm_rng_fini_priv();
}
