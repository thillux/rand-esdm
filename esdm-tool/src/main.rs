use rand_core::RngCore;
use std::{
    io::{Read, Write},
    process::{ExitCode, Stdio},
    time::Duration,
};

use clap::{Args, Parser, Subcommand, arg};
use rand_esdm::{
    EsdmRng, esdm_add_entropy, esdm_get_entropy_count, esdm_get_entropy_level,
    esdm_is_fully_seeded, esdm_rng_fini, esdm_rng_fini_priv, esdm_rng_init, esdm_rng_init_checked,
    esdm_rng_init_priv_checked, esdm_status_str,
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
    WriteToAuxPool(WriteToAuxPoolArg),
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
        };

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
    }
}
