use std::{
    env::{self},
    os::unix::process::CommandExt as _,
    process::{Command, Stdio},
};

use tracing::{debug, trace};

use crate::{
    toolchain::{
        find_toolchain_file, get_or_update_toolchain, parse_toolchain_override, ToolchainOverride,
    },
    unstd::AnyExt as _,
};

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

    let toolchain = get_or_update_toolchain(toolchain);

    debug!("toolchain found");

    let bin_path = toolchain
        // directory with the binaries
        .join("bin")
        // the binary itself
        .join(bin);

    debug!("starting {bin_path:?}");

    // FIXME:
    // we should probably set some env vars, to make sure toolchain doesn't change out of nowhere.
    // e.g. `cargo build` should use `rustc` from the same toolchain and not accidentally change
    // toolchains when building a project with a different `rust-toolchain.toml`?
    let error = Command::new(&bin_path)
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
        .exec();

    panic!("couldn't execute {bin_path:?}: {error}");
}
