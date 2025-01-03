use std::{env, process::Command};

fn main() {
    let commit_envs_missing =
        env::var("VCS_COMMIT_FULL").is_err() || env::var("VCS_COMMIT_ABBR").is_err();
    let date_env_missing = env::var("VCS_COMMIT_DATE").is_err();

    if !commit_envs_missing && !date_env_missing {
        // don't run `git` if there is no need to.
        // (this allows building this without `git` binary present, by setting all the envs)
        return;
    }

    // wffl: if someone can come up with a nicer/simpler command (that for example doesn't include time),
    //       i will kiss them on the forehead (if they want)
    let output = Command::new("git")
        .args(&[
            "log",
            "HEAD",
            "-1",
            // TZ is only respected by iso-local and local it seems like
            "--date=iso-local",
            // hash-abbr hash-full commiter-date
            "--format=%h %H %cd",
        ])
        .env("TZ", "UTC")
        .output();

    match output {
        Ok(output) => {
            // Output is something like this:
            // 34b7ee1 34b7ee149d22a7ed78c0a0a9acf76a21bf0b600e 2025-01-03 11:47:20 +0000
            let output = String::from_utf8(output.stdout).unwrap();
            let mut parts = output.split(" ");

            let hash_abbr = parts.next().unwrap();
            let hash_full = parts.next().unwrap();
            let date = parts.next().unwrap();
            let _time = parts.next().unwrap();
            let tz = parts.next().unwrap();

            assert_eq!(tz, "+0000\n");

            // Allow overwriting git hash when building, if both variables are already set
            if commit_envs_missing {
                println!("cargo::rustc-env=VCS_COMMIT_ABBR={hash_abbr}");
                println!("cargo::rustc-env=VCS_COMMIT_FULL={hash_full}");
            }

            if date_env_missing {
                println!("cargo::rustc-env=VCS_COMMIT_DATE={date}");
            }
        }
        Err(e) => {
            println!("cargo::warning=couldn't run git to get current commit: {e}")
        }
    }
}
