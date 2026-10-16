//! OpenCAMStudio application entry point.
//!
//! The default build is headless (the tested [`cam_app::AppController`]); the
//! interactive desktop app is behind the `gui` feature:
//!
//! ```text
//! cargo run -p cam-app --features gui
//! ```

// Without this, Windows opens a console window behind the application and leaves it
// there for the whole session, because the default subsystem for a Rust binary is
// `console`. Three conditions, each load-bearing:
//
// - `windows`     — the attribute is meaningless elsewhere.
// - `feature = "gui"` — the headless build's whole output is a `println!`, so
//   detaching it from the console would send that into nothing. It must keep one.
// - `not(debug_assertions)` — a debug run keeps its console, so `cargo run` still
//   shows panics and logging on Windows. Only the shipped build is detached.
#![cfg_attr(
    all(windows, not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

mod cli;

fn main() -> std::process::ExitCode {
    let args = match cli::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}\n\n{}", cli::USAGE);
            return std::process::ExitCode::from(2);
        }
    };
    if args.help {
        println!("{}", cli::USAGE);
        return std::process::ExitCode::SUCCESS;
    }
    match launch(args.setup) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Open CAM Studio: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "gui")]
fn launch(setup: Option<std::path::PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    match setup {
        Some(path) => cam_app::gui::run_with_setup(&path),
        None => cam_app::gui::run().map_err(Into::into),
    }
}

#[cfg(not(feature = "gui"))]
fn launch(setup: Option<std::path::PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    if setup.is_some() {
        return Err("--setup requires the desktop app; build with --features gui".into());
    }
    println!(
        "Open CAM Studio {} — headless build. Launch the desktop app with:\n    \
         cargo run -p cam-app --features gui",
        cam_app::version_string()
    );
    Ok(())
}
