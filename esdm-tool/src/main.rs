#![forbid(unsafe_code)]

use rand_core::TryRng;
use std::{
    io::{Read, Write},
    process::{Child, Command, ExitCode},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use clap::{Args, Parser, Subcommand};
use rand_esdm::{
    EsdmNotification, EsdmRng, EsdmRngType, esdm_add_entropy, esdm_crng_reseed,
    esdm_drng_status_json_str, esdm_drng_status_pr_json_str, esdm_get_entropy_count,
    esdm_get_entropy_level, esdm_is_fully_seeded, esdm_jent_status_str, esdm_rng_fini,
    esdm_rng_fini_priv, esdm_rng_init, esdm_rng_init_checked, esdm_rng_init_priv, esdm_selftest,
    esdm_status_json_str, esdm_status_str,
};
use zeroize::{Zeroize, Zeroizing};

/// largest input `write-to-aux-pool` accepts, more cannot add entropy as the
/// ESDM auxiliary pool is much smaller anyway
const AUX_POOL_MAX_INPUT: u64 = 1 << 20;

/// size of the chunks `get-random` fetches and writes out
const GET_RANDOM_CHUNK: usize = 4096;

#[derive(Debug, Args)]
struct GetRandomArg {
    #[arg(required = true)]
    size: usize,

    #[arg(short = 'H', long, action)]
    hex: bool,

    #[arg(short = 'P', long, action)]
    pr: bool,
}

#[derive(Debug, Args)]
struct WaitUntilSeededArg {
    #[arg(required = false, default_value = "100")]
    tries: usize,
}

#[derive(Debug, Args)]
struct WaitUntilSeedingNecessaryArg {
    #[arg(required = false, default_value = "100")]
    timeout_secs: u64,
}

#[derive(Debug, Args)]
struct WriteToAuxPoolArg {
    #[arg(required = false, default_value = "0")]
    ent_bits: usize,
}

#[derive(Debug, Args)]
struct DrngStatusArg {
    /// node whose DRNG instance is queried, or "pr" for the prediction resistant instance
    #[arg(required = true)]
    instance: String,
}

#[derive(Debug, Subcommand)]
enum ToolCommand {
    IsFullySeeded,
    Status,
    StatusJson,
    DrngStatus(DrngStatusArg),
    Selftest,
    JentStatus,
    EntropyLevel,
    EntropyCount,
    CrngReseed,
    WriteToAuxPool(WriteToAuxPoolArg),
    WaitUntilSeeded(WaitUntilSeededArg),
    WaitUntilSeedingNeeded(WaitUntilSeedingNecessaryArg),
    GetRandom(GetRandomArg),
    SeedFromOs,
    ReseedFromOs,
    StressMultiThreading,
    StressDelay,
    StressMultiProcess,
    Speed,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct ToolArgs {
    #[clap(subcommand)]
    command: ToolCommand,
}

/// prints the status string returned by `fetch` over an unprivileged connection
fn print_status(fetch: impl FnOnce() -> std::io::Result<String>, name: &str) -> ExitCode {
    if !esdm_rng_init() {
        println!("Cannot init ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    let ret = match fetch() {
        Ok(status) => {
            print!("{status}");
            if !status.ends_with('\n') {
                println!();
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot get ESDM {name}: {e}. Exiting!");
            ExitCode::FAILURE
        }
    };

    esdm_rng_fini();

    ret
}

fn handle_status() -> ExitCode {
    print_status(esdm_status_str, "status string")
}

fn handle_status_json() -> ExitCode {
    print_status(esdm_status_json_str, "status JSON")
}

fn handle_drng_status(arg: &DrngStatusArg) -> ExitCode {
    if arg.instance.eq_ignore_ascii_case("pr") {
        return print_status(esdm_drng_status_pr_json_str, "PR DRNG status");
    }

    let Ok(node) = arg.instance.parse::<u32>() else {
        eprintln!("DRNG instance must be a node number or \"pr\"");
        return ExitCode::FAILURE;
    };

    print_status(|| esdm_drng_status_json_str(node), "DRNG status")
}

fn handle_selftest() -> ExitCode {
    if !esdm_rng_init_priv() {
        println!("Cannot init privileged ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    let ret = match esdm_selftest() {
        Ok((result, passed)) => {
            println!("Crypto self tests: {:?}", result.crypto_state);
            println!(
                "Entropy source self tests: {:?} ({} tested, {} failed)",
                result.es_state, result.es_sources, result.es_failures
            );
            if passed {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Self test failed: {e}. Missing root privileges?");
            ExitCode::FAILURE
        }
    };

    esdm_rng_fini_priv();

    ret
}

fn handle_jent_status() -> ExitCode {
    print_status(esdm_jent_status_str, "jent status string")
}

fn wait_until_seeded(arg: &WaitUntilSeededArg) -> ExitCode {
    let mut try_counter = arg.tries;

    while try_counter > 0 {
        if esdm_is_fully_seeded() == Some(true) {
            println!("ESDM is fully seeded!");
            return ExitCode::SUCCESS;
        }

        println!("ESDM is still not fully seeded! Retry in 1s.");
        try_counter -= 1;
        std::thread::sleep(Duration::from_secs(1));
    }

    println!("ESDM can't be reached or is still not fully seeded, exiting!");
    ExitCode::FAILURE
}

fn get_random(arg: &GetRandomArg) -> ExitCode {
    let rng_type = if arg.pr {
        EsdmRngType::PredictionResistant
    } else {
        EsdmRngType::FullySeeded
    };
    let mut rng = match EsdmRng::try_new(rng_type) {
        Ok(rng) => rng,
        Err(e) => {
            eprintln!("Cannot init ESDM connection: {e}");
            return ExitCode::FAILURE;
        }
    };

    // fetch and write in chunks: the requested size is not bounded and the
    // random bytes should not linger in memory longer than needed
    let mut buf = Zeroizing::new([0u8; GET_RANDOM_CHUNK]);
    let mut stdout = std::io::stdout().lock();
    let mut remaining = arg.size;
    while remaining > 0 {
        let chunk = &mut buf[..remaining.min(GET_RANDOM_CHUNK)];
        if let Err(e) = rng.try_fill_bytes(chunk) {
            eprintln!("Cannot get random bytes from ESDM: {e}");
            return ExitCode::FAILURE;
        }

        let written = if arg.hex {
            let mut hex = Zeroizing::new(hex::encode(&*chunk));
            let ret = stdout.write_all(hex.as_bytes());
            hex.zeroize();
            ret
        } else {
            stdout.write_all(chunk)
        };
        if let Err(e) = written {
            eprintln!("Cannot write random bytes: {e}");
            return ExitCode::FAILURE;
        }

        remaining -= chunk.len();
    }

    if let Err(e) = stdout.flush() {
        eprintln!("Cannot write random bytes: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn get_entropy_level() -> ExitCode {
    if let Some(entropy_level) = esdm_get_entropy_level() {
        println!("Entropy level: {entropy_level}");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn get_entropy_count() -> ExitCode {
    if !esdm_rng_init() {
        eprintln!("Cannot init ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }
    let ret = match esdm_get_entropy_count() {
        Ok(cnt) => {
            println!("Entropy count: {cnt}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot get entropy count: {e}");
            ExitCode::FAILURE
        }
    };
    esdm_rng_fini();

    ret
}

fn write_to_aux_pool(arg: &WriteToAuxPoolArg) -> ExitCode {
    // read one byte more than allowed to detect oversized input
    let mut buf = Zeroizing::new(Vec::new());
    if let Err(e) = std::io::stdin()
        .take(AUX_POOL_MAX_INPUT + 1)
        .read_to_end(&mut buf)
    {
        eprintln!("Cannot read input: {e}");
        return ExitCode::FAILURE;
    }
    if buf.len() as u64 > AUX_POOL_MAX_INPUT {
        eprintln!("Input exceeds {AUX_POOL_MAX_INPUT} bytes, refusing to seed ESDM");
        return ExitCode::FAILURE;
    }

    let Ok(ent_bits) = u32::try_from(arg.ent_bits) else {
        eprintln!("Entropy count {} is out of range", arg.ent_bits);
        return ExitCode::FAILURE;
    };

    if !esdm_rng_init_priv() {
        eprintln!("Cannot init privileged ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    let exit_status = match esdm_add_entropy(&buf, ent_bits) {
        Ok(()) => {
            println!(
                "Added {} Byte input to ESDM Auxiliary Pool, accounted with {ent_bits} Bit of entropy.",
                buf.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to seed ESDM: {e}. Missing root privileges?");
            ExitCode::FAILURE
        }
    };

    esdm_rng_fini_priv();

    exit_status
}

fn is_fully_seeded() -> ExitCode {
    if esdm_is_fully_seeded() == Some(true) {
        println!("ESDM is fully seeded!");
        return ExitCode::SUCCESS;
    }

    println!("ESDM is not fully seeded!");
    ExitCode::FAILURE
}

fn crng_reseed() -> ExitCode {
    if !esdm_rng_init_priv() {
        eprintln!("Cannot init privileged ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }
    let ok = esdm_crng_reseed().is_ok();
    esdm_rng_fini_priv();

    if ok {
        ExitCode::SUCCESS
    } else {
        println!("CRNG reseed failed. Missing root privileges?");
        ExitCode::FAILURE
    }
}

fn wait_until_seeding_necessary(arg: &WaitUntilSeedingNecessaryArg) -> ExitCode {
    if !esdm_rng_init() {
        eprintln!("Cannot init ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    let ret = match EsdmNotification::try_new().and_then(|mut notifier| {
        notifier.wait_for_entropy_needed_timeout(Duration::from_secs(arg.timeout_secs))
    }) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Waiting for ESDM to need entropy failed: {e}");
            ExitCode::FAILURE
        }
    };

    esdm_rng_fini();

    ret
}

fn seed_from_os() -> ExitCode {
    // get 64 Byte from OS
    let mut buf = Zeroizing::new([0u8; 64]);
    if let Err(e) = getrandom::fill(&mut *buf) {
        eprintln!("Cannot get random bytes from the OS: {e}");
        return ExitCode::FAILURE;
    }

    if !esdm_rng_init_priv() {
        eprintln!("Cannot init privileged ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    let mut exit_status = ExitCode::SUCCESS;

    if esdm_add_entropy(&*buf, u32::try_from(buf.len() * 8).unwrap()).is_err() {
        exit_status = ExitCode::FAILURE;
        eprintln!("Failed to seed ESDM, maybe root privileges missing?");
    } else {
        println!(
            "Added {} Byte input to ESDM Auxiliary Pool, accounted with {} Bit of entropy.",
            buf.len(),
            buf.len() * 8
        );
    }

    esdm_rng_fini_priv();

    exit_status
}

fn reseed_from_os() -> ExitCode {
    let start = Instant::now();
    for i in 0..100_000 {
        let arg = WaitUntilSeedingNecessaryArg { timeout_secs: 100 };
        wait_until_seeding_necessary(&arg);
        let elapsed = start.elapsed();
        println!(
            "Wakeup {i} after {} secs: need entropy",
            elapsed.as_secs_f64()
        );

        let _ = seed_from_os();
        let elapsed = start.elapsed();
        println!("Reseed {i} after {} secs: reseeded", elapsed.as_secs_f64());
    }

    ExitCode::SUCCESS
}

fn stress_multi_threading(num_threads: Option<usize>) -> ExitCode {
    use std::sync::mpsc;

    let mut threads = vec![];

    let rng = &mut EsdmRng::new(EsdmRngType::FullySeeded);
    let _ = rng.try_next_u64().unwrap();
    println!("Got bytes on a single core! Start multi-core stress test!");

    let cores = num_threads.unwrap_or_else(|| std::thread::available_parallelism().unwrap().into());
    println!("Use {cores} threads");

    let (tx, rx) = mpsc::channel();

    for i in 0..cores {
        println!("Start thread {i}");
        let tx = tx.clone();
        threads.push(std::thread::spawn(move || stress_one_core(&tx)));
    }

    for received in rx {
        println!("Got: {received}");
    }

    for t in threads {
        let _ = t.join();
    }

    ExitCode::SUCCESS
}

fn stress_one_core(tx: &Sender<String>) {
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
    let mut mean_duration = 0.0;
    let alpha = 0.2;
    let mut i: u64 = 0;
    loop {
        let start = Instant::now();
        let rnd_number = rng.try_next_u32().unwrap();
        let duration = start.elapsed();

        if duration.as_secs_f64() > 100.0 * mean_duration {
            let _ = tx.send(format!("rnd: {rnd_number} took {duration:?}"));
        }

        mean_duration = alpha * duration.as_secs_f64() + (1.0 - alpha) * mean_duration;
        i += 1;

        if i.is_multiple_of(20000) {
            let _ = tx.send(format!("mean duration: {duration:?}"));
        }
    }
}

fn stress_delay() -> ExitCode {
    stress_multi_threading(Some(1))
}

fn measure_speed() -> ExitCode {
    let sizes: Vec<usize> = (0..12).map(|x| 1 << x).collect();

    for m in ["Fully Seeded", "Prediction Resistant"] {
        let mut rng = if m == "Fully Seeded" {
            EsdmRng::new(EsdmRngType::FullySeeded)
        } else {
            EsdmRng::new(EsdmRngType::PredictionResistant)
        };

        let iterations = if m == "Fully Seeded" { 20000 } else { 100 };

        println!("ESDM ({m}):");
        for size in &sizes {
            let mut buf = vec![0u8; *size];

            let now = Instant::now();

            for _ in 0..iterations {
                rng.try_fill_bytes(&mut buf).unwrap();
            }

            let elapsed = now.elapsed();
            let iterations_per_sec = (iterations as f64) / elapsed.as_secs_f64();
            if m == "Fully Seeded" {
                println!(
                    "Request size: {size} | Elapsed: {elapsed:.2?} | Rate: {:.2?} MB/s | Iterations: {iterations_per_sec:.2?} 1/s",
                    (iterations * buf.len()) as f64 / elapsed.as_secs_f64() / 1000.0 / 1000.0
                );
            } else {
                println!(
                    "Request size: {size} | Elapsed: {elapsed:.2?} | Rate: {:.2?} KB/s | Iterations: {iterations_per_sec:.2?} 1/s",
                    (iterations * buf.len()) as f64 / elapsed.as_secs_f64() / 1000.0
                );

                if *size >= 128 {
                    println!(
                        "\nSkip large sizes in prediction resistant mode, as there is no new information here"
                    );
                    break;
                }
            }
        }
        println!();
    }

    ExitCode::SUCCESS
}

fn stress_multi_process() -> ExitCode {
    use std::env;

    // test if fds are leaking
    esdm_rng_init_checked();
    let mut rng = EsdmRng::new(EsdmRngType::FullySeeded);
    for _ in 0..100 {
        let r = rng.try_next_u64().unwrap();
        println!("rnd: {r}");
    }

    let cores = std::thread::available_parallelism().unwrap().into();
    println!("Use {cores} processes");

    let mut processes: Vec<Child> = vec![];

    match env::current_exe() {
        Ok(exe_path) => {
            println!("Path of this executable is: {}", exe_path.display());
            for _ in 0..cores {
                let p = Command::new(&exe_path)
                    .args(["stress-delay"])
                    .spawn()
                    .unwrap();
                processes.push(p);
            }
        }
        Err(e) => println!("failed to get current exe path: {e}"),
    }

    for c in &mut processes {
        let _ = c.wait();
    }

    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args = ToolArgs::parse();

    match args.command {
        ToolCommand::IsFullySeeded => is_fully_seeded(),
        ToolCommand::Status => handle_status(),
        ToolCommand::StatusJson => handle_status_json(),
        ToolCommand::DrngStatus(arg) => handle_drng_status(&arg),
        ToolCommand::Selftest => handle_selftest(),
        ToolCommand::JentStatus => handle_jent_status(),
        ToolCommand::WaitUntilSeeded(arg) => wait_until_seeded(&arg),
        ToolCommand::GetRandom(arg) => get_random(&arg),
        ToolCommand::EntropyLevel => get_entropy_level(),
        ToolCommand::EntropyCount => get_entropy_count(),
        ToolCommand::WriteToAuxPool(arg) => write_to_aux_pool(&arg),
        ToolCommand::CrngReseed => crng_reseed(),
        ToolCommand::WaitUntilSeedingNeeded(arg) => wait_until_seeding_necessary(&arg),
        ToolCommand::SeedFromOs => seed_from_os(),
        ToolCommand::StressDelay => stress_delay(),
        ToolCommand::StressMultiThreading => stress_multi_threading(None),
        ToolCommand::ReseedFromOs => reseed_from_os(),
        ToolCommand::Speed => measure_speed(),
        ToolCommand::StressMultiProcess => stress_multi_process(),
    }
}
