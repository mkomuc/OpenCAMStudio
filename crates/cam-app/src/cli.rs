//! Command-line arguments for the desktop executable.

use std::{ffi::OsString, path::PathBuf};

pub const USAGE: &str = "Usage: opencamstudio [--setup <path.ocam>] [--help]

  --setup <path.ocam>  Open a saved setup/project on startup without a file dialog
  -h, --help           Show this help

Relative paths are resolved from the current working directory.";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub setup: Option<PathBuf>,
    pub help: bool,
}

pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--setup" {
            if parsed.setup.is_some() {
                return Err("--setup may only be specified once".into());
            }
            let path = args
                .next()
                .ok_or("--setup requires a path to a .ocam project")?;
            if path.is_empty() || path.as_encoded_bytes().starts_with(b"-") {
                return Err("--setup requires a path to a .ocam project".into());
            }
            parsed.setup = Some(path.into());
        } else if arg == "--help" || arg == "-h" {
            parsed.help = true;
        } else {
            return Err(format!("Unknown argument: {}", arg.to_string_lossy()));
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Args, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn default_startup_and_help() {
        assert_eq!(parse_strs(&[]).unwrap(), Args::default());
        for flag in ["--help", "-h"] {
            assert!(parse_strs(&[flag]).unwrap().help);
        }
    }

    #[test]
    fn setup_paths_preserve_spaces_and_relative_components() {
        for path in ["testfiles/test_project.ocam", "../My setups/test part.ocam"] {
            assert_eq!(
                parse_strs(&["--setup", path]).unwrap().setup,
                Some(path.into())
            );
        }
    }

    #[test]
    fn rejects_missing_paths_duplicates_and_unknown_arguments() {
        for args in [
            vec!["--setup"],
            vec!["--setup", ""],
            vec!["--setup", "--help"],
            vec!["--setup", "a.ocam", "--setup", "b.ocam"],
            vec!["--unknown"],
            vec!["unexpected.ocam"],
        ] {
            assert!(parse_strs(&args).is_err(), "accepted {args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn setup_path_need_not_be_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let path = OsString::from_vec(b"setup-\xff.ocam".to_vec());
        assert_eq!(
            parse([OsString::from("--setup"), path.clone()])
                .unwrap()
                .setup,
            Some(path.into())
        );
    }
}
