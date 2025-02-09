use std::{
    env,
    ffi::OsString,
    fs,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{self, Command},
};

use clap::{Parser, Subcommand};
use tracing::{debug, warn};

use crate::{
    toolchain::{get_or_update_toolchain, parse_toolchain_name, ToolchainOverride},
    unstd::AnyExt,
};

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
    Shell(ShellCmd),
}

#[derive(Debug, Subcommand, Clone)]
enum ToolchainCmd {
    /// List available toolchains.
    List,
}

/// Create an interactive shell with access to tools from a specified toolchain, or run a command
/// from the toolchain.
///
/// N.B. this works by running specified command (`$SHELL` by default) with `$PATH` prepended with
/// toolchain's binaries.
#[derive(Parser, Debug, Clone)]
struct ShellCmd {
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
        Cmd::Shell(args) => shell(args),
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

fn shell(args: ShellCmd) {
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
