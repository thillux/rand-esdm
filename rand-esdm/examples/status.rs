//! Prints a summary of the ESDM status from its typed model.
//!
//! Needs a running `esdm-server`: `cargo run --example status`

use rand_esdm::EsdmConnection;

fn main() -> std::io::Result<()> {
    // the unprivileged interface of the ESDM
    let conn = EsdmConnection::try_new()?;
    let status = conn.status()?;

    println!(
        "ESDM {}, {}, {} bit security strength, {} DRNG instance(s)",
        status.library_version,
        status.drng_name,
        status.security_strength_bits,
        status.drng_instances
    );
    println!(
        "fully seeded: {}, entropy level: {} bits, standards: {}",
        status.fully_seeded,
        status.entropy_level_bits,
        status.standards_compliance.join(", ")
    );
    println!(
        "auxiliary pool: {} bits, write wakeup threshold: {} bits",
        conn.entropy_count()?,
        conn.write_wakeup_thresh()?
    );

    println!("\nentropy sources:");
    for es in &status.entropy_sources {
        if let Some(reason) = es.disabled_reason() {
            println!("  {:<16} disabled: {reason}", es.name);
            continue;
        }
        println!(
            "  {:<16} available: {:>4} bits, rate: {:>3} bits per 256 data bits",
            es.name,
            opt(es.available_entropy),
            opt(es.entropy_rate_per_256_data_bits)
        );
    }

    println!("\nDRNGs:");
    for drng in &status.drngs {
        println!(
            "  {:?}: fully seeded: {}, seed generation: {}, last seeded: {}",
            drng.drng_type,
            drng.fully_seeded,
            drng.seed_generation,
            drng.last_seeding_time.as_deref().unwrap_or("never")
        );
    }

    let st = &status.periodic_self_test;
    println!(
        "\nself tests: crypto {:?}, entropy sources {:?} ({} tested, {} failed), {} runs",
        st.state,
        st.entropy_source_state,
        st.entropy_sources_tested,
        st.entropy_sources_failed,
        st.completed_self_tests
    );

    Ok(())
}

fn opt(value: Option<i64>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| v.to_string())
}
