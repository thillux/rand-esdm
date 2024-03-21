use std::{io::Write, process::ExitCode, time::Duration};

use clap::{arg, Args, Parser, Subcommand};
use rand::RngCore;
use rand_esdm::{esdm_rng_fini, esdm_rng_init, esdm_rng_init_checked, esdm_status_str, EsdmRng};

#[derive(Debug, Args)]
struct GetRandomArg {
    #[arg(required = true)]
    size: usize,

    #[arg(short='H', long, action)]
    hex: bool,

    #[arg(short='P', long, action)]
    pr: bool,
}

#[derive(Debug, Subcommand)]
enum ToolCommand {
    Status,
    WaitUntilSeeded,
    GetRandom(GetRandomArg),
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct ToolArgs {
    #[clap(subcommand)]
    command: ToolCommand,
}

fn handle_status() -> ExitCode {
    esdm_rng_init_checked();

    print!("{}", esdm_status_str().unwrap());

    esdm_rng_fini();

    ExitCode::SUCCESS
}

fn is_fully_seeded() -> bool {
    if !esdm_rng_init() {
        return false;
    }

    let mut fully_seeded = true;

    if let Ok(status) = esdm_status_str() {
        if status.contains("ESDM fully seeded: true") {
            println!("ESDM is fully seeded!");
        } else {
            println!("ESDM is still not fully seeded!");
            fully_seeded = false;
        }
    } else {
        fully_seeded = false;
    }

    esdm_rng_fini();

    fully_seeded
}

fn wait_until_seeded() -> ExitCode {
    let mut try_counter = 100;

    while try_counter > 0 {
        if is_fully_seeded() {
            return ExitCode::SUCCESS;
        }
        try_counter -= 1;
        std::thread::sleep(Duration::from_secs(1));
    }

    ExitCode::FAILURE
}

fn get_random(arg: GetRandomArg) -> ExitCode {
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

    return ExitCode::SUCCESS;
}

fn main() -> ExitCode {
    let args = ToolArgs::parse();

    match args.command {
        ToolCommand::Status => handle_status(),
        ToolCommand::WaitUntilSeeded => wait_until_seeded(),
        ToolCommand::GetRandom(arg) => get_random(arg),
    }
}
