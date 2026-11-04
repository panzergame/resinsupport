//! ResinSupport desktop app: reads the command line, opens the window and hands it to the egui
//! front end.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
Usage: resinsupport [OPTIONS] [FILE.stl | PROJECT.rsup]...

Prepares STL models for resin printers. A PROJECT (the last one given) is opened when the
window opens, then each FILE is imported.

Options:
  -h, --help     Print this help and exit
  -V, --version  Print the version and exit
  --             Treat every later argument as a file
";

/// What the command line asks for.
#[derive(Debug, PartialEq)]
enum Cli {
    Help,
    Version,
    Open(Vec<PathBuf>),
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Cli, String> {
    let mut files = Vec::new();
    let mut options = true;
    for arg in args {
        if options {
            match arg.to_str() {
                Some("-h" | "--help") => return Ok(Cli::Help),
                Some("-V" | "--version") => return Ok(Cli::Version),
                Some("--") => {
                    options = false;
                    continue;
                }
                // A lone `-` is a file name, as in most tools.
                Some(s) if s.starts_with('-') && s != "-" => return Err(format!("unknown option `{s}`")),
                _ => {}
            }
        }
        files.push(PathBuf::from(arg));
    }
    Ok(Cli::Open(files))
}

/// The files that can be opened; the others are reported on stderr so the window still opens
/// with the rest.
fn usable(files: Vec<PathBuf>) -> Vec<PathBuf> {
    files
        .into_iter()
        .filter(|f| {
            let problem = if !f.is_file() {
                Some("no such file")
            } else if !f.extension().is_some_and(|e| e.eq_ignore_ascii_case("stl") || e.eq_ignore_ascii_case("rsup")) {
                Some("not an .stl or .rsup file")
            } else {
                None
            };
            if let Some(p) = problem {
                eprintln!("resinsupport: {}: {p}", f.display());
            }
            problem.is_none()
        })
        .collect()
}

fn main() -> ExitCode {
    // A broken Vulkan driver logs warnings on every start while wgpu falls back to another one.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,wgpu_hal=error")).init();
    let files = match parse(std::env::args_os().skip(1)) {
        Ok(Cli::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Cli::Version) => {
            println!("resinsupport {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Cli::Open(files)) => usable(files),
        Err(e) => {
            eprintln!("resinsupport: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_title("ResinSupport").with_inner_size([1280.0, 800.0]).with_drag_and_drop(true),
        // The viewport pipelines are built for this depth buffer and sample count.
        depth_buffer: resin_ui_egui::DEPTH_BITS,
        multisampling: resin_ui_egui::MSAA_SAMPLES,
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    let run = eframe::run_native(
        "ResinSupport",
        options,
        Box::new(|cc| {
            let mut app = resin_ui_egui::ResinApp::new(cc)?;
            app.open_later(files);
            Ok(Box::new(app) as Box<dyn eframe::App>)
        }),
    );
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("resinsupport: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<OsString> {
        a.iter().map(OsString::from).collect()
    }

    #[test]
    fn options_and_files() {
        assert_eq!(parse(args(&[])), Ok(Cli::Open(vec![])));
        assert_eq!(parse(args(&["a.stl", "b.STL"])), Ok(Cli::Open(vec!["a.stl".into(), "b.STL".into()])));
        assert_eq!(parse(args(&["a.stl", "--help"])), Ok(Cli::Help));
        assert_eq!(parse(args(&["-V"])), Ok(Cli::Version));
        assert!(parse(args(&["-x", "a.stl"])).is_err());
        assert_eq!(parse(args(&["--", "--help", "-"])), Ok(Cli::Open(vec!["--help".into(), "-".into()])));
    }

    #[test]
    fn missing_and_foreign_files_are_left_out() {
        let dir = std::env::temp_dir().join(format!("resinsupport-cli-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (stl, txt, project) = (dir.join("part.StL"), dir.join("notes.txt"), dir.join("plate.rsup"));
        std::fs::write(&stl, b"solid x\nendsolid x\n").unwrap();
        std::fs::write(&txt, b"").unwrap();
        std::fs::write(&project, b"").unwrap();
        assert_eq!(usable(vec![stl.clone(), txt, dir.join("gone.stl"), dir.clone(), project.clone()]), vec![stl, project]);
        std::fs::remove_dir_all(dir).ok();
    }
}
