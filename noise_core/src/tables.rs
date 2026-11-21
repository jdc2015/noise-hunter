//! The upstream tables this program reads but does not own.
//!
//! [`SampleSheet`] is the sheet `bcl-convertor` demultiplexes with: one row per
//! sample, naming its two 8-base indices. The probe table that `fastq-binner`
//! counts with arrives later, with the file itself; nothing here guesses at its
//! columns.
//!
//! Both are supplied at runtime rather than compiled in — the indices belong to a
//! sequencing run, not to the program. Parsing is strict about anything that
//! would silently corrupt a sample name (duplicate names, the wrong number of
//! fields, an index that is not eight `A/C/G/T` bases) and tolerant about what a
//! spreadsheet emits: a header, CRLF line endings, blank lines.

/// One sample's pair of 8-base indices, in the order the sheet lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleIndices {
    pub name: String,
    pub index1: String,
    pub index2: String,
}

/// A parsed sample sheet, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SampleSheet {
    samples: Vec<SampleIndices>,
}

impl SampleSheet {
    /// Parses a sheet with or without a header. Fields are trimmed; indices are
    /// uppercased so they compare equal to cleaned reads.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut samples: Vec<SampleIndices> = Vec::new();
        let mut seen: Vec<String> = Vec::new();

        for (number, raw) in text.split('\n').enumerate() {
            let number = number + 1;
            let line = raw.trim_end_matches('\r');
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            if fields.len() != 3 {
                return Err(format!(
                    "sample sheet line {number}: expected 3 fields (sample,index,index2), found {}",
                    fields.len()
                ));
            }
            // A spreadsheet header is the only row whose first field is not a
            // sample name, so recognise it by name rather than by position.
            if number == 1 && fields[0].eq_ignore_ascii_case("sample_id") {
                continue;
            }
            let name = fields[0];
            if name.is_empty() {
                return Err(format!("sample sheet line {number}: empty sample name"));
            }
            if seen.iter().any(|existing| existing == name) {
                return Err(format!(
                    "sample sheet line {number}: sample '{name}' appears more than once"
                ));
            }
            seen.push(name.to_string());
            samples.push(SampleIndices {
                name: name.to_string(),
                index1: parse_index(fields[1], number, "index")?,
                index2: parse_index(fields[2], number, "index2")?,
            });
        }

        if samples.is_empty() {
            return Err("sample sheet has no samples".to_string());
        }
        Ok(Self { samples })
    }

    pub fn samples(&self) -> &[SampleIndices] {
        &self.samples
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The row for a sample name, if the sheet has one.
    pub fn get(&self, name: &str) -> Option<&SampleIndices> {
        self.samples.iter().find(|sample| sample.name == name)
    }
}

/// An index is exactly eight bases of A/C/G/T. Anything else would never match a
/// read, so it is a sheet error rather than a warning.
fn parse_index(field: &str, line: usize, column: &str) -> Result<String, String> {
    let upper = field.to_ascii_uppercase();
    if upper.len() != 8 {
        return Err(format!(
            "sample sheet line {line}: {column} '{field}' is not 8 bases"
        ));
    }
    if !upper.chars().all(|base| matches!(base, 'A' | 'C' | 'G' | 'T')) {
        return Err(format!(
            "sample sheet line {line}: {column} '{field}' has a base that is not A, C, G or T"
        ));
    }
    Ok(upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_header_and_crlf() {
        let text = "Sample_ID,index,index2\r\nsample1,ATATGCGC,CTGATCGT\r\nsample2,AGAACGAG,CGGCTAAT\r\n";
        let sheet = SampleSheet::parse(text).expect("parse");
        assert_eq!(sheet.len(), 2);
        assert_eq!(
            sheet.get("sample2"),
            Some(&SampleIndices {
                name: "sample2".to_string(),
                index1: "AGAACGAG".to_string(),
                index2: "CGGCTAAT".to_string(),
            })
        );
    }

    #[test]
    fn parses_without_a_header_and_with_blank_lines() {
        let text = "sample1,ATATGCGC,CTGATCGT\n\nsample2,AGAACGAG,CGGCTAAT\n";
        let sheet = SampleSheet::parse(text).expect("parse");
        assert_eq!(
            sheet
                .samples()
                .iter()
                .map(|sample| sample.name.as_str())
                .collect::<Vec<_>>(),
            vec!["sample1", "sample2"]
        );
    }

    #[test]
    fn uppercases_and_trims_indices() {
        let sheet = SampleSheet::parse("sample1, atatgcgc , ctgatcgt \n").expect("parse");
        let sample = sheet.get("sample1").expect("sample1");
        assert_eq!(sample.index1, "ATATGCGC");
        assert_eq!(sample.index2, "CTGATCGT");
    }

    #[test]
    fn rejects_a_duplicate_sample_name() {
        let error = SampleSheet::parse("sample1,ATATGCGC,CTGATCGT\nsample1,AGAACGAG,CGGCTAAT\n")
            .expect_err("duplicate must be rejected");
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn rejects_an_index_of_the_wrong_length() {
        let error = SampleSheet::parse("sample1,ATATGCG,CTGATCGT\n").expect_err("short index");
        assert!(error.contains("not 8 bases"), "{error}");
    }

    #[test]
    fn rejects_a_base_that_is_not_acgt() {
        let error = SampleSheet::parse("sample1,ATATGCGN,CTGATCGT\n").expect_err("bad base");
        assert!(error.contains("not A, C, G or T"), "{error}");
    }

    #[test]
    fn rejects_a_row_with_the_wrong_number_of_fields() {
        let error = SampleSheet::parse("sample1,ATATGCGC\n").expect_err("two fields");
        assert!(error.contains("expected 3 fields"), "{error}");
    }

    #[test]
    fn rejects_an_empty_sheet() {
        let error = SampleSheet::parse("\n\r\n").expect_err("empty");
        assert!(error.contains("no samples"), "{error}");
    }
}
