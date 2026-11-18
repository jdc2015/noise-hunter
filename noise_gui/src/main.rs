//! Desktop front end for noise-hunter. Same two jobs as the TUI, driven with a
//! mouse instead of keys, plus sortable tables, charts and parallel scanning.
//! Output files are identical to the TUI's.

use std::env;

use eframe::NativeOptions;
use noise_gui::app;

fn main() -> Result<(), eframe::Error> {
    let cli = cli_from_args();
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1220.0, 780.0])
            .with_min_inner_size([880.0, 520.0])
            .with_title(app::APP_TITLE),
        renderer: cli.renderer,
        ..Default::default()
    };
    let folder = cli.folder;

    eframe::run_native(
        app::APP_TITLE,
        options,
        Box::new(move |cc| Ok(Box::new(app::NoiseApp::new(cc, folder.clone())))),
    )
}

/// What the command line asked for.
struct Cli {
    /// Folder to open on start-up; the working directory if it isn't given,
    /// which is what the TUI does.
    folder: Option<std::path::PathBuf>,
    renderer: eframe::Renderer,
}

/// `noise_gui [--folder <path>] [--renderer wgpu|glow]`.
///
/// ```text
/// cargo run --release -p noise_gui -- --folder ../00-raw-unbinned_reads
/// cargo run --release -p noise_gui -- --renderer glow
/// ```
fn cli_from_args() -> Cli {
    let mut cli = Cli {
        folder: None,
        renderer: eframe::Renderer::Wgpu,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--folder" => match args.next() {
                Some(path) => cli.folder = Some(std::path::PathBuf::from(path)),
                None => {
                    eprintln!("--folder needs a path.");
                    break;
                }
            },
            "--renderer" => match args.next().as_deref() {
                Some("glow") => cli.renderer = eframe::Renderer::Glow,
                Some("wgpu") => cli.renderer = eframe::Renderer::Wgpu,
                Some(other) => {
                    eprintln!("Unknown renderer '{other}' — expected 'wgpu' or 'glow'.");
                    break;
                }
                None => {
                    eprintln!("--renderer needs 'wgpu' or 'glow'.");
                    break;
                }
            },
            "--help" | "-h" => {
                println!(
                    "usage: noise_gui [--folder <samples folder>] [--renderer wgpu|glow]\n\
                     Counts duplicate sequences and searches reads for a sequence.\n\
                     Output goes in noise_results/ beside the samples, exactly as the TUI writes it."
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("Unknown option '{other}'.");
                break;
            }
        }
    }
    cli
}
