use std::process::ExitCode;

use patchelf::cli::{self, Command};

fn main() -> ExitCode {
    if std::env::var_os("PATCHELF_DEBUG").is_some() {
        patchelf::log::set_enabled(true);
    }
    let result = cli::parse(std::env::args_os().skip(1)).and_then(|cmd| match cmd {
        Command::Help { failed } => {
            eprint!("{}", cli::USAGE);
            Ok(if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS })
        }
        Command::Version => {
            println!("patchelf {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        Command::Run(opts) => {
            if opts.debug {
                patchelf::log::set_enabled(true);
            }
            patchelf::run(&opts).map(|()| ExitCode::SUCCESS)
        }
    });
    result.unwrap_or_else(|e| {
        eprintln!("patchelf: {e}");
        ExitCode::FAILURE
    })
}
