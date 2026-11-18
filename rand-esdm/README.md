# rand-esdm

[![crates.io](https://img.shields.io/crates/v/rand-esdm.svg)](https://crates.io/crates/rand-esdm)
[![docs.rs](https://docs.rs/rand-esdm/badge.svg)](https://docs.rs/rand-esdm)

Rust interface to the [ESDM](https://www.chronox.de/esdm/) (Entropy Source and
DRNG Manager), a random number server running in user space.

- `EsdmRng`: a cryptographically secure RNG implementing the `rand_core`
  traits, usable with everything in the [rand](https://docs.rs/rand)
  ecosystem
- `EsdmConnection`: the unprivileged interface, e.g. the status of the ESDM,
  as text or typed
- `EsdmPrivConnection`: the privileged interface to feed and control the
  entropy pool (`add_entropy`, `crng_reseed`, `selftest`, ...)
- `EsdmNotification`: waits until the ESDM asks for entropy

## Requirements

A running `esdm-server` of ESDM 1.2.4 or later, and its client libraries at
build time, see [`esdm-sys`](https://crates.io/crates/esdm-sys).

```toml
[dependencies]
rand-esdm = "0.5.0"
rand = "0.10"
```

## Random numbers

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

`EsdmRng` implements the fallible `TryRng` and `TryCryptoRng` traits. Wrap it
in `UnwrapErr`, as above, to use it as infallible `Rng`/`CryptoRng`, which
panics if the ESDM cannot deliver. To handle errors instead:

```rust
use rand::rand_core::TryRng;
use rand_esdm::{EsdmRng, EsdmRngType};

fn main() -> std::io::Result<()> {
    let mut rng = EsdmRng::try_new(EsdmRngType::FullySeeded)?;
    let mut key = [0u8; 32];
    rng.try_fill_bytes(&mut key)?;
    Ok(())
}
```

There are two modes:

| `EsdmRngType` | Semantics | Use for |
| --- | --- | --- |
| `FullySeeded` | `getrandom()`: output of the DRNGs, only once the ESDM is fully seeded | almost everything, it is fast |
| `PredictionResistant` | `getrandom(GRND_RANDOM)`: fresh entropy for every request | long term secrets; limited by the entropy sources, possibly only a few KiB/s |

## Status

```rust
use rand_esdm::EsdmConnection;

fn main() -> std::io::Result<()> {
    let conn = EsdmConnection::try_new()?;
    let status = conn.status()?;
    println!(
        "fully seeded: {}, entropy level: {} bits",
        status.fully_seeded, status.entropy_level_bits
    );
    Ok(())
}
```

## Connections and privileges

Every call to the ESDM is a method of the connection it needs, so it cannot
be made without one:

| Type | Offers | Needs |
| --- | --- | --- |
| `EsdmRng` | random numbers | nothing, it holds a connection of its own |
| `EsdmConnection` | `status`, `status_json`, `status_text`, `drng_status`, `entropy_count`, `write_wakeup_thresh`, `write_data`, ... | nothing |
| `EsdmPrivConnection` | `add_entropy`, `add_to_entropy_count`, `clear_pool`, `crng_reseed`, `selftest`, `set_es_rate` | root, the calls fail with `ErrorKind::PermissionDenied` otherwise |
| `EsdmNotification` | `wait_for_entropy_needed_timeout` | root |

```rust
use rand_esdm::EsdmPrivConnection;

fn main() -> std::io::Result<()> {
    let priv_conn = EsdmPrivConnection::try_new()?;
    let entropy = [0u8; 32]; // read from a real entropy source instead
    priv_conn.add_entropy(&entropy, 256)?;
    Ok(())
}
```

Connections are cheap: the client library connects lazily and all
connections of a process share one connection pool. Since the library
connects on the first request and reconnects on its own, a connection or RNG
can be created before the server runs and keeps working across server
restarts. Without a server, calls fail with `ErrorKind::NotFound`; once a
running server is stopped, with `ErrorKind::ConnectionRefused` after the
library tried to reconnect for a few seconds. A server which hangs is reported
with `ErrorKind::TimedOut` after a few seconds of silence. All types can be used
from any thread, connections can be shared by reference or cloned.

## Examples

Complete programs in [`examples`](examples), run them with
`cargo run --example <name>`:

| Example | Shows |
| --- | --- |
| [`readme`](examples/readme.rs) | the minimal example above |
| [`random`](examples/random.rs) | both RNG modes, rand helpers, seeding a user space RNG |
| [`status`](examples/status.rs) | a summary of the typed status |
| [`feed_entropy`](examples/feed_entropy.rs) | privileged: provide entropy whenever the ESDM asks for it |

## Upgrading from 0.4

- `esdm_rng_init*`/`esdm_rng_fini*` are gone, and so are the free functions
  which needed them. Create an `EsdmConnection` or `EsdmPrivConnection` and
  call its methods; it is released when dropped:

  | 0.4 | 0.5 |
  | --- | --- |
  | `esdm_status_str()` | `conn.status_text()` |
  | `esdm_jent_status_str()` | `conn.jent_status_text()` |
  | `esdm_get_entropy_count()` | `conn.entropy_count()` |
  | `esdm_write_wakeup_thresh()` | `conn.write_wakeup_thresh()` |
  | `esdm_write_data()` | `conn.write_data()` |
  | `esdm_add_entropy()` | `priv_conn.add_entropy()` |
  | `esdm_add_to_entropy_count()` | `priv_conn.add_to_entropy_count()` |
  | `esdm_clear_pool()` | `priv_conn.clear_pool()` |
  | `esdm_crng_reseed()`, `esdm_reseed_crng()` | `priv_conn.crng_reseed()` |

  `esdm_is_fully_seeded`, `esdm_get_entropy_level` and
  `esdm_set_max_online_nodes` remain free functions.
- New: typed and JSON status reports (`conn.status()`, `conn.drng_status()`,
  ...), `priv_conn.selftest()`, `priv_conn.set_es_rate()`, and
  `TryCryptoRng` for `EsdmRng`.
- `EsdmNotification` holds its own connection and no longer implements
  `Default`.
- The status and self test types are `#[non_exhaustive]`.
- ESDM 1.2.4 or later is required.

## License

MIT
