//! `noise_profile` — the headless half of noise-hunter.
//!
//! Profiles every unbinned file in a folder, writes the profile and the
//! effective-length histogram, then searches for k-mers shared by every sample
//! and writes them to their own folder. All three outputs go to the sibling
//! `noise_results/`, the same place the counting and search jobs write.
//!
//! ```text
//! noise_profile --folder counted_results --sample-sheet docs/sample_sheet.csv
//! noise_profile --folder counted_results --sample-sheet docs/sample_sheet.csv \
//!               --k 12 --exclude-barcode --exclude-g-rich 8
//! ```

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use noise_core::{
    list_directory_for_browse, profile_files, results_dir, sample_name_from_path, shared_kmers_with,
    write_lengths, write_profile, write_shared_kmers, PatternOptions, LENGTHS_FILE, PATTERNS_DIR,
    PROFILE_FILE,
};

/// The allele-locus barcode in this sample set: the sequence the README's worked
/// example counts, and the one every construct read carries. Overridable because
/// it belongs to the run, not to the program.
const DEFAULT_BARCODE: &str = "CACCAGACACGTCTGAACTCCAGTCAC";

struct Args {
    folder: PathBuf,
    sheet: PathBuf,
    samples: Vec<String>,
    barcode: Option<String>,
    k: usize,
    exclude_barcode: bool,
    exclude_g_rich: Option<usize>,
    force: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("noise_profile: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let Some(args) = parse_args()? else {
        return Ok(()); // --help
    };

    let listing = list_directory_for_browse(&args.folder)?;
    let mut files: Vec<PathBuf> = listing.read_files;
    if !args.samples.is_empty() {
        let wanted: Vec<String> = args.samples;
        files.retain(|path| wanted.contains(&sample_name_from_path(path)));
        if files.is_empty() {
            return Err(format!(
                "none of the requested samples are in '{}'",
                args.folder.display()
            ));
        }
        let missing: Vec<&String> = wanted
            .iter()
            .filter(|name| !files.iter().any(|path| &sample_name_from_path(path) == *name))
            .collect();
        if !missing.is_empty() {
            return Err(format!("not in the folder: {}", missing
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")));
        }
    }
    if files.is_empty() {
        return Err(format!(
            "no *_unbinned_reads files in '{}'",
            args.folder.display()
        ));
    }

    let sheet_text = std::fs::read_to_string(&args.sheet)
        .map_err(|error| format!("could not read '{}': {error}", args.sheet.display()))?;
    let sheet = noise_core::SampleSheet::parse(&sheet_text)?;

    let results = results_dir(&args.folder);
    guard_overwrite(&results, args.force)?;

    // 1. The profile.
    let barcode = args.barcode.clone().unwrap_or_default();
    let profiles = profile_files(&files, &sheet, &barcode)?;
    let profile_path = write_profile(&profiles, &results)?;
    let lengths_path = write_lengths(&profiles, &results)?;

    // 2. Shared patterns.
    let mut options = PatternOptions::new(args.k);
    options.exclude_barcode = args
        .exclude_barcode
        .then(|| args.barcode.clone().unwrap_or_else(|| DEFAULT_BARCODE.to_string()));
    options.exclude_g_rich = args.exclude_g_rich;
    let search = shared_kmers_with(&files, &options)?;
    let names: Vec<String> = files
        .iter()
        .map(|path| sample_name_from_path(path))
        .collect();
    let command = env::args().collect::<Vec<_>>().join(" ");
    let patterns_dir = write_shared_kmers(&search, &names, &options, &results, &command)?;

    // 3. Say what happened.
    println!(
        "profiled {} sample{} ({} compared, {} empty)",
        profiles.len(),
        if profiles.len() == 1 { "" } else { "s" },
        search.samples_compared,
        profiles.len() - search.samples_compared
    );
    println!("  {}", profile_path.display());
    println!("  {}", lengths_path.display());
    println!(
        "  {} ({} shared k-mer{} at k={}, {} excluded by {})",
        patterns_dir.display(),
        search.patterns.len(),
        if search.patterns.len() == 1 { "" } else { "s" },
        options.k,
        search.excluded,
        options.describe(),
    );
    for pattern in search.patterns.iter().take(5) {
        println!(
            "  {:<32} total {:>10}  present {:>3}/{:<3}  o/e {}",
            pattern.kmer,
            pattern.total(),
            pattern.samples_present(),
            search.samples_compared,
            pattern
                .observed_over_expected()
                .map(|ratio| format!("{ratio:.1}"))
                .unwrap_or_else(|| "n/a".to_string()),
        );
    }
    Ok(())
}

/// Refuses to replace earlier output unless asked. The GUI asks with a dialog;
/// here the flag is the whole conversation.
fn guard_overwrite(results: &Path, force: bool) -> Result<(), String> {
    if force {
        return Ok(());
    }
    let mut existing = Vec::new();
    for name in [PROFILE_FILE, LENGTHS_FILE] {
        let path = results.join(name);
        if path.exists() {
            existing.push(path);
        }
    }
    let folder = results.join(PATTERNS_DIR);
    if folder.exists() {
        existing.push(folder);
    }
    if existing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to replace {} — pass --force to overwrite:\n  {}",
        if existing.len() == 1 { "an existing output" } else { "existing outputs" },
        existing
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n  ")
    ))
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut folder = None;
    let mut sheet = None;
    let mut samples = Vec::new();
    let mut barcode = Some(DEFAULT_BARCODE.to_string());
    let mut k = 20;
    let mut exclude_barcode = false;
    let mut exclude_g_rich = None;
    let mut force = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| -> Result<String, String> {
            args.next()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--folder" => folder = Some(PathBuf::from(value("--folder")?)),
            "--sample-sheet" => sheet = Some(PathBuf::from(value("--sample-sheet")?)),
            "--samples" => {
                samples = value("--samples")?
                    .split(',')
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();
            }
            "--barcode" => barcode = Some(value("--barcode")?),
            "--no-barcode" => barcode = None,
            "--k" => {
                k = value("--k")?
                    .parse()
                    .map_err(|_| "--k needs a whole number".to_string())?
            }
            "--exclude-barcode" => exclude_barcode = true,
            "--exclude-g-rich" => {
                exclude_g_rich = Some(
                    value("--exclude-g-rich")?
                        .parse()
                        .map_err(|_| "--exclude-g-rich needs a whole number".to_string())?,
                )
            }
            "--force" => force = true,
            "--help" | "-h" => {
                println!(
                    "usage: noise_profile --folder <samples> --sample-sheet <csv> [options]\n\
                     \n\
                     Profiles unbinned reads and finds k-mers shared by every sample.\n\
                     Output goes to noise_results/ beside the samples.\n\
                     \n\
                     options:\n\
                       --samples a,b       analyse only these samples (default: all)\n\
                       --barcode <seq>     allele-locus barcode to count\n\
                       --no-barcode        do not count a barcode\n\
                       --k <n>             k-mer length for the pattern search (default 20)\n\
                       --exclude-barcode   drop shared k-mers from the barcode\n\
                       --exclude-g-rich <n> drop shared k-mers with n or more Gs\n\
                       --force             replace existing output\n"
                );
                return Ok(None);
            }
            other => return Err(format!("unknown option '{other}' (try --help)")),
        }
    }

    Ok(Some(Args {
        folder: folder.ok_or("--folder is required")?,
        sheet: sheet.ok_or("--sample-sheet is required")?,
        samples,
        barcode,
        k,
        exclude_barcode,
        exclude_g_rich,
        force,
    }))
}
