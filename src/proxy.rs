use std::{
    env::{self, current_dir},
    ffi::OsString,
    iter,
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
};

use tracing::{debug, trace};

use crate::unstd::AnyExt as _;

/// Entry point for command proxies.
///
/// This chooses the appropriate toolchain and then runs `bin` from it with `args`[^1].
///
/// Toolchain is chosen like this:
/// 1. If the first argument in `args` starts with `+<...>`, `<...>` is the chosen toolchain
///    - Currently `<...>` must match `(stable|beta|nightly)(-.*)?` regex
/// 2. If the current directory or any of its recursive parents have a file named
///    `rust-toolchain.toml`, it is used to specify toolchain
/// 3. Otherwise a minimal stable toolchain is used
///
/// FIXME:
/// - Allow custom toolchains in `+` similarly to what `rustup` allows with `rustup toolchain link`
///   (I'm not sure where to store information about toolchains though)
/// - Allow `+x.y.z` (shorthand for stable) and `+yyyy-mm-dd` (shorthand for nightly)
/// - Allow overriding the default (again, not sure where to store it)
/// - *Maybe* support outdated `rust-toolchain` file
/// - *Maybe* support paths in `+<...>`
///   - Paths to `rust-toolchain[.toml]`?
///   - Paths to rustc checkouts?
///   - Paths have to start with `.` or `/` (to distinguish them from local names and channels)?
/// - Maybe support specifying hashes (where? `+stable@hash...?` a field in `rust-toolchain.toml`?)
///
/// [^1]: if the first argument in `args` starts with `+` it is treated as a toolchain override and
///       is not passed to the `bin`
pub(super) fn main(bin: &str, mut args: env::Args) {
    trace!("proxying {bin}");

    let toolchain_override_or_arg = args.next();
    let mut toolchain_overridden_from_args = false;

    let toolchain = 't: {
        if let Some(t) = parse_toolchain_override(toolchain_override_or_arg.as_deref()).unwrap() {
            toolchain_overridden_from_args = true;
            break 't t;
        }

        if let Some(t) = find_toolchain_file().unwrap() {
            break 't t;
        }

        ToolchainOverride::None
    };

    debug!("toolchain override is {toolchain:?}");

    let expr = format!(
        "{}{}",
        r#"{}: (import <nixpkgs> {overlays = [(import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))];}).rust-bin."#,
        match toolchain {
            ToolchainOverride::File(f) => format!(r#"fromRustupToolchainFile "{}""#, f.display()),
            ToolchainOverride::Version { channel, version } => format!(
                r#"{}."{}".default"#,
                channel.as_str(),
                version.as_deref().unwrap_or("latest")
            ),
            ToolchainOverride::None => format!("stable.latest.default"),
        }
    );

    debug!("starting nix-build");

    // FIXME: handle errors from the command
    // FIXME: we should report *something* if `nix-build` is running for longer than, say, a second.
    //        some kind of throbber would be nice, to show that *something* is happening,
    //        toolchain is being downloaded
    let toolchain_path = Command::new("nix-build")
        // Don't create `./result` symlinks.
        // N.B.: this means that the result of the build does not become a gc root,
        //       so `nix-store --gc` might delete the toolchain.
        //       we might want to provide options to deal with it.
        // IDEA: have a directory like `~/.rustup/toolchains` and use `--out-link` to link the
        //       results to there. then we can list "installed" toolchains and "uninstalling"
        //       them becomes a reasonable operation.
        .arg("--no-out-link")
        .arg("--expr")
        .arg(expr)
        .output()
        .expect("couldn't build rust toolchain") // this only checks for failures to *run* the command
        .stdout
        .also(|v| assert_eq!(v.pop(), Some(b'\n')))
        .apply(OsString::from_vec) // N.B.: unix only
        .apply(PathBuf::from);

    debug!("starting nix-build finished");

    let bin_path = toolchain_path.also(|p| {
        p.push("bin");
        p.push(bin);
    });

    debug!("starting {bin_path:?}");

    // FIXME:
    // we should probably set some env vars, to make sure toolchain doesn't change out of nowhere.
    // e.g. `cargo build` should use `rustc` from the same toolchain and not accidentally change
    // toolchains when building a project with a different `rust-toolchain.toml`?
    let status = Command::new(bin_path)
        .also(|c| {
            if !toolchain_overridden_from_args {
                if let Some(arg) = toolchain_override_or_arg {
                    c.arg(arg);
                }
            }
        })
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .status()
        .expect("failed to not fail");

    debug!("binary finished");

    process::exit(status.code().unwrap_or(0));
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
enum ToolchainOverride {
    File(Box<Path>),
    Version {
        channel: Channel,
        version: Option<String>,
    },
    None,
    // FIXME: how is this supposed to work??
    //LocalName(String),
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(test, derive(Eq, PartialEq))]
enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }
}

fn parse_toolchain_override(s: Option<&str>) -> Result<Option<ToolchainOverride>, ()> {
    let Some(s) = s else { return Ok(None) };

    let Some(s) = s.strip_prefix('+') else {
        return Ok(None);
    };

    if let Some(s) = s.strip_prefix("stable") {
        let version = parse_toolchain_version(s)?;
        return Ok(Some(ToolchainOverride::Version {
            channel: Channel::Stable,
            version,
        }));
    }

    if let Some(s) = s.strip_prefix("beta") {
        let version = parse_toolchain_version(s)?;
        return Ok(Some(ToolchainOverride::Version {
            channel: Channel::Beta,
            version,
        }));
    }

    if let Some(s) = s.strip_prefix("nightly") {
        let version = parse_toolchain_version(s)?;
        return Ok(Some(ToolchainOverride::Version {
            channel: Channel::Nightly,
            version,
        }));
    }

    // Invalid toolchain override specification
    Err(())
}

fn parse_toolchain_version(s: &str) -> Result<Option<String>, ()> {
    if s.is_empty() {
        return Ok(None);
    }

    s.strip_prefix("-").map(str::to_owned).map(Some).ok_or(())
}

fn find_toolchain_file() -> Result<Option<ToolchainOverride>, ()> {
    let current_dir = current_dir().map_err(drop)?;

    iter::successors(Some(&*current_dir), |d| d.parent())
        .map(|d| d.join("rust-toolchain.toml"))
        .find(|f| f.exists())
        .map(PathBuf::into_boxed_path)
        .map(ToolchainOverride::File)
        .apply(Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        assert_eq!(parse_toolchain_override(None), Ok(None));
        assert_eq!(parse_toolchain_override(Some("not-plus")), Ok(None));
        assert_eq!(
            parse_toolchain_override(Some("+stable")),
            Ok(Some(ToolchainOverride::Version {
                channel: Channel::Stable,
                version: None
            }))
        );
        assert_eq!(
            parse_toolchain_override(Some("+stable-")),
            Ok(Some(ToolchainOverride::Version {
                channel: Channel::Stable,
                version: Some("".to_owned())
            }))
        );
        assert_eq!(
            parse_toolchain_override(Some("+stable-1.78")),
            Ok(Some(ToolchainOverride::Version {
                channel: Channel::Stable,
                version: Some("1.78".to_owned())
            }))
        );
    }
}
