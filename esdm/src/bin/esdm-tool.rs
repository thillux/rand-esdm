use clap::{Parser, Subcommand};

use rand_esdm::{esdm_rng_fini, esdm_rng_init_checked, esdm_status_str};

#[derive(Debug, Subcommand)]
enum ToolCommand {
    Status,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: ToolCommand,
}

fn handle_status() {
    esdm_rng_init_checked();

    print!("{}", esdm_status_str().unwrap());

    esdm_rng_fini();
}

fn main() {
    let args = Args::parse();

    match args.command {
        ToolCommand::Status => handle_status(),
    }
}
