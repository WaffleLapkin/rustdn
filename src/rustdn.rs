use std::env;

use clap::Parser;
use color_eyre::eyre;

mod shell;
mod toolchain;
mod version;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Rust toolchain manager via nix",
    long_about = "`rustdn` is a rust toolchain manager which uses `nix` to build/download the toolchains."
)]
#[command(version = version::get())]
enum Args {
    /// Commands for managing "installed" toolchains.
    #[command(subcommand)]
    Toolchain(toolchain::ToolchainCmd),
    Shell(shell::ShellCmd),
}

/// `rustdn` command entry point.
///
/// This provides meta (?) commands to manage toolchains, like `rustdn shell 1.87`.
///
/// FIXME: (sub) commands that I'd like to have (most are shamelessly stollen from `rustup`)
/// - `show` - show a toolchain that would be chosen by `rustdn`
/// - `which` - display what binary would be run
/// - ~~`run` - run a command in the toolchain environment~~ `shell` currently does what `run` was supposed to do...
/// - `doc` - Open the documentation for the current toolchain
/// - A command to remove a toolchain from the nix cache?
/// - ~~`check` - check for updates~~ Not sure about this one...
///
pub(super) fn main(args: env::ArgsOs) -> eyre::Result<()> {
    let args = Args::parse_from([std::ffi::OsString::from("rustdn")].into_iter().chain(args));

    match args {
        Args::Toolchain(args) => toolchain::toolchain(args),
        Args::Shell(args) => shell::shell(args),
    }
}
