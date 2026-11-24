use std::path::Path;

use resolvo_rpm::RpmProvider;

const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/assets");

pub fn repo_path(name: &str) -> std::path::PathBuf {
    Path::new(ASSETS_DIR).join(name)
}

pub fn load_cs10_provider() -> RpmProvider {
    let mut provider = RpmProvider::new(Some("x86_64"));
    provider.load_repo(&repo_path("cs10-baseos"), "cs10-baseos");
    provider.load_repo(&repo_path("cs10-appstream"), "cs10-appstream");
    provider
}
