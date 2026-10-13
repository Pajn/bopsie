//! Writes the test library to the directory given, with 30-second tracks as the Bopsie
//! integration suites expect: `bopsie-fixtures <dir>`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: bopsie-fixtures <dir>");
        return ExitCode::FAILURE;
    };
    match bopsie_fixtures::write_library(&dir, 30) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
