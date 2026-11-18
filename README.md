# ESDM Rust Crates

Rust support for the [ESDM](https://www.chronox.de/esdm/) (Entropy Source and
DRNG Manager), a random number server running in user space. Its sources are
on [GitHub](https://github.com/smuellerDD/esdm).

| Crate | Purpose |
| --- | --- |
| [`rand-esdm`](rand-esdm) | Safe Rust API: an RNG for the [rand](https://docs.rs/rand) ecosystem, status reports, the privileged entropy interface |
| [`esdm-sys`](esdm-sys) | Raw FFI bindings to the ESDM client libraries, used by `rand-esdm` |
| [`esdm-tool`](esdm-tool) | Command line tool to query and control a running ESDM |
| [`benchmark`](benchmark) | Compares the throughput of ESDM with Rust RNGs |

Most users only need `rand-esdm`, see its [README](rand-esdm/README.md).

## Requirements

- ESDM 1.2.4 or later: a running `esdm-server` and its client libraries
  (`esdm_rpc_client`, `esdm_aux_client`), found via `pkg-config`
- `protobuf-c`, `pkg-config` and `clang` (for the bindings) at build time
- Rust 1.88 or later

The Nix flake provides all of it, including a matching ESDM build:

```sh
nix develop            # shell with ESDM, Rust and the build dependencies
nix build .#esdm       # build the ESDM server
sudo ./result/bin/esdm-server -f
```

## Building and testing

```sh
cargo build --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Most tests talk to a real server, so start one first and wait until it is
seeded:

```sh
sudo esdm-server
cargo run -p esdm-tool -- wait-until-seeded 60
cargo test --workspace
```

Further test sets, each behind a feature of `rand-esdm`:

- `privileged_tests`: the privileged interface (adding entropy, self tests,
  notifications). Needs root:
  `sudo -E cargo test -p rand-esdm --features privileged_tests`
- `sandbox_tests`: JSON status tests, each against its own `esdm-server` in
  unprivileged namespaces, independent of the server on the host. Needs
  `unshare` (util-linux >= 2.38) and a subordinate id range in `/etc/subuid`
  and `/etc/subgid`, see
  [`status_sandbox.rs`](rand-esdm/tests/status_sandbox.rs):
  `cargo test -p rand-esdm --features sandbox_tests --test status_sandbox`

## License

MIT, see [LICENSE.MIT](LICENSE.MIT).
