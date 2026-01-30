use clap::Parser;
use greentic_operator::cli;
use std::env;

fn main() -> anyhow::Result<()> {
    if env::var("GREENTIC_PROVIDER_CORE_ONLY").is_err() {
        // set_var is unsafe in this codebase, so wrap it accordingly.
        unsafe {
            env::set_var("GREENTIC_PROVIDER_CORE_ONLY", "false");
        }
    }
    let cli = cli::Cli::parse();
    cli.run()
}
