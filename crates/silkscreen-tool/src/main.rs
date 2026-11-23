//! `cargo silkscreen <command>`: see [`silkscreen_tool`].

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // Run by cargo as `cargo silkscreen …`, the first argument is the
    // subcommand's own name.
    if args.first().is_some_and(|a| a == "silkscreen") {
        args.remove(0);
    }
    match silkscreen_tool::run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cargo silkscreen: {e}");
            ExitCode::FAILURE
        }
    }
}
