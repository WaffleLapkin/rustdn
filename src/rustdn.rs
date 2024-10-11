use std::{env, fs};

use crate::toolchain::ToolchainOverride;

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
pub(super) fn main(mut args: env::Args) {
    if args.next().as_deref() == Some("toolchain") {
        toolchain(args);
    } else {
        unimplemented!()
    }
}

fn toolchain(mut args: env::Args) {
    if args.next().as_deref() == Some("list") {
        let toolchains_dir = dirs::home_dir().unwrap().join(".rustdn/toolchains");

        let dir = fs::read_dir(&toolchains_dir).unwrap();
        let mut toolchains = Vec::new();

        for res in dir {
            match res {
                Ok(entry) => {
                    let name = entry.file_name();
                    if let Some(toolchain) = ToolchainOverride::from_key(name) {
                        toolchains.push(toolchain);
                    }
                    // FIXME: log if there is a non-toolchain file?
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
    } else {
        unimplemented!()
    }
}
