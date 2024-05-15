use std::hash::Hasher;

use cargo_metadata::{
    diagnostic::DiagnosticLevel,
    CompilerMessage,
};
use serde::{
    Deserialize,
    Serialize,
};

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
        lineno: usize,
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
                lines: Lines { begin: lineno },
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
