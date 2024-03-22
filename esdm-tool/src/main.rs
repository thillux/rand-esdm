use std::{io::Write, process::ExitCode, time::Duration};

use clap::{arg, Args, Parser, Subcommand};
use rand::RngCore;
use rand_esdm::{
    esdm_get_entropy_count, esdm_get_entropy_level, esdm_is_fully_seeded, esdm_rng_fini,
    esdm_rng_init, esdm_rng_init_checked, esdm_status_str, EsdmRng,
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

#[derive(Debug, Subcommand)]
enum ToolCommand {
    Status,
    EntropyLevel,
    EntropyCount,
    WaitUntilSeeded(WaitUntilSeededArg),
    GetRandom(GetRandomArg),
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
        if !esdm_rng_init() {
            println!("ESDM is still not fully seeded! Retry in 1s.");
            try_counter -= 1;
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if let Some(status) = esdm_is_fully_seeded() {
            if status {
                esdm_rng_fini();
                println!("ESDM is fully seeded!");
                return ExitCode::SUCCESS;
            }
        }
        esdm_rng_fini();
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
    rng.fill_bytes(&mut buf);

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

fn main() -> ExitCode {
    let args = ToolArgs::parse();

    match args.command {
        ToolCommand::Status => handle_status(),
        ToolCommand::WaitUntilSeeded(arg) => wait_until_seeded(&arg),
        ToolCommand::GetRandom(arg) => get_random(&arg),
        ToolCommand::EntropyLevel => get_entropy_level(),
        ToolCommand::EntropyCount => get_entropy_count(),
    }
}
