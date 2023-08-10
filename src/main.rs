use rand_esdm::{esdm_rng_init, esdm_rng_fini, EsdmRngFullySeeded};
use rand::Rng;

fn main() {
    esdm_rng_init();

    let mut rng = EsdmRngFullySeeded {};
    let rnd : u64 = rng.gen();

    println!("Random Number: {rnd:?}");

    esdm_rng_fini();
}
