//! Toolchain management.

use core::{fmt, slice, str};
use std::{
    env::current_dir,
    ffi::{OsStr, OsString},
    fs,
    io::{stderr, Write as _},
    iter,
    ops::{ControlFlow, Deref},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::{self, Command},
    str::FromStr,
    thread,
    time::Duration,
};

use tracing::debug;

use crate::{
    lock::{Exclusive, Lock},
    unstd::AnyExt as _,
};

/// Returns path to a toolchain directory somewhere in nix store.
pub fn get_or_update_toolchain(toolchain: ToolchainOverride) -> PathBuf {
    let toolchain_key = toolchain.key();

    let toolchain_dir = dirs::home_dir()
        .unwrap()
        .join(".rustdn/toolchains")
        .join(toolchain_key);

    fs::create_dir_all(&toolchain_dir).unwrap();

    let lock_file = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .open(toolchain_dir.join("lock"))
        .unwrap();

    debug!("starting looking for the toolchain");

    loop {
        let lock = crate::lock::lock_shared(&lock_file).unwrap();

        if toolchain_dir.join("toolchain").exists()
            && toolchain.cache_is_valid(&toolchain_dir, &lock)
        {
            // we are free
            break;
        }

        let mut lock = match lock.upgrade() {
            Ok(l) => l,
            Err(e) if e == rustix::io::Errno::DEADLK => {
                // DEADLK error is returned when multiple readers are trying to upgrade.
                // it's returned to all, but one, processes.

                // a small delay to make sure the one process that didn't get the error can actually get an exclusive lock.
                // this is likely unnecessary, but since updates usually take much more than a second, this is fine to leave.
                thread::sleep(Duration::from_secs_f32(0.1));
                continue;
            }
            e => e.unwrap(),
        };

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

        // FIXME: we should report *something* if `nix-build` is running for longer than, say, a second.
        //        some kind of throbber would be nice, to show that *something* is happening,
        //        toolchain is being downloaded
        let output = Command::new("nix-build")
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
            .expect("couldn't start `nix-build` to build rust toolchain");

        // Very important: fail if `nix-build` failed.
        // This *must* happen before we commit to the cache,
        // since otherwise we might create an invalid cache and go insane.
        if !output.status.success() {
            eprintln!("`nix-build` failed:");
            stderr().write_all(&output.stderr).unwrap();

            // Just to be safe (and, well, correct for non-file toolchains),
            // remove the cache entirely.
            fs::remove_dir_all(toolchain_dir).unwrap();

            process::exit(output.status.code().unwrap_or(1));
        }

        debug!("starting nix-build finished");

        if let ControlFlow::Break(()) = toolchain.commit_cache(&toolchain_dir, &mut lock) {
            break;
        }
    }

    toolchain_dir.join("toolchain")
}

#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub enum ToolchainOverride {
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
            // FIXME: use different keys for channel and channel+version
            //        (channel should be a "global" key?)
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

    pub fn from_key(k: OsString) -> Option<Self> {
        if let Some(mut encoded_path) = k.as_bytes().strip_prefix(b"file-") {
            const ESC: u8 = 0x10;
            const SEP: u8 = b'/';

            let mut path = PathBuf::new();
            let mut buf = Vec::new();

            loop {
                match encoded_path {
                    &[ESC, a, b, ref rest @ ..] => {
                        let x = u8::from_str_radix(str::from_utf8(&[a, b]).ok()?, 16).ok()?;

                        match x {
                            ESC => buf.push(x),
                            SEP => {
                                path.push(OsStr::from_bytes(&buf));
                                buf.clear();
                            }
                            _ => return None,
                        }
                        encoded_path = rest;
                    }
                    &[ESC, ..] => return None,
                    &[fst, ref rest @ ..] => {
                        buf.push(fst);
                        encoded_path = rest;
                    }
                    [] => break,
                }
            }

            if !buf.is_empty() {
                path.push(OsStr::from_bytes(&buf));
                drop(buf);
            }

            return Some(Self::File(path.into_boxed_path()));
        }

        if let Some(rest) = k.as_bytes().strip_prefix(b"external-") {
            let rest = str::from_utf8(rest).ok()?;
            let toolchain = match rest.split_once("-") {
                Some((channel, version)) => ToolchainOverride::Version {
                    channel: channel.parse().ok()?,
                    version: Some(version.to_owned()),
                },
                None => ToolchainOverride::Version {
                    channel: rest.parse().ok()?,
                    version: None,
                },
            };

            return Some(toolchain);
        }

        if k.as_bytes() == b"default" {
            return Some(ToolchainOverride::None);
        }

        None
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
        _lock: &Lock<impl Deref<Target = fs::File>, impl Sized>,
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
        _lock: &mut Lock<impl Deref<Target = fs::File>, Exclusive>,
    ) -> ControlFlow<()> {
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
pub enum Channel {
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

impl FromStr for Channel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stable" => Ok(Channel::Stable),
            "beta" => Ok(Channel::Beta),
            "nightly" => Ok(Channel::Nightly),
            _ => Err(()),
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn parse_toolchain_override(s: Option<&str>) -> Result<Option<ToolchainOverride>, ()> {
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

pub fn find_toolchain_file() -> Result<Option<ToolchainOverride>, ()> {
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
