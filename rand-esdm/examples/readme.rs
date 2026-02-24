use rand::RngExt;
use rand::rand_core::UnwrapErr;
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() {
    let mut rng = UnwrapErr(EsdmRng::new(EsdmRngType::FullySeeded));
    let rnd: u32 = rng.random();
    println!("{rnd:X}");
}
