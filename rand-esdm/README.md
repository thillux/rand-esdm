# rand-esdm

[![crates.io](https://img.shields.io/crates/v/rand-esdm.svg)](https://crates.io/crates/rand-esdm)

## About
A small library for interfacing Rust with the [ESDM](https://github.com/smuellerDD/esdm) user-space random server.

It currently provides the minimal amount of bindings necessary to use ESDM together with the [rand crate](https://github.com/rust-random/rand).

## Usage Example

### Add rand-esdm to your Cargo.toml

```toml
rand-esdm = "0.2.1"
```

### Generate Random Numbers with rand crate

Choose type of rng:

- Only usable when fully seeded: ```let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);```
- Only usable with fresh entropy: ```let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);```

Include Rng utility trait from rand:
```rust
use rand::Rng;
```

Draw random numbers as needed, e.g.:
```rust  
let rnd: u64 = rng.random();
```

Complete toy example can be found in `examples/readme.rs`:
```rust
use rand::{Rng, TryRngCore};
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded).unwrap_err();
    let rnd: u32 = rng.random();
    println!("{rnd:X}");
}
```
