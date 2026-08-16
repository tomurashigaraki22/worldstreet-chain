use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf};
use wsc_program::{compile_rust_source, execute, ProgramPackage};

#[derive(Parser)]
#[command(name = "it", about = "Intertrain deterministic .it program tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Build {
        #[arg(long)]
        language: String,
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "program")]
        name: String,
    },
    Verify {
        package: PathBuf,
    },
    Run {
        package: PathBuf,
        #[arg(long, default_value_t = 1_000_000)]
        gas: u64,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        Command::Build {
            language,
            source,
            out,
            name,
        } => {
            if language.to_ascii_lowercase() != "rust" {
                return Err("only Rust is enabled in this release; Python and JavaScript frontends are planned".into());
            }
            let text = fs::read_to_string(source)?;
            let package = compile_rust_source(name, &text)?;
            fs::write(&out, package.encode()?)?;
            println!(
                "program_id={} code_hash={} output={}",
                package.program_id(),
                package.code_hash,
                out.display()
            );
        }
        Command::Verify { package } => {
            let p = ProgramPackage::decode(&fs::read(package)?)?;
            println!(
                "ok program_id={} language={} vm={} hash={}",
                p.program_id(),
                p.manifest.language,
                p.manifest.vm_version,
                p.code_hash
            );
        }
        Command::Run { package, gas } => {
            let p = ProgramPackage::decode(&fs::read(package)?)?;
            let (out, used) = execute(&p, gas)?;
            println!(
                "status=success return_data_hex={} gas_used={}",
                hex::encode(out),
                used
            );
        }
    }
    Ok(())
}
