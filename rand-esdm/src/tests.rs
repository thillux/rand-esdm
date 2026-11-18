// these tests assume a running esdm-server on the system!
use rand_core::TryRng;

use crate::*;

#[test]
fn test_prediction_resistant_mode() {
    let mut rng = EsdmRng::new(EsdmRngType::PredictionResistant);

    for _ in 1..1000 {
        let random_num: u64 = rng.try_next_u64().unwrap();
        println!("Random Number: {random_num:?}");
    }
}

#[test]
fn test_write_wakeup_thresh() {
    let conn = EsdmConnection::new();

    let write_wakup_thresh = conn.write_wakeup_thresh().unwrap();
    assert_ne!(write_wakup_thresh, 0);

    println!("write wakeup thresh: {write_wakup_thresh}");
}

#[test]
fn test_reuse() {
    for _ in 0..1000 {
        let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
        let _ = rng.try_next_u64().unwrap();
    }
}

#[test]
fn test_multithreading() {
    let mut threads = vec![];
    let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
    let _ = rng.try_next_u64().unwrap();

    println!("Got bytes!");

    for _ in 0..10 {
        threads.push(std::thread::spawn(move || {
            for _ in 0..1000 {
                let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
                let _ = rng.try_next_u64().unwrap();
            }
        }));
    }

    for t in threads {
        let _ = t.join();
    }
}

#[test]
fn test_shared_connection() {
    // one connection serves several threads by reference
    let conn = EsdmConnection::new();
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| {
                for _ in 0..50 {
                    conn.entropy_count().unwrap();
                }
            });
        }
    });
}

#[test]
fn test_cloned_connection_outlives_original() {
    let conn = EsdmConnection::new();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            let conn = conn.clone();
            std::thread::spawn(move || conn.entropy_count())
        })
        .collect();
    // the clones keep the service alive on their own
    drop(conn);
    for t in threads {
        t.join().unwrap().unwrap();
    }

    let priv_conn = EsdmPrivConnection::new();
    let clone = priv_conn.clone();
    drop(priv_conn);
    // reaches the server: without root it fails for missing privileges, not
    // for a missing connection
    if let Err(e) = clone.clear_pool() {
        assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
    }
}

#[test]
fn test_fully_seeded_mode() {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

    for _ in 1..1000 {
        let random_num: u64 = rng.try_next_u64().unwrap();
        println!("Random Number: {random_num:?}");
    }
}

#[test]
fn test_status_json() {
    let conn = EsdmConnection::new();

    let status: serde_json::Value = serde_json::from_str(&conn.status_json().unwrap()).unwrap();
    assert!(status.get("fully_seeded").unwrap().is_boolean());
    assert!(status.get("entropy_level_bits").unwrap().is_u64());

    let drng: serde_json::Value =
        serde_json::from_str(&conn.drng_status_pr_json().unwrap()).unwrap();
    assert!(drng.is_object());

    let status = conn.status().unwrap();
    assert_ne!(status.entropy_sources.len(), 0);
    assert_eq!(
        conn.drng_status_pr().unwrap().drng_type,
        EsdmDrngType::PredictionResistance
    );
    conn.drng_status(0).unwrap();

    // a node without DRNG
    let err = conn.drng_status(u32::MAX).unwrap_err();
    assert!(err.to_string().contains("DRNG status"), "{err}");

    drop(conn);

    // set up connections of their own
    assert!(esdm_is_fully_seeded().is_some());
    assert!(esdm_get_entropy_level().is_some());
}

#[test]
fn test_large_request() {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
    let mut buf = vec![0u8; 1 << 20];
    rng.try_fill_bytes(&mut buf).unwrap();
    assert!(buf.iter().any(|&b| b != 0));
}

#[test]
fn test_status() {
    let conn = EsdmConnection::new();

    for _ in 0..100 {
        let status = conn.status_text().unwrap();
        println!("{status}");
    }
}

