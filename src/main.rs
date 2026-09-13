use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    let cli = nodes::cli::Cli::parse();
    match nodes::cli::run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("nodes: {error}");
            ExitCode::FAILURE
        }
    }
}
