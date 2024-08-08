use std::{
    env::{self, current_dir},
    fs,
    io::Read,
    iter,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
};

use itertools::Itertools as _;

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

    let shell_path = PathBuf::from(format!("/tmp/rustdn-shell-{:?}.nix", random_bytes::<12>()));

    // FIXME: This feels slow, there should be a better way to activate a particular toolchain than
    //        writing a nix-shell to /tmp... We should probably just cleverly set `$PATH`, but I'm
    //        not exactly sure how to find already "installed" toolchains.

    let script = format!(
        r#"{{
  pkgs ? import <nixpkgs> {{
    overlays = [
      (import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))
    ];
  }},
}}:
pkgs.mkShell {{
  name = "rustdn";
  buildInputs = [ (pkgs.rust-bin.{}) ];
}}
"#,
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

    fs::write(&shell_path, script).unwrap();

    let status = Command::new("nix-shell")
        .arg(&shell_path)
        .arg("--run")
        .arg(
            [bin.to_owned()]
                .into_iter()
                .chain(toolchain_override_or_arg.filter(|_| !toolchain_overridden_from_args))
                .chain(args)
                .join(" "),
        )
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit())
        .status()
        .expect("failed to not fail");

    fs::remove_file(shell_path).unwrap();

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

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut rnd = [0; N];

    fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut rnd)
        .unwrap();

    rnd
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
