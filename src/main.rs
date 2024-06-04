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
type RustfmtArgs = SubcommandArgs;

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
    Rustfmt(RustfmtArgs),

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
    let result: Vec<CodeQualityReportEntry> = Message::parse_stream(input)
        .filter_map(Result::ok)
        .filter_map(|each| match each {
            Message::CompilerMessage(msg) => Some(msg.try_into().ok()?),
            _ => None,
        })
        .collect();
    serde_json::to_writer_pretty(output, &result)?;

    Ok(())
}

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
            gitlab_clippy(&args, input, output).unwrap();
        }
        Command::Rustfmt(args) => {
            let input = get_infile(args.input.as_ref());
            let output = get_outfile(args.output.as_ref());
            rustfmt::rustfmt(&args, input, output).unwrap()
        }
        Command::Stats(args) => {
            let output = get_outfile(args.output.as_ref());
            stats(&args, output).unwrap();
        }
    }
}

mod gitlab {

    use std::hash::Hasher;

    use cargo_metadata::{diagnostic::DiagnosticLevel, CompilerMessage};
    use serde::{Deserialize, Serialize};

    /// <https://docs.gitlab.com/ee/ci/testing/code_quality.html#implement-a-custom-tool>
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct CodeQualityReportEntry {
        description: String,
        check_name: String,
        fingerprint: String,
        severity: Severity,
        location: Location,
    }

    impl CodeQualityReportEntry {
        pub fn new(
            check_name: String,
            severity: Severity,
            description: String,
            filename: String,
            line_number: usize,
        ) -> Self {
            let fingerprint = {
                #[allow(deprecated)]
                let mut hasher = std::hash::SipHasher::new();
                hasher.write(filename.as_bytes());
                hasher.write_u8(0xff);
                hasher.write(description.as_bytes());
                format!("{:x}", hasher.finish())
            };

            Self {
                description,
                check_name,
                fingerprint,
                severity,
                location: Location {
                    path: filename,
                    lines: Lines { begin: line_number },
                },
            }
        }
    }

    #[derive(Copy, Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Severity {
        Info,
        Minor,
        Major,
        Critical,
        Blocker,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct Location {
        path: String,
        lines: Lines,
    }

    #[derive(Clone, Debug, Deserialize, Serialize)]
    struct Lines {
        begin: usize,
    }

    impl TryFrom<CompilerMessage> for CodeQualityReportEntry {
        type Error = ();

        fn try_from(value: CompilerMessage) -> Result<Self, Self::Error> {
            let diagnostic = value.message;
            let description = diagnostic.message;

            let span = diagnostic.spans.first().ok_or(())?.to_owned();
            let path = span.file_name;
            let begin = span.line_start;
            let span_text = span
                .text
                .iter()
                .map(|line| line.text.trim())
                .collect::<String>();

            Ok(Self::new(
                diagnostic
                    .code
                    .map(|dc| dc.code)
                    .unwrap_or(String::from("unknown")),
                diagnostic.level.try_into()?,
                format!("{description}. {span_text}"),
                path,
                begin,
            ))
        }
    }

    impl TryFrom<DiagnosticLevel> for Severity {
        type Error = ();

        fn try_from(value: DiagnosticLevel) -> Result<Self, Self::Error> {
            Ok(match value {
                DiagnosticLevel::Note | DiagnosticLevel::Help => Self::Info,
                DiagnosticLevel::Error => Self::Major,
                DiagnosticLevel::Warning => Self::Minor,
                DiagnosticLevel::Ice | DiagnosticLevel::FailureNote => return Err(()),
                _ => return Err(()),
            })
        }
    }
}

mod rustfmt {

    use std::borrow::Cow;

    use serde::Deserialize;

    use super::*;
    use crate::gitlab::{CodeQualityReportEntry, Severity};

    #[derive(Clone, Debug, Deserialize)]
    pub struct RustfmtJsonEntry<'a> {
        /// full path filename
        name: Cow<'a, str>,
        mismatches: Vec<Mismatch<'a>>,
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct Mismatch<'a> {
        original_begin_line: usize,
        // original_end_line: usize,
        // expected_begin_line: usize,
        // expected_end_line: usize,
        original: Cow<'a, str>,
        expected: Cow<'a, str>,
    }

    impl From<RustfmtJsonEntry<'_>> for Vec<CodeQualityReportEntry> {
        fn from(value: RustfmtJsonEntry) -> Self {
            fn diff(original: &str, expected: &str) -> String {
                let mut byte_idx = None;
                for (i, (c1, c2)) in std::iter::zip(original.chars(), expected.chars()).enumerate()
                {
                    if c1 != c2 {
                        byte_idx = Some(i);
                        break;
                    }
                }

                format!(
                    "Difference at byte: {}.\noriginal: {original}. expected: {expected}",
                    byte_idx.unwrap()
                )
            }

            value
                .mismatches
                .into_iter()
                .map(|e| {
                    let description = diff(&e.original, &e.expected);
                    CodeQualityReportEntry::new(
                        "rustfmt".to_string(),
                        Severity::Minor,
                        description,
                        value.name.to_string(),
                        e.original_begin_line,
                    )
                })
                .collect()
        }
    }

    pub fn rustfmt(_args: &RustfmtArgs, input: impl BufRead, output: impl Write) -> io::Result<()> {
        let result: Vec<_> = Message::parse_stream(input)
            .filter_map(Result::ok)
            .flat_map(|each| match each {
                Message::TextLine(text) => {
                    serde_json::from_str::<Vec<RustfmtJsonEntry>>(&text).unwrap_or_default()
                }
                _ => vec![],
            })
            .flat_map(Vec::<CodeQualityReportEntry>::from)
            .collect();

        serde_json::to_writer_pretty(output, &result)?;

        Ok(())
    }
}
