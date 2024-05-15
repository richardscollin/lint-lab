mod gitlab;
mod rustfmt;

use std::{
    self,
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::Context;
use cargo_metadata::Message;
use clap::{builder::PossibleValue, Parser};

use crate::gitlab::CodeQualityReportEntry;

#[derive(clap::Parser)]
#[command(version, about, arg_required_else_help = true)]
struct Args {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(
    Clone, Debug, serde::Deserialize, serde::Serialize, strum::Display, strum::VariantArray,
)]
#[serde(rename_all = "kebab-case")]
enum Format {
    Json,
    OpenMetrics,
}
impl clap::ValueEnum for Format {
    fn value_variants<'a>() -> &'a [Self] {
        use strum::VariantArray;
        Self::VARIANTS
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Format::OpenMetrics => PossibleValue::new("open-metrics"),
            Format::Json => PossibleValue::new("json"),
        })
    }
}

#[derive(Debug, clap::Args)]
#[command(arg_required_else_help = true)]
struct SubcommandArgs {
    /// use - for stdin
    #[arg(short, long)]
    input: String,

    /// use - for stdout
    #[arg(short, long)]
    output: String,
}
// type RustfmtArgs = SubcommandArgs;

#[derive(Debug, clap::Args)]
#[command(arg_required_else_help = true)]
struct LintsArgs {
    /// use - for stdin
    /// Example usage:
    /// cargo clippy --message-format=json -- -D clippy::pedantic | lint-lab lints -i - -o -
    #[arg(short, long)]
    input: String,

    /// use - for stdout
    #[arg(short, long)]
    output: String,
}

#[derive(Debug, clap::Args)]
#[command()]
struct StatsArgs {
    #[arg(long, default_value = "Cargo.lock")]
    lockfile: PathBuf,

    #[arg(short, long, default_value = "json")]
    format: Format,

    /// use - for stdout
    #[arg(short, long, default_value = "-")]
    output: String,
}

#[derive(Debug, clap::Subcommand)]
enum Command {
    /// Convert clippy json output to gitlab code quality report
    Lints(LintsArgs),

    // Convert rustfmt json output (nightly) to gitlab code quality report
    // Rustfmt(RustfmtArgs),
    /// Print out project statistics
    Stats(StatsArgs),
}

fn get_infile(input_filename: &Path) -> Box<dyn BufRead> {
    match input_filename {
        filename if filename.as_os_str() == "-" => Box::new(std::io::stdin().lock()),
        filename => Box::new(BufReader::new(File::open(filename).unwrap_or_else(|err| {
            panic!(
                "Error: {err}. Unable to open {}",
                filename.to_string_lossy()
            )
        }))),
    }
}

fn get_outfile(output_filename: &Path) -> Box<dyn Write> {
    match output_filename {
        filename if filename.as_os_str() == "-" => Box::new(std::io::stdout().lock()),
        filename => Box::new(BufWriter::new(File::create(filename).unwrap_or_else(
            |err| {
                panic!(
                    "Error: {err}. Unable to open {}",
                    filename.to_string_lossy()
                )
            },
        ))),
    }
}

fn gitlab_clippy(_args: &LintsArgs, input: impl BufRead, output: impl Write) -> io::Result<()> {
    let result = Message::parse_stream(input)
        .filter_map(Result::ok)
        .filter_map(|each| match each {
            Message::CompilerMessage(msg) => Some(msg.try_into().ok()?),
            _ => None,
        })
        .collect::<Vec<CodeQualityReportEntry>>();
    serde_json::to_writer_pretty(output, &result)?;

    Ok(())
}

// fn rustfmt(_args: &RustfmtArgs, _reader: impl BufRead, _writer: impl Write) -> io::Result<()> { todo!() }

// ideas:
//
// build all targets
// record binary size of each target
//
// memory usage in some releae tests
//
// llvm lines for certain functions

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Stats {
    number_of_packages: Option<usize>,
}

fn stats(args: &StatsArgs, mut out: impl Write) -> std::io::Result<()> {
    let lockfile = cargo_lock::Lockfile::load(&args.lockfile)
        .context("unable to load lockfile")
        .unwrap();
    let num_packages = lockfile.packages.len();

    match args.format {
        Format::Json => {
            let stats = Stats {
                number_of_packages: Some(num_packages),
            };
            serde_json::to_writer_pretty(&mut out, &stats)?;
            writeln!(&mut out)?;
        }
        Format::OpenMetrics => {
            let mut registry = prometheus_client::registry::Registry::default();
            let guage = prometheus_client::metrics::gauge::Gauge::<i64>::default();
            guage.set(num_packages as i64);
            registry.register("dependencies", "number of dependencies", guage);

            let mut s = String::new();
            prometheus_client::encoding::text::encode(&mut s, &registry)
                .map_err(io::Error::other)?;
            write!(&mut out, "{}", s)?;
        }
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    match args.cmd {
        Command::Lints(args) => {
            let input = get_infile(args.input.as_ref());
            let output = get_outfile(args.output.as_ref());
            gitlab_clippy(&args, input, output).unwrap()
        }
        /*
        Command::Rustfmt(args) => {
            let input = get_infile(args.input.as_ref());
            let output = get_outfile(args.output.as_ref());
            rustfmt(&args, input, output).unwrap()
        }
        */
        Command::Stats(args) => {
            let output = get_outfile(args.output.as_ref());
            stats(&args, output).unwrap()
        }
    }
}