// need to be root to run this test
#[test]
#[cfg(feature = "privileged_tests")]
fn test_privileged_interface() {
    let conn = EsdmConnection::new();
    let priv_conn = EsdmPrivConnection::new();

    priv_conn.clear_pool().unwrap();
    assert_eq!(conn.entropy_count().unwrap(), 0);
    priv_conn.add_to_entropy_count(64 * 8).unwrap();
    priv_conn.crng_reseed().unwrap();

    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

    // don't do this in production: circular seeding
    let mut buf: [u8; 32] = [42; 32];
    rng.try_fill_bytes(&mut buf).unwrap();
    priv_conn.clear_pool().unwrap();
    priv_conn
        .add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap())
        .unwrap();
    // The credited entropy cannot be read back reliably: adding it starts a
    // reseed right away if any DRNG wants one (the prediction resistant DRNG
    // always does once used), and prediction resistant requests of parallel
    // tests drain the auxiliary pool as well.
    conn.entropy_count().unwrap();

    let result = priv_conn.selftest().unwrap();
    println!("selftest: {result:?}");
    // the known answer tests of the hash and the DRNG pass on every server,
    // the entropy source tests depend on the hardware
    assert_eq!(result.crypto_state, EsdmSelftestState::Passed);
    if result.es_failures > 0 {
        assert_eq!(result.es_state, EsdmSelftestState::Failed);
        assert!(!result.all_passed());
    }
}

// need to be root to run this test: the server creates its need-entropy
// semaphore with mode 0644, but attaching to it requires write access
#[test]
#[cfg(feature = "privileged_tests")]
fn test_notification_handles_and_timeout() {
    // several handles share the aux library, dropping one must not detach
    // the others
    let first = EsdmNotification::try_new().unwrap();
    let mut second = EsdmNotification::try_new().unwrap();
    drop(first);

    // a timeout has to be reported as such, not as success
    match second.wait_for_entropy_needed_timeout(std::time::Duration::from_millis(100)) {
        Err(e) => assert_eq!(e.kind(), std::io::ErrorKind::TimedOut),
        // the server may really need entropy right now
        Ok(()) => println!("ESDM needs entropy"),
    }
}

#[test]
fn test_privileged_input_checks() {
    // checked before talking to the server, so no privileges are needed
    let priv_conn = EsdmPrivConnection::new();
    let err = priv_conn.add_entropy(&[0u8; 4], 33).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    let err = priv_conn.add_entropy(&[], 1).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    let err = priv_conn.set_es_rate("jent\0", 0).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn test_privileged_calls_need_root() {
    // the connection itself can be set up without root
    if is_root() {
        return;
    }
    let priv_conn = EsdmPrivConnection::new();
    let err = priv_conn.clear_pool().unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
}

fn is_root() -> bool {
    // SAFETY: getuid() has no preconditions and cannot fail
    unsafe { libc::getuid() == 0 }
}

#[test]
fn test_next_u32_and_odd_sizes() {
    let mut rng = EsdmRng::try_new(EsdmRngType::PredictionResistant).unwrap();
    let _: u32 = rng.try_next_u32().unwrap();

    // nothing to fetch, must not fail
    rng.try_fill_bytes(&mut []).unwrap();

    for len in [1, 7, 31, 4097] {
        let mut buf = vec![0u8; len];
        rng.try_fill_bytes(&mut buf).unwrap();
    }
}

#[test]
fn test_nested_connections() {
    // the connection is reference counted, dropping an inner one must keep
    // the outer one working
    let outer = EsdmConnection::new();
    let inner = EsdmConnection::new();
    drop(inner);
    assert!(outer.status_json().is_ok());
}

#[test]
fn test_other_status_reports() {
    let conn = EsdmConnection::new();

    // may be empty if the server lacks the Jitter RNG entropy source
    println!("{}", conn.jent_status_text().unwrap());

    let drng: serde_json::Value = serde_json::from_str(&conn.drng_status_json(0).unwrap()).unwrap();
    assert!(drng.is_object());
}

#[test]
fn test_unprivileged_entropy_interface() {
    let conn = EsdmConnection::new();

    // works without privileges, the data is mixed in but not credited
    conn.write_data(&[0x5a; 64]).unwrap();
    let _ = conn.entropy_count().unwrap();
}
