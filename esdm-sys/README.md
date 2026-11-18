# esdm-sys

[![crates.io](https://img.shields.io/crates/v/esdm-sys.svg)](https://crates.io/crates/esdm-sys)

Raw FFI bindings to the client libraries of the
[ESDM](https://www.chronox.de/esdm/) random number server, generated with
`bindgen` at build time.

**You probably want [`rand-esdm`](https://crates.io/crates/rand-esdm)**, the
safe API on top of these bindings.

## Modules

- `esdm`: the RPC client (`esdm_rpc_client.h`): random bytes, status
  reports, the privileged entropy interface, self tests
- `esdm_aux`: the aux client (`esdm_aux_client.h`): waiting until the server
  needs entropy

Only the functions used by `rand-esdm` are bound. Their contracts, e.g.
thread safety and error codes, are documented in the ESDM headers.

## Requirements

ESDM 1.2.4 or later with its client libraries `esdm_rpc_client` and
`esdm_aux_client`, plus `libprotobuf-c`, all found via `pkg-config`, and
`clang` for `bindgen`. The build fails if a library is missing or ESDM is
older than 1.2.4.

## License

MIT
