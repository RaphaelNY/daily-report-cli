mod cli;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

use daily_git::generate_report;

fn main() -> Result<()> {
    let request = Cli::parse().into_request()?;
    let generated = generate_report(&request)?;

    println!("{}", generated.output_path.display());
    eprintln!("polish: {}", generated.polish_state);
    Ok(())
}
