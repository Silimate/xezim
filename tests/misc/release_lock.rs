//! The release builder uses a separate lockfile. Keep the two core packages
//! it resolves at the revisions requested by the simulator manifest.
#[test]
fn release_lock_matches_core_revisions() {
    let manifest = include_str!("../../Cargo.toml");
    let lock = include_str!("../../release/Cargo.lock");
    for name in ["xezim-core", "sv-parser"] {
        let (_, spec) = manifest
            .lines()
            .filter_map(|line| line.split_once('='))
            .find(|(key, _)| key.trim() == name)
            .expect("core dependency in Cargo.toml");
        let revision = spec
            .trim_matches(|c: char| c.is_whitespace() || c == '{' || c == '}')
            .split(',')
            .filter_map(|field| field.split_once('='))
            .find(|(key, _)| key.trim() == "rev")
            .expect("pinned core dependency")
            .1
            .trim()
            .trim_matches('"');
        let package = lock
            .split("[[package]]")
            .find(|entry| entry.lines().any(|l| l == format!("name = \"{name}\"")))
            .expect("core package in release/Cargo.lock");
        let source = package
            .lines()
            .find(|l| l.starts_with("source = "))
            .expect("git source in release/Cargo.lock");
        assert!(
            source.ends_with(&format!("?rev={revision}#{revision}\"")),
            "{name}: {source}"
        );
    }
}
