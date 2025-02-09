use std::fs;

use clap::Subcommand;
use color_eyre::eyre;
use tracing::warn;

use crate::toolchain::ToolchainOverride;

#[derive(Debug, Subcommand, Clone)]
pub(super) enum ToolchainCmd {
    /// List available toolchains.
    List,
}

pub(super) fn toolchain(args: ToolchainCmd) -> eyre::Result<()> {
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

    Ok(())
}
