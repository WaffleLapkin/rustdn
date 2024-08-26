mod lock;
mod proxy;
mod rustdn;
mod toolchain;
mod unstd;

// FIXME: add actual error handling

fn main() {
    use std::{env, ffi::OsStr, path::Path};

    setup_tracing();

    let mut args = env::args();

    let arg0 = args.next();
    let bin = arg0
        .as_deref()
        .map(Path::new)
        .and_then(|a| a.file_stem())
        .and_then(OsStr::to_str);

    // `rustdn` is a "chimera binary" -- it changes behavior depending on the name of the
    // binary name (arg0). This is used to enable rustup-style "proxies" -- you can symlink `rustc`
    // to `rustdn` and `rustdn` will choose an appropriate `rustc` version and run it.
    match bin {
        Some("rustdn") => rustdn::main(args),
        Some(tool) => proxy::main(tool, args),

        // Edge-case: no arg0 (or it's last part is not utf-8!)
        None => panic!("No arg0?"),
    }
}

fn setup_tracing() {
    use tracing::level_filters::LevelFilter;
    use tracing_subscriber::{layer::SubscriberExt as _, EnvFilter, Layer as _, Registry};

    let logger = tracing_subscriber::fmt::layer()
        .with_writer(move || std::io::stderr())
        .with_ansi(true);

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse_lossy(std::env::var("RUSTDN_LOG").as_deref().unwrap_or(""));

    let console_logger = logger.compact().with_filter(env_filter).boxed();
    let subscriber = Registry::default().with(console_logger);

    tracing::subscriber::set_global_default(subscriber).unwrap();
}
