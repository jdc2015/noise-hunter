//! What a read is made of.
//!
//! One read of an unbinned file is expected to look like
//! `[27-base barcode][index1][insert]`, with a run of `G`s padding the end when
//! the molecule was short. [`anatomise`] reports where each part sits, in
//! 1-based offsets, so the profile and the pattern search can ask questions like
//! "does this read carry its index" without each re-deriving the layout.
//!
//! It works on the same cleaned sequence the counting job uses, so a line the
//! counter would skip is a line with no anatomy.

use crate::count::clean_sequence;
use crate::tables::SampleIndices;

/// A trailing `G` run at least this long is padding, not sequence: the molecule
/// ended and the sequencer filled the rest of the read. Shorter runs are left
/// alone, because a real read can end in a handful of `G`s.
pub const POLY_G_MIN_RUN: usize = 8;

/// Where the pieces of one read are, as 1-based offsets. `None` means absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadAnatomy {
    /// First occurrence of the allele-locus barcode (the 27-mer counted as `readA`).
    pub barcode: Option<usize>,
    /// First occurrence of this sample's index1.
    pub index1: Option<usize>,
    /// Whether index1 sits in its usual slot, base 28 — the position the sample
    /// set here overwhelmingly uses. Any other position is still a hit.
    pub index1_expected: bool,
    /// First occurrence of this sample's index2.
    pub index2: Option<usize>,
    /// First base of the terminal `G` run, when it is long enough to be padding.
    pub poly_g_start: Option<usize>,
    /// Bases before the padding starts — the length of real sequence.
    pub effective_len: usize,
}

impl ReadAnatomy {
    /// True when either index was found anywhere in the read.
    pub fn has_index(&self) -> bool {
        self.index1.is_some() || self.index2.is_some()
    }

    /// True when the barcode was found, which is what the binner counts.
    pub fn has_barcode(&self) -> bool {
        self.barcode.is_some()
    }
}

/// Reports the anatomy of one read, or `None` when the line is not a usable
/// sequence at all (empty after cleaning, or containing anything but `A/C/G/T`).
///
/// `barcode` is the allele-locus 27-mer to look for; it belongs to the variant,
/// not to the program, so it is supplied rather than assumed.
pub fn anatomise(read: &str, indices: &SampleIndices, barcode: &str) -> Option<ReadAnatomy> {
    let cleaned = clean_sequence(read)?;
    let length = cleaned.len();

    let offset = |needle: &str| -> Option<usize> {
        if needle.is_empty() {
            None
        } else {
            cleaned.find(needle).map(|at| at + 1)
        }
    };

    let index1 = offset(&indices.index1);
    let trailing_g = cleaned.chars().rev().take_while(|base| *base == 'G').count();
    let poly_g_start = if trailing_g >= POLY_G_MIN_RUN {
        Some(length - trailing_g + 1)
    } else {
        None
    };

    Some(ReadAnatomy {
        barcode: offset(barcode),
        index1,
        index1_expected: index1 == Some(28),
        index2: offset(&indices.index2),
        poly_g_start,
        effective_len: poly_g_start.map_or(length, |start| start - 1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";

    fn indices() -> SampleIndices {
        SampleIndices {
            name: "sample1".to_string(),
            index1: "ATATGCGC".to_string(),
            index2: "CTGATCGT".to_string(),
        }
    }

    /// Barcode at 1, index1 at 28: the shape the sample set mostly has.
    #[test]
    fn finds_the_barcode_and_an_index_in_its_slot() {
        let read = format!("{BARCODE}ATATGCGCATCTGGGGTT");
        let anatomy = anatomise(&read, &indices(), BARCODE).expect("a valid read");

        assert_eq!(anatomy.barcode, Some(1));
        assert_eq!(anatomy.index1, Some(28));
        assert!(anatomy.index1_expected, "base 28 is the usual slot");
        assert_eq!(anatomy.index2, None, "index2 is far downstream");
        assert_eq!(anatomy.poly_g_start, None, "a short G run is real sequence");
        assert_eq!(anatomy.effective_len, read.len());
        assert!(anatomy.has_barcode() && anatomy.has_index());
    }

    /// The same index in a different place still counts as a hit, and is not
    /// "expected".
    #[test]
    fn an_index_outside_its_slot_is_still_found() {
        let read = format!("{BARCODE}GGATATGCGCATCT");
        let anatomy = anatomise(&read, &indices(), BARCODE).expect("a valid read");
        assert_eq!(anatomy.index1, Some(30));
        assert!(!anatomy.index1_expected);
        assert!(anatomy.has_index());
    }

    #[test]
    fn trailing_g_padding_is_measured_and_excluded_from_the_length() {
        let read = format!("{BARCODE}ATATGCGCATCTGGGGGGGGGG");
        let anatomy = anatomise(&read, &indices(), BARCODE).expect("a valid read");
        let real = BARCODE.len() + "ATATGCGCATCT".len();
        assert_eq!(anatomy.poly_g_start, Some(real + 1));
        assert_eq!(anatomy.effective_len, real);
    }

    #[test]
    fn a_short_g_run_is_not_padding() {
        let read = format!("{BARCODE}ATATGCGCATCTGGGGGGG");
        let anatomy = anatomise(&read, &indices(), BARCODE).expect("a valid read");
        assert_eq!(anatomy.poly_g_start, None);
        assert_eq!(anatomy.effective_len, read.len());
    }

    #[test]
    fn trailing_ns_are_cleaned_away_before_measuring() {
        let read = format!("{BARCODE}ATATGCGCATCTNNNN");
        let anatomy = anatomise(&read, &indices(), BARCODE).expect("a valid read");
        assert_eq!(anatomy.effective_len, BARCODE.len() + "ATATGCGCATCT".len());
    }

    #[test]
    fn a_line_that_is_not_acgt_has_no_anatomy() {
        assert_eq!(anatomise("ACGTNACGT", &indices(), BARCODE), None);
        assert_eq!(anatomise("   ", &indices(), BARCODE), None);
    }

    #[test]
    fn an_absent_barcode_is_reported_as_absent() {
        let read = "GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG";
        let anatomy = anatomise(read, &indices(), BARCODE).expect("a valid read");
        assert_eq!(anatomy.barcode, None);
        assert!(!anatomy.has_barcode());
    }
}
