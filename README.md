# rand-esdm

[https://img.shields.io/crates/v/rand-esdm.svg](https://crates.io/crates/rand-esdm)

## About
A small library for interfacing Rust with the [ESDM](https://github.com/smuellerDD/esdm) user-space random server.

It currently provides the minimal amount of bindings necessary to use ESDM together with the [rand crate](https://github.com/rust-random/rand).

## Usage Example

### Add rand-esdm to your Cargo.toml

```toml
rand-esdm = "0.0.2"
```

### Init library once in your usage context

```rust
esdm_rng_init_checked();
```

### Generate Random Numbers with rand crate

Choose type of rng:

- Only usable when fully seeded: ```let mut rng = EsdmRngFullySeeded {};```
- Only usable with fresh entropy: ```let mut rng = EsdmRngPredictionResistant {};```

Include Rng utility trait from rand:
```rust
use rand::Rng;
```

Draw random numbers as needed, e.g.:
```rust  
let rnd: u64 = rng.gen();
```

### Destroy library context when done

```rust
esdm_rng_fini();
```
