# rand-esdm

[![crates.io](https://img.shields.io/crates/v/rand-esdm.svg)](https://crates.io/crates/rand-esdm)

## About
A small library for interfacing Rust with the [ESDM](https://github.com/smuellerDD/esdm) user-space random server.

It currently provides the minimal amount of bindings necessary to use ESDM together with the [rand crate](https://github.com/rust-random/rand).

## Usage Example

### Add rand-esdm to your Cargo.toml

```toml
rand-esdm = "0.5.0"
```

### Generate Random Numbers with rand crate

Choose type of rng:

- Only usable when fully seeded: ```EsdmRng::new(EsdmRngType::FullySeeded)```
- Only usable with fresh entropy: ```EsdmRng::new(EsdmRngType::PredictionResistant)```

`EsdmRng` implements the fallible `TryRng` and `TryCryptoRng` traits. Wrap it
in `UnwrapErr` to use it as infallible `Rng`/`CryptoRng`, which panics if the
ESDM cannot deliver random bytes:
```rust
use rand::rand_core::UnwrapErr;

let mut rng = UnwrapErr(EsdmRng::new(EsdmRngType::FullySeeded));
```

Include RngExt utility trait from rand:
```rust
use rand::RngExt;
```

Draw random numbers as needed, e.g.:
```rust
let rnd: u64 = rng.random();
```

Complete toy example can be found in `examples/readme.rs`:
```rust
use rand::RngExt;
use rand::rand_core::UnwrapErr;
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() {
    let mut rng = UnwrapErr(EsdmRng::new(EsdmRngType::FullySeeded));
    let rnd: u32 = rng.random();
    println!("{rnd:X}");
}
```
