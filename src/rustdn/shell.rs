use std::{
    env, ffi::OsString, os::unix::process::CommandExt as _, path::PathBuf, process::Command,
};

use clap::Parser;
use color_eyre::eyre::{self, bail, Context, OptionExt};
use tracing::{debug, trace};

use crate::{
    toolchain::{get_or_update_toolchain, parse_toolchain_name},
    unstd::AnyExt as _,
};

/// Create an interactive shell with access to tools from a specified toolchain, or run a command
/// from the toolchain.
///
/// N.B. this works by running specified command (`$SHELL` by default) with `$PATH` prepended with
/// toolchain's binaries.
#[derive(Parser, Debug, Clone)]
pub(super) struct ShellCmd {
    /// Name of the toolchain to be used for the shell.
    toolchain: String,

    /// By default `rustdn shell` creates an interactive shell with `$SHELL` (which corresponds to
    /// current user's *default* shell). This option uses a provided program instead.
    ///
    /// This can be a shell (`rustdn shell -c fish`) or any command (`rustdn shell -c rustc -- --version`)
    #[arg(short, long)]
    cmd: Option<PathBuf>,

    /// Arguments to pass to the shell/command.
    #[arg(last = true)]
    args: Vec<OsString>,
}

pub(super) fn shell(args: ShellCmd) -> eyre::Result<()> {
    let Ok(toolchain) = parse_toolchain_name(&args.toolchain) else {
        bail!("invalid toolchain name: {}", args.toolchain)
    };
    debug!(?toolchain);

    let toolchain_path = get_or_update_toolchain(toolchain);
    debug!(?toolchain_path);

    let command = args
        .cmd
        .or_else(|| env::var("SHELL").map(<_>::into).ok())
        .ok_or_eyre("no shell specified: either `$SHELL` must be set or `--shell` must be used")?;
    debug!(?command);

    let path = env::var_os("PATH").unwrap_or(<_>::default());
    trace!("old $PATH = {path:?}");

    let path = toolchain_path
        .join("bin:")
        .into_os_string()
        .also(|s| s.push(path));
    trace!("new $PATH = {path:?}");

    let error = Command::new(command)
        .env("PATH", path)
        .args(args.args)
        .exec();

    Err(error).wrap_err("couldn't start the shell: {error}")
}
