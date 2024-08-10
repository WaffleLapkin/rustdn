use core::{fmt, slice};
use std::{
    env::{self, current_dir},
    ffi::OsString,
    fs, iter,
    ops::{ControlFlow, Deref},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    str, thread,
    time::Duration,
};

use file_guard::{os::unix::FileGuardExt, FileGuard, Lock};
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

    let toolchain_key = toolchain.key();

    let toolchain_dir = dirs::home_dir()
        .unwrap()
        .join(".rustdn/toolchains")
        .join(toolchain_key);

    fs::create_dir_all(&toolchain_dir).unwrap();

    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(toolchain_dir.join("lock"))
        .unwrap();

    debug!("starting looking for the toolchain");

    loop {
        let mut guard = file_guard::lock(&lock, Lock::Shared, 0, 1024).unwrap();

        if toolchain_dir.join("toolchain").exists()
            && toolchain.cache_is_valid(&toolchain_dir, &guard)
        {
            // we are free
            break;
        }

        if let Err(e) = guard.upgrade() {
            // Deadlock is 30
            // FIXME: use the actual `io::ErrorKind::Deadlock` (once it's stabilized or w/e)
            if e.kind() as u32 == 30 {
                // Deadlock error is returned when multiple readers are trying to upgrade.
                // Retry after a small delay to let one the other processes trying to upgrade, upgrade.
                // FIXME: we might need to sleep a random amount of time,
                //        *if `Deadlock` is returned to all the processes*,
                //        so that one of them has a chance to get an exclusive lock.
                thread::sleep(Duration::from_secs_f32(0.1));
                continue;
            }

            Err::<(), _>(e).unwrap();
        }

        // We are in exclusive possession of this toolchain.
        // Try to update it to be up to date.
        assert!(guard.is_exclusive());

        let expr = format!(
            "{}{}",
            r#"{}: (import <nixpkgs> {overlays = [(import (builtins.fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))];}).rust-bin."#,
            match &toolchain {
                ToolchainOverride::File(f) =>
                    format!(r#"fromRustupToolchainFile "{}""#, f.display()),
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
        Command::new("nix-build")
            // Don't create `./result` symlinks.
            // N.B.: this means that the result of the build does not become a gc root,
            //       so `nix-store --gc` might delete the toolchain.
            //       we might want to provide options to deal with it.
            // IDEA: have a directory like `~/.rustup/toolchains` and use `--out-link` to link the
            //       results to there. then we can list "installed" toolchains and "uninstalling"
            //       them becomes a reasonable operation.
            .arg("--out-link")
            .arg(toolchain_dir.join("toolchain"))
            .arg("--expr")
            .arg(expr)
            .output()
            .expect("couldn't build rust toolchain") // this only checks for failures to *run* the command
            .also(|o| _ = dbg!(String::from_utf8_lossy(&o.stderr)));

        debug!("starting nix-build finished");

        if let ControlFlow::Break(()) = toolchain.commit_cache(&toolchain_dir, &mut guard) {
            break;
        }
    }

    debug!("toolchain found");

    let bin_path = toolchain_dir
        // symlink to the toolchain in the nix store (or just *somewhere* in case of local toolchains)
        .join("toolchain")
        // directory with the binaries
        .join("bin")
        // the binary itself
        .join(bin);

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

impl ToolchainOverride {
    // N.B. all function here must agree with each other.

    fn key(&self) -> OsString {
        match self {
            ToolchainOverride::File(f) => {
                let mut key = OsString::from("file-");

                // FIXME: figure out an encoding for paths which is less cursed

                const ESC: u8 = 0x10;
                f.as_os_str().as_encoded_bytes().iter().for_each(|&b| {
                    if b.is_ascii() && b != ESC && b != b'/' {
                        key.push(str::from_utf8(slice::from_ref(&b)).unwrap())
                    } else {
                        key.push(format!("\x10{b:x}"))
                    }
                });

                key
            }
            ToolchainOverride::Version {
                channel,
                version: Some(version),
            } => format!("external-{channel}-{version}").into(),
            ToolchainOverride::Version {
                channel,
                version: None,
            } => format!("external-{channel}").into(),
            ToolchainOverride::None => "default".to_owned().into(),
        }
    }

    /// Returns `true` if the cached version of the toolchain for this override can be trusted.
    /// Or, in other words, that the toolchain version can be solely determined on input
    /// parameters/cache key, so the cached version can't change.
    ///
    /// For [`File`] this checks if the toolchain file we used before is exactly the same as the current one.
    /// For [`Version`] this checks that [`Version::version`] is specified.
    /// [`None`] can never depend on cache.
    ///
    /// [`File`]: ToolchainOverride::File
    /// [`Version`]: ToolchainOverride::Version
    /// [`Version::version`]: ToolchainOverride::Version::version
    ///
    /// **N.B.**: you still need to check that the cache actually exists.
    fn cache_is_valid(
        &self,
        path: &Path,
        _guard: &FileGuard<impl Deref<Target = fs::File>>,
    ) -> bool {
        match self {
            ToolchainOverride::File(current) => {
                let current_contents = fs::read(current).unwrap();
                let Ok(cached_contents) = fs::read(path.join("rust-toolchain.toml")) else {
                    return false;
                };

                current_contents == cached_contents
            }

            ToolchainOverride::Version { version, .. } => version.is_some(),

            // FIXME: If we could force somehow that update of the default toolchain causes the
            //        `toolchains/default` cache to be deleted, then we could actually trust this
            //        (and similarly for version-less version spec).
            //        Jono says it's possible, but I'm not sure how.
            ToolchainOverride::None => false,
        }
    }

    /// Commits the new toolchain to cache.
    ///
    /// Returns [`ControlFlow::Continue`] if the cache should be re-checked.
    /// Returns [`ControlFlow::Break`] if the cache mustn't be rechecked.
    fn commit_cache(
        &self,
        toolchain_dir: &PathBuf,
        guard: &mut FileGuard<impl Deref<Target = fs::File>>,
    ) -> ControlFlow<()> {
        assert!(guard.is_exclusive());

        match self {
            ToolchainOverride::File(p) => {
                fs::copy(p, toolchain_dir.join("rust-toolchain.toml")).unwrap();
                ControlFlow::Continue(())
            }
            ToolchainOverride::Version {
                version: Some(_), ..
            } => ControlFlow::Continue(()),

            // These never say that the cache is valid, so there is no reason to re-check it after `nix-build`
            ToolchainOverride::None | ToolchainOverride::Version { version: None, .. } => {
                ControlFlow::Break(())
            }
        }
    }
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

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
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
