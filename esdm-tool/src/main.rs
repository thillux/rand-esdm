#![forbid(unsafe_code)]

use rand_core::TryRng;
use std::{
    fs::File,
    io::{ErrorKind, Read, Write},
    os::fd::{AsFd, BorrowedFd},
    process::{Child, Command, ExitCode},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use clap::{Args, Parser, Subcommand};
use rand_esdm::{
    EsdmConnection, EsdmNotification, EsdmPrivConnection, EsdmRng, EsdmRngType,
    esdm_get_entropy_level, esdm_is_fully_seeded,
};
use zeroize::Zeroizing;

/// largest input `write-to-aux-pool` accepts, more cannot add entropy as the
/// ESDM auxiliary pool is much smaller anyway
const AUX_POOL_MAX_INPUT: u64 = 1 << 20;

/// size of the chunks `get-random` fetches and writes out
const GET_RANDOM_CHUNK: usize = 4096;

/// unbuffered handle of a standard stream
///
/// `std::io::stdin()` and `stdout()` copy everything through process-wide
/// buffers, which are never zeroized. Random bytes and entropy input go
/// through a duplicate of the file descriptor instead, so the only copies are
/// the `Zeroizing` buffers of this tool.
fn unbuffered(fd: BorrowedFd<'_>) -> std::io::Result<File> {
    Ok(File::from(fd.try_clone_to_owned()?))
}

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
fn print_status(
    fetch: impl FnOnce(&EsdmConnection) -> std::io::Result<String>,
    name: &str,
) -> ExitCode {
    let conn = match EsdmConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };

    match fetch(&conn) {
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
    }
}

fn handle_status() -> ExitCode {
    print_status(EsdmConnection::status_text, "status string")
}

fn handle_status_json() -> ExitCode {
    print_status(EsdmConnection::status_json, "status JSON")
}

fn handle_drng_status(arg: &DrngStatusArg) -> ExitCode {
    if arg.instance.eq_ignore_ascii_case("pr") {
        return print_status(EsdmConnection::drng_status_pr_json, "PR DRNG status");
    }

    let Ok(node) = arg.instance.parse::<u32>() else {
        eprintln!("DRNG instance must be a node number or \"pr\"");
        return ExitCode::FAILURE;
    };

    print_status(|conn| conn.drng_status_json(node), "DRNG status")
}

fn handle_selftest() -> ExitCode {
    let priv_conn = match EsdmPrivConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init privileged ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };

    match priv_conn.selftest() {
        Ok(result) => {
            println!("Crypto self tests: {:?}", result.crypto_state);
            println!(
                "Entropy source self tests: {:?} ({} tested, {} failed)",
                result.es_state, result.es_sources, result.es_failures
            );
            if result.all_passed() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("Self test failed: {e}. Missing root privileges?");
            ExitCode::FAILURE
        }
    }
}

fn handle_jent_status() -> ExitCode {
    print_status(EsdmConnection::jent_status_text, "jent status string")
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
    let mut hex = Zeroizing::new([0u8; 2 * GET_RANDOM_CHUNK]);
    let mut stdout = match unbuffered(std::io::stdout().as_fd()) {
        Ok(stdout) => stdout,
        Err(e) => {
            eprintln!("Cannot open stdout: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut remaining = arg.size;
    while remaining > 0 {
        let chunk = &mut buf[..remaining.min(GET_RANDOM_CHUNK)];
        if let Err(e) = rng.try_fill_bytes(chunk) {
            eprintln!("Cannot get random bytes from ESDM: {e}");
            return ExitCode::FAILURE;
        }

        let out: &[u8] = if arg.hex {
            let hex = &mut hex[..2 * chunk.len()];
            hex::encode_to_slice(&*chunk, hex).expect("hex buffer has twice the chunk size");
            hex
        } else {
            chunk
        };
        if let Err(e) = stdout.write_all(out) {
            eprintln!("Cannot write random bytes: {e}");
            return ExitCode::FAILURE;
        }

        remaining -= chunk.len();
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
    let conn = match EsdmConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };
    match conn.entropy_count() {
        Ok(cnt) => {
            println!("Entropy count: {cnt}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot get entropy count: {e}");
            ExitCode::FAILURE
        }
    }
}

fn write_to_aux_pool(arg: &WriteToAuxPoolArg) -> ExitCode {
    // Read into a buffer allocated once with room for one byte more than
    // allowed, to detect oversized input: a growing Vec would leave copies of
    // the input behind in the allocations it frees.
    let max_input = usize::try_from(AUX_POOL_MAX_INPUT).unwrap();
    let mut buf = Zeroizing::new(vec![0u8; max_input + 1]);
    let mut len = 0;
    let read = unbuffered(std::io::stdin().as_fd()).and_then(|mut stdin| {
        while len < buf.len() {
            match stdin.read(&mut buf[len..]) {
                Ok(0) => break,
                Ok(n) => len += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    });
    if let Err(e) = read {
        eprintln!("Cannot read input: {e}");
        return ExitCode::FAILURE;
    }
    if len > max_input {
        eprintln!("Input exceeds {AUX_POOL_MAX_INPUT} bytes, refusing to seed ESDM");
        return ExitCode::FAILURE;
    }
    let input = &buf[..len];

    let Ok(ent_bits) = u32::try_from(arg.ent_bits) else {
        eprintln!("Entropy count {} is out of range", arg.ent_bits);
        return ExitCode::FAILURE;
    };

    let priv_conn = match EsdmPrivConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init privileged ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };

    match priv_conn.add_entropy(input, ent_bits) {
        Ok(()) => {
            println!(
                "Added {} Byte input to ESDM Auxiliary Pool, accounted with {ent_bits} Bit of entropy.",
                input.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Failed to seed ESDM: {e}. Missing root privileges?");
            ExitCode::FAILURE
        }
    }
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
    let priv_conn = match EsdmPrivConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init privileged ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };
    let ok = priv_conn.crng_reseed().is_ok();

    if ok {
        ExitCode::SUCCESS
    } else {
        println!("CRNG reseed failed. Missing root privileges?");
        ExitCode::FAILURE
    }
}

fn wait_until_seeding_necessary(arg: &WaitUntilSeedingNecessaryArg) -> ExitCode {
    match EsdmNotification::try_new().and_then(|mut notifier| {
        notifier.wait_for_entropy_needed_timeout(Duration::from_secs(arg.timeout_secs))
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Waiting for ESDM to need entropy failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn seed_from_os() -> ExitCode {
    // get 64 Byte from OS
    let mut buf = Zeroizing::new([0u8; 64]);
    if let Err(e) = getrandom::fill(&mut *buf) {
        eprintln!("Cannot get random bytes from the OS: {e}");
        return ExitCode::FAILURE;
    }

    let priv_conn = match EsdmPrivConnection::try_new() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Cannot init privileged ESDM connection: {e}. Exiting!");
            return ExitCode::FAILURE;
        }
    };

    let mut exit_status = ExitCode::SUCCESS;

    if priv_conn
        .add_entropy(&*buf, u32::try_from(buf.len() * 8).unwrap())
        .is_err()
    {
        exit_status = ExitCode::FAILURE;
        eprintln!("Failed to seed ESDM, maybe root privileges missing?");
    } else {
        println!(
            "Added {} Byte input to ESDM Auxiliary Pool, accounted with {} Bit of entropy.",
            buf.len(),
            buf.len() * 8
        );
    }

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
