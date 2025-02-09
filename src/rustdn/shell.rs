use std::{
    env,
    ffi::OsString,
    os::unix::process::CommandExt as _,
    path::PathBuf,
    process::{self, Command},
};

use clap::Parser;
use tracing::debug;

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

pub(super) fn shell(args: ShellCmd) {
    let Ok(toolchain) = parse_toolchain_name(&args.toolchain) else {
        eprintln!("invalid toolchain name: {}", args.toolchain);
        process::exit(1);
    };

    debug!("toolchain override is {toolchain:?}");

    let toolchain = get_or_update_toolchain(toolchain);

    debug!("toolchain path is {}", toolchain.display());

    let cmd = args
        .cmd
        .or_else(|| env::var("SHELL").map(<_>::into).ok())
        .unwrap_or_else(|| {
            eprintln!("no shell specified: either `$SHELL` must be set or `--shell` must be used");
            process::exit(1);
        });

    debug!("shell is {}", cmd.display());

    let path = env::var_os("PATH").unwrap_or(<_>::default());

    debug!("$PATH = {path:?}");

    let path = toolchain
        .join("bin:")
        .into_os_string()
        .also(|s| s.push(path));

    debug!("new $PATH = {path:?}");

    let error = Command::new(cmd).env("PATH", path).args(args.args).exec();

    eprintln!("couldn't start the shell: {error}");
    process::exit(1);
}
