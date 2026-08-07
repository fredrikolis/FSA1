// Concern: locates the pinned Vega bundle this crate embeds | Non-concern: fetching or verifying it (scripts/fetch-vega.sh) | IO: (env or vendor/) -> the bundle's path, else a refusal
//! The bundle is gitignored, so a fresh clone has none until `scripts/fetch-vega.sh` has run.
//! `FSA1_VEGA_BUNDLE` is the OFFLINE escape: a machine with no network points at a bundle it
//! already holds. Failing loudly here beats embedding nothing and shipping a page that draws a
//! blank rectangle.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=FSA1_VEGA_BUNDLE");

    let vendored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/vega-bundle.js");
    let bundle = match std::env::var_os("FSA1_VEGA_BUNDLE") {
        Some(p) => PathBuf::from(p),
        None => vendored,
    };
    println!("cargo::rerun-if-changed={}", bundle.display());

    if !bundle.is_file() {
        panic!(
            "the pinned Vega runtime is missing at {}.\n\
             Run `bash scripts/fetch-vega.sh` from the repo root to fetch and verify it against \
             vega-manifest.txt, or set FSA1_VEGA_BUNDLE=<path> to a bundle you already hold.",
            bundle.display()
        );
    }
    println!(
        "cargo::rustc-env=FSA1_VEGA_BUNDLE_PATH={}",
        bundle.display()
    );
}
