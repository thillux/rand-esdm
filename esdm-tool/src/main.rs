use rand_core::TryRngCore;
use std::{
    io::{Read, Write},
    process::{Child, Command, ExitCode, Stdio},
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use clap::{Args, Parser, Subcommand, arg};
use rand_esdm::{
    EsdmNotification, EsdmRng, esdm_add_entropy, esdm_crng_reseed, esdm_get_entropy_count,
    esdm_get_entropy_level, esdm_is_fully_seeded, esdm_rng_fini, esdm_rng_fini_priv, esdm_rng_init,
    esdm_rng_init_checked, esdm_rng_init_priv_checked, esdm_status_str,
};

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

#[derive(Debug, Subcommand)]
enum ToolCommand {
    IsFullySeeded,
    Status,
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

fn handle_status() -> ExitCode {
    if !esdm_rng_init() {
        println!("Cannot init ESDM connection. Exiting!");
        return ExitCode::FAILURE;
    }

    if let Ok(status) = esdm_status_str() {
        print!("{status}");
    } else {
        println!("Cannot get ESDM status string. Exiting!");
        esdm_rng_fini();
        return ExitCode::FAILURE;
    }

    esdm_rng_fini();

    ExitCode::SUCCESS
}

fn wait_until_seeded(arg: &WaitUntilSeededArg) -> ExitCode {
    let mut try_counter = arg.tries;

    while try_counter > 0 {
        /*
         * reuse, if SEGV on startup is resolved
         */
        // if let Some(status) = esdm_is_fully_seeded() {
        //     if status {
        //         println!("ESDM is fully seeded!");
        //         return ExitCode::SUCCESS;
        //     }
        // }

        match std::env::current_exe() {
            Ok(exe_path) => {
                if let Ok(status) = std::process::Command::new(exe_path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .env_clear()
                    .arg("is-fully-seeded")
                    .status()
                {
                    if status.success() {
                        println!("ESDM is fully seeded!");
                        return ExitCode::SUCCESS;
                    }
                }
            }
            Err(e) => {
                println!("failed to get current exe path: {e}");
                return ExitCode::FAILURE;
            }
        }

        println!("ESDM is still not fully seeded! Retry in 1s.");
        try_counter -= 1;
        std::thread::sleep(Duration::from_secs(1));
    }

    println!("ESDM can't be reached or is still not fully seeded, exiting!");
    ExitCode::FAILURE
}

fn get_random(arg: &GetRandomArg) -> ExitCode {
    let mut buf = vec![0u8; arg.size];
    let mut rng = if arg.pr {
        EsdmRng::new(rand_esdm::EsdmRngType::PredictionResistant)
    } else {
        EsdmRng::new(rand_esdm::EsdmRngType::FullySeeded)
    };
    rng.try_fill_bytes(&mut buf).unwrap();

    if arg.hex {
        print!("{}", hex::encode(buf));
    } else {
        std::io::stdout().write_all(&buf).unwrap();
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
    esdm_rng_init_checked();
    let cnt = esdm_get_entropy_count().unwrap();
    println!("Entropy count: {cnt}");
    esdm_rng_fini();

    ExitCode::SUCCESS
}

fn write_to_aux_pool(arg: &WriteToAuxPoolArg) -> ExitCode {
    esdm_rng_init_checked();
    esdm_rng_init_priv_checked();

    let mut exit_status = ExitCode::SUCCESS;

    let mut stdin = std::io::stdin();
    let mut buf = vec![];
    if let Ok(size) = stdin.read_to_end(&mut buf) {
        if esdm_add_entropy(&buf, u32::try_from(arg.ent_bits).unwrap()).is_err() {
            exit_status = ExitCode::FAILURE;
            eprintln!("Failed to seed ESDM, maybe root privileges missing?");
        } else {
            println!(
                "Added {size} Byte input to ESDM Auxiliary Pool, accounted with {} Bit of entropy.",
                arg.ent_bits
            );
        }
    } else {
        println!("Seeding ESDM Aux Pool failed!");
    }

    esdm_rng_fini_priv();
    esdm_rng_fini();

    exit_status
}

fn is_fully_seeded() -> ExitCode {
    if let Some(status) = esdm_is_fully_seeded() {
        if status {
            println!("ESDM is fully seeded!");
            return ExitCode::SUCCESS;
        }
    }

    println!("ESDM is not fully seeded!");
    ExitCode::FAILURE
}

fn crng_reseed() -> ExitCode {
    esdm_rng_init_priv_checked();
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
    esdm_rng_init_checked();

    let mut notifier = EsdmNotification::new();

    let ret = if notifier
        .wait_for_entropy_needed_timeout(Duration::from_secs(arg.timeout_secs))
        .is_ok()
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    };

    esdm_rng_fini();

    ret
}

fn seed_from_os() -> ExitCode {
    esdm_rng_init_priv_checked();

    let mut exit_status = ExitCode::SUCCESS;
    // get 64 Byte from OS
    let mut buf = vec![0u8; 64];
    if getrandom::fill(&mut buf).is_err() {
        esdm_rng_fini_priv();
        return ExitCode::FAILURE;
    }

    if esdm_add_entropy(&buf, u32::try_from(buf.len() * 8).unwrap()).is_err() {
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

    let rng = &mut EsdmRng::new(rand_esdm::EsdmRngType::FullySeeded);
    let _ = rng.try_next_u64().unwrap();
    println!("Got bytes on a single core! Start multi-core stress test!");

    let cores = if let Some(c) = num_threads {
        c
    } else {
        std::thread::available_parallelism().unwrap().into()
    };
    println!("Use {cores} threads");

    let (tx, rx) = mpsc::channel();

    for i in 0..cores {
        println!("Start thread {i}");
        let mut tx1 = tx.clone();
        threads.push(std::thread::spawn(move || {
            stress_one_core(&mut tx1);
        }));
    }

    for received in rx {
        println!("Got: {received}");
    }

    for t in threads {
        let _ = t.join();
    }

    ExitCode::SUCCESS
}

fn stress_one_core(tx: &mut Sender<String>) {
    let mut rng = rand_esdm::EsdmRng::new(rand_esdm::EsdmRngType::FullySeeded);
    let mut mean_duration = 0.0;
    let alpha = 0.2;
    let mut i: u64 = 0;
    //for _ in 0..10000000 {
    loop {
        let start = Instant::now();
        let rnd_number = rng.try_next_u32().unwrap();
        let duration = start.elapsed();

        if duration.as_secs_f64() > 100.0 * mean_duration {
            let _ = tx.send(format!("rnd: {rnd_number} took {duration:?}"));
        }

        mean_duration = alpha * duration.as_secs_f64() + (1.0 - alpha) * mean_duration;
        i += 1;

        if i % 20000 == 0 {
            let _ = tx.send(format!("mean duration: {duration:?}"));
        }
    }
}

fn stress_delay() -> ExitCode {
    stress_multi_threading(Some(1))
}

fn measure_speed() -> ExitCode {
    use std::time::Instant;

    let sizes = cute::c![1 << x, for x in 0..12];

    for m in ["Fully Seeded", "Prediction Resistant"] {
        let mut rng = if m == "Fully Seeded" {
            EsdmRng::new(rand_esdm::EsdmRngType::FullySeeded)
        } else {
            EsdmRng::new(rand_esdm::EsdmRngType::PredictionResistant)
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
    let mut rng = EsdmRng::new(rand_esdm::EsdmRngType::FullySeeded);
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
