//! The minimal example of the README: one random number via the rand API.
//!
//! Needs a running `esdm-server`: `cargo run --example readme`

use rand::RngExt;
use rand::rand_core::UnwrapErr;
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() {
    let mut rng = UnwrapErr(EsdmRng::new(EsdmRngType::FullySeeded));
    let rnd: u32 = rng.random();
    println!("{rnd:X}");
}
