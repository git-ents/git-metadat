mod cli;

use clap::Parser;
use cli::{Cli, Command};
use git_ledger::{IdStrategy, Ledger, Mutation};
use git2::Repository;
use std::io::Read;
use std::path::Path;
use std::process;

fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(&cli) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

fn open_repo(path: Option<&Path>) -> Result<Repository, git2::Error> {
    match path {
        Some(p) => Repository::open(p),
        None => Repository::open_from_env(),
    }
}

fn parse_key_value(s: &str) -> Result<(&str, &str), String> {
    let (key, value) = s
        .split_once('=')
        .ok_or_else(|| format!("invalid field format '{}': expected KEY=VALUE", s))?;
    Ok((key, value))
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repo(cli.repo.as_deref())?;

    match &cli.command {
        Command::Create {
            ref_prefix,
            id,
            content_hash,
            fields,
            message,
        } => {
            let parsed: Vec<(&str, &[u8])> = fields
                .iter()
                .map(|f| {
                    let (k, v) = parse_key_value(f)?;
                    Ok((k, v.as_bytes()))
                })
                .collect::<Result<Vec<_>, String>>()?;

            let strategy = if *content_hash {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf)?;
                IdStrategy::ContentAddressed(Box::leak(buf.into_boxed_slice()))
            } else if let Some(id) = id {
                IdStrategy::CallerProvided(id)
            } else {
                IdStrategy::Sequential
            };

            let entry = repo.create(ref_prefix, &strategy, &parsed, message)?;
            println!("{}", entry.ref_);
        }

        Command::Read { ref_name } => {
            let entry = repo.read(ref_name)?;
            for (key, value) in &entry.fields {
                let text = String::from_utf8_lossy(value);
                println!("{}\t{}", key, text);
            }
        }

        Command::Update {
            ref_name,
            fields,
            deletes,
            message,
        } => {
            let mut mutations: Vec<Mutation<'_>> = Vec::new();

            for f in fields {
                let (k, v) = parse_key_value(f)?;
                mutations.push(Mutation::Set(k, v.as_bytes()));
            }
            for d in deletes {
                mutations.push(Mutation::Delete(d));
            }

            let entry = repo.update(ref_name, &mutations, message)?;
            println!("{}", entry.ref_);
        }

        Command::List { ref_prefix } => {
            let ids = repo.list(ref_prefix)?;
            for id in &ids {
                println!("{}", id);
            }
        }
    }

    Ok(())
}
