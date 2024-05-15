use std::{
    borrow::Cow,
    marker::PhantomData,
};

use serde::Deserialize;

use crate::gitlab::{
    CodeQualityReportEntry,
    Severity,
};

#[derive(Clone, Debug, Deserialize)]
struct RustfmtJsonEntry<'a> {
    /// full path filename
    name: Cow<'a, str>,
    mismatches: Vec<Mismatch<'a>>,
}

#[derive(Clone, Debug, Deserialize)]
struct Mismatch<'a> {
    original_begin_line: usize,
    // original_end_line: usize,
    // expected_begin_line: usize,
    // expected_end_line: usize,
    // original: Cow<'a, str>,
    // expected: Cow<'a, str>,
    #[serde(skip)]
    _phantom: PhantomData<&'a ()>,
}

impl TryFrom<RustfmtJsonEntry<'_>> for CodeQualityReportEntry {
    type Error = ();

    fn try_from(value: RustfmtJsonEntry) -> Result<Self, Self::Error> {
        Ok(Self::new(
            "rustfmt".to_string(),
            Severity::Minor,
            "".to_string(),
            value.name.to_string(),
            value.mismatches.first().ok_or(())?.original_begin_line,
        ))
    }
}
