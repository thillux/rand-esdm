//! Provides entropy whenever the ESDM asks for it, the job of an entropy
//! daemon backed by e.g. a hardware RNG.
//!
//! Needs a running `esdm-server` and root privileges:
//!
//! ```sh
//! cargo build --example feed_entropy
//! sudo target/debug/examples/feed_entropy
//! ```
//!
//! As a stand-in for a real entropy source, this example reads the random
//! number generator of the operating system.

use std::io::{Error, ErrorKind};
use std::time::Duration;

use rand_esdm::{EsdmConnection, EsdmNotification, EsdmPrivConnection};
use zeroize::Zeroizing;

/// how often to wait for the ESDM before exiting
const ROUNDS: usize = 3;
/// how long to wait for the ESDM to ask for entropy
const TIMEOUT: Duration = Duration::from_secs(10);
/// bytes added per request
const CHUNK: usize = 32;

fn main() -> std::io::Result<()> {
    let conn = EsdmConnection::try_new()?;
    // adding entropy is privileged
    let priv_conn = EsdmPrivConnection::try_new()?;
    let mut notification = EsdmNotification::try_new()?;

    println!(
        "waiting for the ESDM to drop below {} bits",
        conn.write_wakeup_thresh()?
    );

    for _ in 0..ROUNDS {
        match notification.wait_for_entropy_needed_timeout(TIMEOUT) {
            Ok(()) => {
                let mut entropy = Zeroizing::new([0u8; CHUNK]);
                read_entropy_source(&mut *entropy)?;
                // credit only the entropy the source really delivers, the OS
                // RNG output carries full entropy
                let bits = u32::try_from(CHUNK * 8).expect("small constant");
                priv_conn.add_entropy(&*entropy, bits)?;
                println!(
                    "ESDM asked for entropy, added {bits} bits, auxiliary pool: {} bits",
                    conn.entropy_count()?
                );
            }
            Err(e) if e.kind() == ErrorKind::TimedOut => {
                println!("ESDM did not ask for entropy within {TIMEOUT:?}");
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// stand-in for a real entropy source
fn read_entropy_source(buf: &mut [u8]) -> std::io::Result<()> {
    getrandom::fill(buf).map_err(Error::other)
}
