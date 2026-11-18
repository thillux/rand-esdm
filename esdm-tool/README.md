# esdm-tool

[![crates.io](https://img.shields.io/crates/v/esdm-tool.svg)](https://crates.io/crates/esdm-tool)

Command line tool to query and control a running
[ESDM](https://www.chronox.de/esdm/) random number server, built on
[`rand-esdm`](https://crates.io/crates/rand-esdm).

```sh
cargo install esdm-tool
```

It needs the ESDM client libraries at build time, see the
[`rand-esdm` README](https://github.com/thillux/rand-esdm/tree/master/rand-esdm#requirements).

## Commands

Status:

| Command | Output |
| --- | --- |
| `status` | human readable status report |
| `status-json` | status as JSON document |
| `drng-status <NODE\|pr>` | JSON status of the DRNG serving a node, or of the prediction resistant DRNG |
| `jent-status` | status of the Jitter RNG entropy source |
| `is-fully-seeded` | whether the ESDM is fully seeded (also as exit code) |
| `entropy-level` | entropy level of the ESDM in bits |
| `entropy-count` | entropy count of the auxiliary pool in bits |
| `wait-until-seeded [TRIES]` | waits up to `TRIES` seconds (default 100) until the ESDM is fully seeded |

Random numbers:

| Command | Output |
| --- | --- |
| `get-random [-H] [-P] <SIZE>` | `SIZE` random bytes on stdout, `-H` hex encoded, `-P` from the prediction resistant DRNG |

Privileged, run as root:

| Command | Effect |
| --- | --- |
| `write-to-aux-pool [ENT_BITS]` | adds stdin (up to 1 MiB) to the auxiliary pool, credited with `ENT_BITS` bits of entropy (default 0) |
| `seed-from-os` | adds 64 bytes from the OS RNG, credited with full entropy |
| `crng-reseed` | reseeds the DRNGs |
| `selftest` | runs the self tests (also as exit code) |
| `wait-until-seeding-needed [TIMEOUT_SECS]` | waits until the ESDM asks for entropy |
| `reseed-from-os` | loop: waits until the ESDM asks for entropy, then runs `seed-from-os` |

Testing:

| Command | Effect |
| --- | --- |
| `speed` | throughput of both RNG modes for request sizes from 1 byte to 2 KiB |
| `stress-multi-threading`, `stress-multi-process`, `stress-delay` | load tests of the server |

## Examples

```sh
esdm-tool wait-until-seeded 60 && esdm-tool get-random -H 32
esdm-tool status-json | jq .fully_seeded
head -c 64 /dev/hwrng | sudo esdm-tool write-to-aux-pool 256
```

## License

MIT
