use std::env;

use color_eyre::eyre::{self, bail, Context};
use tracing_subscriber::fmt::time::Uptime;

mod lock;
mod proxy;
mod rustdn;
mod toolchain;
mod unstd;

// FIXME: add actual error handling
// FIXME: meow meow meow mrrrmph~!

fn main() -> eyre::Result<()> {
    use std::{env, ffi::OsStr, path::Path};

    color_eyre::install()?;
    setup_tracing()?;
    tracing::trace!("meow");

    let mut args = env::args_os();

    let arg0 = args.next();
    let bin = arg0
        .as_deref()
        .map(Path::new)
        .and_then(|a| a.file_stem())
        .and_then(OsStr::to_str)
        // Edge-case: no arg0 (or it's last part is not utf-8!)
        .expect("No arg0 or arg0 is not utf8?");

    // `rustdn` is a "chimera binary" -- it changes behavior depending on the name of the
    // binary name (arg0). This is used to enable rustup-style "proxies" -- you can symlink `rustc`
    // to `rustdn` and `rustdn` will choose an appropriate `rustc` version and run it.
    match bin {
        "rustdn" => rustdn::main(args),
        tool => proxy::main(tool, args),
    }
}

fn setup_tracing() -> eyre::Result<()> {
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::{layer::SubscriberExt as _, EnvFilter, Layer as _, Registry};

    let rustdn_log = match std::env::var("RUSTDN_LOG") {
        Ok(v) => v,
        Err(env::VarError::NotPresent) => <_>::default(),
        Err(env::VarError::NotUnicode(contents)) => {
            bail!("$RUSTDN_LOG contains non-unicode symbols: {contents:?}")
        }
    };

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse(rustdn_log)
        .wrap_err("couldn't parse $RUSTDN_LOG")?;

    let console_logger = tracing_subscriber::fmt::layer()
        .with_writer(move || std::io::stderr())
        .with_timer(Uptime::default())
        .with_ansi(true)
        .compact()
        .with_filter(env_filter);

    let subscriber = Registry::default().with(console_logger);

    tracing::subscriber::set_global_default(subscriber).wrap_err("couldn't set global logger")
}
