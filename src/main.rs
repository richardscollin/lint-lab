use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use cargo_metadata::Message;
use clap::{Parser, Subcommand};

use steel_wool::gitlab::CodeQualityReportEntry;

/// steel wool
#[derive(Parser)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    cmd: Command,

    #[arg(short, long)]
    input: Option<PathBuf>,

    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Convert clippy json output to gitlab code quality reports
    Clippy,
}

fn main() {
    let args = Args::parse();

    let in_file;
    let out_file;

    let input: Box<dyn BufRead> = if let Some(filename) = args.input {
        if filename.as_os_str() != "-" {
            in_file = File::open(filename.clone()).unwrap_or_else(|err| {
                panic!(
                    "Error: {err}. Unable to open {}",
                    filename.to_string_lossy()
                )
            });
            Box::new(BufReader::new(in_file))
        } else {
            Box::new(std::io::stdin().lock())
        }
    } else {
        Box::new(std::io::stdin().lock())
    };

    let output: Box<dyn Write> = if let Some(filename) = args.output {
        out_file = File::create(filename.clone()).unwrap_or_else(|err| {
            panic!(
                "Error: {err}. Unable to open {}",
                filename.to_string_lossy()
            )
        });
        Box::new(BufWriter::new(out_file))
    } else {
        Box::new(std::io::stdout().lock())
    };

    let result = Message::parse_stream(input)
        .filter_map(Result::ok)
        .filter_map(|each| match each {
            Message::CompilerMessage(msg) => Some(msg.try_into().ok()?),
            _ => None,
        })
        .collect::<Vec<CodeQualityReportEntry>>();

    serde_json::to_writer_pretty(output, &result).unwrap();
}
