//! Draws random numbers from the ESDM: with the fallible `rand_core` API,
//! with the infallible `rand` helpers, from the prediction resistant DRNG,
//! and as seed of a fast user space RNG.
//!
//! Needs a running `esdm-server`: `cargo run --example random`

use std::fmt::Write;

use rand::rand_core::{TryRng, UnwrapErr};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::{RngExt, SeedableRng};
use rand_esdm::{EsdmRng, EsdmRngType};
use zeroize::Zeroizing;

fn main() -> std::io::Result<()> {
    // The fallible API returns the errors of the ESDM instead of panicking.
    let mut rng = EsdmRng::try_new(EsdmRngType::FullySeeded)?;
    let mut bytes = [0u8; 16];
    rng.try_fill_bytes(&mut bytes)?;
    println!("16 random bytes:      {}", hex(&bytes));
    println!("random u64:           {:#018x}", rng.try_next_u64()?);

    // `UnwrapErr` turns it into an infallible `Rng` for all helpers of rand.
    // It panics if the ESDM cannot deliver.
    let mut rng = UnwrapErr(rng);
    let dice: u8 = rng.random_range(1..=6);
    let coin = rng.random_bool(0.5);
    let mut cards: Vec<u8> = (1..=10).collect();
    cards.shuffle(&mut rng);
    println!("dice: {dice}, coin: {coin}, shuffled: {cards:?}");

    // Every request goes to the server. For masses of random numbers in a
    // hot loop, seed a user space CSPRNG from the ESDM once instead.
    let mut fast = StdRng::try_from_rng(&mut rng.0)?;
    let sample: [u32; 4] = fast.random();
    println!("StdRng seeded by ESDM: {sample:?}");

    // The prediction resistant DRNG uses fresh entropy for every request.
    // It is limited by the rate of the entropy sources, so keep it for long
    // term secrets.
    let mut pr = EsdmRng::try_new(EsdmRngType::PredictionResistant)?;
    let mut long_term_key = Zeroizing::new([0u8; 32]);
    pr.try_fill_bytes(&mut *long_term_key)?;
    println!(
        "generated a {} byte key with prediction resistance",
        long_term_key.len()
    );

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}
