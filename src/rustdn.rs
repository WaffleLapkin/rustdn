use std::{env, fs};

use clap::{Parser, Subcommand};
use tracing::warn;

use crate::toolchain::ToolchainOverride;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Rust toolchain manager via nix",
    long_about = "`rustdn` is a rust toolchain manager which uses `nix` to build/download the toolchains."
)]
#[command(version = version())]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand, Clone)]
enum Cmd {
    /// Commands for managing "installed" toolchains.
    #[command(subcommand)]
    Toolchain(ToolchainCmd),
}

#[derive(Debug, Subcommand, Clone)]
enum ToolchainCmd {
    /// List available toolchains.
    List,
}

/// `rustdn` command entry point.
///
/// This provides meta (?) commands to manage toolchains, like `rustdn shell 1.87`.
///
/// **Completely unimplemented :thumbs_up:**
///
/// FIXME: (sub) commands that I'd like to have (most are shamelessly stollen from `rustup`)
/// - `help`/`--help`/`-h` - self explanatory
/// - `version`/`--version` - self explanatory
/// - `show` - show a toolchain that would be chosen by `rustdn`
/// - `which` - display what binary would be run
/// - `run` - run a command in the toolchain environment
/// - `shell` - creates a shell with an appropriate toolchain.
///   - By default it should probably disable proxies, i.e.
///     ```shell
///     ; rustdn shell stable
///     ; rustc +nightly
///     error: couldn't read +nigthly: No such file or directory (os error 2)
///
///     error: aborting due to 1 previous error
///     ```
///   - But there should be a flag to keep proxies
/// - `doc` - Open the documentation for the current toolchain
/// - `list` - list "installed" toolchains
///   - Is this even feasible?
/// - A command to remove a toolchain from the nix cache?
/// - `check` - check for updates
///
pub(super) fn main(args: env::ArgsOs) {
    let args = Args::parse_from([std::ffi::OsString::from("rustdn")].into_iter().chain(args));

    match args.cmd {
        Cmd::Toolchain(sub) => {
            toolchain(sub);
        }
    }
}

fn version() -> String {
    // Fetch VCS info from the build script
    // (you can override these by setting them during build)
    const COMMIT_ABBR: &str = env!("VCS_COMMIT_ABBR");
    const COMMIT_FULL: &str = env!("VCS_COMMIT_FULL");
    const COMMIT_DATE: &str = env!("VCS_COMMIT_DATE");

    const VERSION: &str = env!("CARGO_PKG_VERSION");

    // FIXME: print full commit when --verbose is used
    //
    // clap thinks that -V/--version should be the normal/verbose version
    // strings... which i disagree with.
    //
    // ideally i'd use some kind of other arg parsing library, but i haven't
    // seen/made such a library, with the same level of features (nice --help,
    // suggestions, completions), so ugh :c
    _ = COMMIT_FULL;

    format!("{VERSION} ({COMMIT_ABBR} {COMMIT_DATE})")
}

fn toolchain(args: ToolchainCmd) {
    match args {
        ToolchainCmd::List {} => {
            let toolchains_dir = dirs::home_dir().unwrap().join(".rustdn/toolchains");

            let dir = fs::read_dir(&toolchains_dir).unwrap();
            let mut toolchains = Vec::new();

            for res in dir {
                match res {
                    Ok(entry) => {
                        let name = entry.file_name();
                        if let Some(toolchain) = ToolchainOverride::from_key(name) {
                            toolchains.push(toolchain);
                        } else {
                            warn!("non-toolchain file: {}", entry.path().display());
                        }
                    }
                    Err(err) => eprintln!(
                        "error while reading `{}` directory: {err}",
                        toolchains_dir.display()
                    ),
                }
            }

            for toolchain in toolchains {
                // FIXME: figure out the actual toolchain versions, somehow
                match toolchain {
                    ToolchainOverride::File(p) => println!("{} (???)", p.display()),
                    ToolchainOverride::Version {
                        channel,
                        version: Some(version),
                    } => println!("{channel}-{version}"),
                    ToolchainOverride::Version {
                        channel,
                        version: None,
                    } => println!("{channel} (???)"),
                    ToolchainOverride::None => println!("default (???)"),
                };
            }
        }
    }
}
