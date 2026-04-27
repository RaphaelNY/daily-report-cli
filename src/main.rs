mod cli;

use anyhow::Result;
use clap::Parser;
use cli::{AppCommand, Cli};

use daily_git::{generate_report, run_update, UpdateState};

fn main() -> Result<()> {
    match Cli::parse().into_command()? {
        AppCommand::Generate(request) => {
            let generated = generate_report(&request)?;
            println!("{}", generated.output_path.display());
            eprintln!("polish: {}", generated.polish_state);
        }
        AppCommand::Update(options) => {
            let result = run_update(&options)?;
            match result.state {
                UpdateState::UpToDate => {
                    println!(
                        "already up to date: {} ({})",
                        result.target_version,
                        result.executable_path.display()
                    );
                }
                UpdateState::Available => {
                    println!(
                        "update available: {} -> {}",
                        result.current_version, result.target_version
                    );
                }
                UpdateState::Updated => {
                    println!(
                        "updated {} -> {} ({})",
                        result.current_version,
                        result.target_version,
                        result.executable_path.display()
                    );
                }
            }
        }
    }
    Ok(())
}
