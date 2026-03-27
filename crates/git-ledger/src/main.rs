mod cli;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};
use git_ledger::{IdStrategy, Ledger, Mutation};
use git2::Repository;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    if let Some(dir) = parse_generate_man_flag() {
        if let Err(e) = generate_man_page(dir) {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
        return;
    }

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

fn parse_field(s: &str) -> (&str, &str) {
    match s.split_once('=') {
        Some((k, v)) => (k, v),
        None => (s, ""),
    }
}

fn parse_file_arg(s: &str) -> Result<(&str, &Path), Box<dyn std::error::Error>> {
    let (key, path) = match s.split_once('=') {
        Some((k, p)) => (k, Path::new(p)),
        None => {
            let p = Path::new(s);
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| format!("cannot derive filename from '{}'", s))?;
            (name, p)
        }
    };
    Ok((key, path))
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let repo = open_repo(cli.repo.as_deref())?;

    match &cli.command {
        Command::Create {
            ref_prefix,
            id,
            content_hash,
            fields,
            files,
            message,
            author_name,
            author_email,
        } => {
            let mut entries: Vec<(String, Vec<u8>)> = fields
                .iter()
                .map(|f| {
                    let (k, v) = parse_field(f);
                    (k.to_string(), v.as_bytes().to_vec())
                })
                .collect();

            for f in files {
                let (key, path) = parse_file_arg(f)?;
                let content = std::fs::read(path)?;
                entries.push((key.to_string(), content));
            }

            let parsed: Vec<(&str, &[u8])> = entries
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_slice()))
                .collect();

            let stdin_buf;
            let strategy = if *content_hash {
                let mut buf = Vec::new();
                std::io::stdin().read_to_end(&mut buf)?;
                stdin_buf = buf;
                IdStrategy::ContentAddressed(&stdin_buf)
            } else if let Some(id) = id {
                IdStrategy::CallerProvided(id)
            } else {
                IdStrategy::Sequential
            };

            let author_sig;
            let author = match author_name {
                Some(name) => {
                    let email = author_email.as_deref().unwrap();
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    let time = git2::Time::new(now, 0);
                    author_sig = git2::Signature::new(name, email, &time)?;
                    Some(&author_sig)
                }
                None => None,
            };

            let entry = repo.create(ref_prefix, &strategy, &parsed, message, author)?;
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
            files,
            deletes,
            message,
        } => {
            let mut file_contents: Vec<(String, Vec<u8>)> = Vec::new();
            let mut mutations: Vec<Mutation<'_>> = Vec::new();

            for f in fields {
                let (k, v) = parse_field(f);
                mutations.push(Mutation::Set(k, v.as_bytes()));
            }
            for f in files {
                let (key, path) = parse_file_arg(f)?;
                let content = std::fs::read(path)?;
                file_contents.push((key.to_string(), content));
            }
            for entry in &file_contents {
                mutations.push(Mutation::Set(&entry.0, &entry.1));
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

        Command::Log { ref_name } => {
            let oids = repo.history(ref_name)?;
            for oid in &oids {
                println!("{}", oid);
            }
        }
    }

    Ok(())
}

/// Check for `--generate-man <DIR>` before clap parses, so it doesn't
/// conflict with the required subcommand.
fn parse_generate_man_flag() -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "--generate-man")?;
    let dir = args
        .get(pos + 1)
        .map(PathBuf::from)
        .unwrap_or_else(default_man_dir);
    Some(dir)
}

fn default_man_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").expect("HOME is not set");
            PathBuf::from(home).join(".local/share")
        })
        .join("man")
}

fn generate_man_page(output_dir: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let man1_dir = output_dir.join("man1");
    std::fs::create_dir_all(&man1_dir)?;

    let cmd = Cli::command();
    let man = clap_mangen::Man::new(cmd);
    let mut buffer = Vec::new();
    man.render(&mut buffer)?;

    let man_path = man1_dir.join("git-ledger.1");
    std::fs::write(&man_path, buffer)?;

    let output_dir = output_dir.canonicalize()?;
    eprintln!("Wrote man page to {}", man_path.canonicalize()?.display());

    if !manpath_covers(&output_dir) {
        eprintln!();
        eprintln!("You may need to add this to your shell environment:");
        eprintln!();
        eprintln!("  export MANPATH=\"{}:$MANPATH\"", output_dir.display());
    }
    Ok(())
}

/// Returns `true` if `dir` is equal to, or a subdirectory of, any component
/// in the `MANPATH` environment variable.
fn manpath_covers(dir: &std::path::Path) -> bool {
    let Some(manpath) = std::env::var_os("MANPATH") else {
        return false;
    };
    for component in std::env::split_paths(&manpath) {
        let Ok(component) = component.canonicalize() else {
            continue;
        };
        if dir.starts_with(&component) {
            return true;
        }
    }
    false
}
