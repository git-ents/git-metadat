use clap::Parser;

#[derive(Parser)]
#[command(name = "git metadata", bin_name = "git metadata")]
#[command(author, version, about = "Manage Git repository metadata")]
pub struct Cli {}
