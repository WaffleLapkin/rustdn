pub(super) fn get() -> String {
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
