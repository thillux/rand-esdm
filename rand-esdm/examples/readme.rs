use rand::{Rng, TryRngCore};
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded).unwrap_err();
    let rnd: u32 = rng.random();
    println!("{rnd:X}");
}
