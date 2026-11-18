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
    esdm_rng_init_checked();

    let write_wakup_thresh = esdm_write_wakeup_thresh().unwrap();
    assert_ne!(write_wakup_thresh, 0);

    println!("write wakeup thresh: {write_wakup_thresh}");

    esdm_rng_fini();
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
fn test_fully_seeded_mode() {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

    for _ in 1..1000 {
        let random_num: u64 = rng.try_next_u64().unwrap();
        println!("Random Number: {random_num:?}");
    }
}

#[test]
fn test_status_json() {
    esdm_rng_init_checked();

    let status: serde_json::Value = serde_json::from_str(&esdm_status_json_str().unwrap()).unwrap();
    assert!(status.get("fully_seeded").unwrap().is_boolean());
    assert!(status.get("entropy_level_bits").unwrap().is_u64());

    let drng: serde_json::Value =
        serde_json::from_str(&esdm_drng_status_pr_json_str().unwrap()).unwrap();
    assert!(drng.is_object());

    esdm_rng_fini();

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
    esdm_rng_init_checked();

    for _ in 0..100 {
        let status = esdm_status_str().unwrap();
        println!("{status}");
    }

    esdm_rng_fini();
}

// need to be root to run this test
#[test]
#[cfg(feature = "privileged_tests")]
fn test_privileged_interface() {
    // also need unprivileged interface for random bytes
    esdm_rng_init_checked();
    esdm_rng_init_priv_checked();

    esdm_clear_pool().unwrap();
    assert_eq!(esdm_get_entropy_count().unwrap(), 0);
    esdm_add_to_entropy_count(64 * 8).unwrap();
    esdm_crng_reseed().unwrap();

    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);

    // don't do this in production: circular seeding
    let mut buf: [u8; 32] = [42; 32];
    rng.try_fill_bytes(&mut buf).unwrap();
    esdm_clear_pool().unwrap();
    esdm_add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap()).unwrap();
    assert!(esdm_get_entropy_count().unwrap() >= 32 * 8);

    let (result, passed) = esdm_selftest().unwrap();
    println!("selftest: {result:?}");
    assert!(passed);
    assert_eq!(result.crypto_state, EsdmSelftestState::Passed);
}

#[test]
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
        Ok(cnt) => println!("ESDM needs entropy, count: {cnt}"),
    }
}

#[test]
fn test_add_entropy_rejects_overcrediting() {
    // checked before talking to the server, so no privileges are needed
    let err = esdm_add_entropy(&[0u8; 4], 33).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    let err = esdm_add_entropy(&[], 1).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}
