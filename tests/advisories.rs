use std::collections::BTreeSet;

use resolvo_rpm::{
    DependencySpec, LoadOptions, PackageSpec, ResolveOptions, RpmProvider, UpdateCollection,
    UpdateCollectionPackage, UpdateRecord, UpdateReference, resolve,
};

mod common;

/// Helper: add a minimal package to the provider with the given NEVRA.
fn add_package(
    provider: &mut RpmProvider,
    repo_id: usize,
    name: &str,
    epoch: &str,
    version: &str,
    release: &str,
    arch: &str,
) {
    provider.add_package(
        repo_id,
        &PackageSpec {
            name,
            epoch,
            version,
            release,
            arch,
            requires: &[],
            provides: &[],
            conflicts: &[],
            obsoletes: &[],
            recommends: &[],
            suggests: &[],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );
}

/// Helper: add a package with a Requires dependency.
fn add_package_with_requires(
    provider: &mut RpmProvider,
    repo_id: usize,
    name: &str,
    epoch: &str,
    version: &str,
    release: &str,
    arch: &str,
    requires: &[DependencySpec],
) {
    provider.add_package(
        repo_id,
        &PackageSpec {
            name,
            epoch,
            version,
            release,
            arch,
            requires,
            provides: &[],
            conflicts: &[],
            obsoletes: &[],
            recommends: &[],
            suggests: &[],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );
}

/// Helper: build an UpdateRecord with a single collection of packages.
fn make_advisory(id: &str, packages: Vec<UpdateCollectionPackage>) -> UpdateRecord {
    UpdateRecord {
        id: id.to_owned(),
        update_type: "security".to_owned(),
        version: "1".to_owned(),
        pkglist: vec![UpdateCollection {
            packages,
            ..UpdateCollection::default()
        }],
        ..UpdateRecord::default()
    }
}

/// Helper: build an UpdateCollectionPackage with the given NEVRA.
fn make_advisory_pkg(
    name: &str,
    epoch: &str,
    version: &str,
    release: &str,
    arch: &str,
) -> UpdateCollectionPackage {
    UpdateCollectionPackage {
        name: name.to_owned(),
        epoch: epoch.to_owned(),
        version: version.to_owned(),
        release: release.to_owned(),
        arch: arch.to_owned(),
        ..UpdateCollectionPackage::default()
    }
}

/// Resolving a patch: advisory should include the virtual solvable and force
/// the referenced package to meet the advisory's minimum version.
#[test]
fn add_advisory_forces_upgrade() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // Two versions of "foo": old (1.0) and fixed (2.0)
    add_package(&mut provider, repo, "foo", "0", "1.0", "1.el9", "x86_64");
    add_package(&mut provider, repo, "foo", "0", "2.0", "1.el9", "x86_64");

    // Advisory says foo needs >= 2.0-1.el9
    let advisory = make_advisory(
        "RHSA-2024:0001",
        vec![make_advisory_pkg("foo", "0", "2.0", "1.el9", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0001", "foo"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let resolved: BTreeSet<(String, String)> = result
        .iter()
        .map(|s| {
            let p = solver.provider();
            (
                p.package_name(*s).to_string(),
                p.package_version(*s).to_string(),
            )
        })
        .collect();

    assert!(
        resolved.iter().any(|(n, _)| n == "patch:RHSA-2024:0001"),
        "the advisory virtual solvable should be in the result"
    );
    assert!(
        resolved
            .iter()
            .any(|(n, v)| n == "foo" && v.contains("2.0")),
        "foo should be upgraded to the fixed version, got: {:?}",
        resolved
    );
    assert!(
        !resolved
            .iter()
            .any(|(n, v)| n == "foo" && v.contains("1.0")),
        "the old version of foo should NOT be selected"
    );
}

/// When multiple versions satisfy the advisory constraint, the solver must
/// not pick a version older than the advisory's fix version.
#[test]
fn advisory_excludes_old_versions() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "bar", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "bar", "0", "2.0", "1", "x86_64");
    add_package(&mut provider, repo, "bar", "0", "3.0", "1", "x86_64");

    // Advisory says bar needs >= 2.0. The old 1.0 version must be excluded.
    let advisory = make_advisory(
        "RHSA-2024:0002",
        vec![make_advisory_pkg("bar", "0", "2.0", "1", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0002", "bar"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let versions: Vec<String> = result
        .iter()
        .filter_map(|s| {
            let p = solver.provider();
            if p.package_name(*s) == "bar" {
                Some(p.package_version(*s).to_string())
            } else {
                None
            }
        })
        .collect();

    assert_eq!(versions.len(), 1);
    assert!(
        !versions[0].contains("1.0"),
        "bar 1.0 should be excluded by the advisory constraint, got: {}",
        versions[0]
    );
}

/// An advisory referencing multiple packages should generate conflicts for
/// all of them. Resolving the advisory + all packages should upgrade each.
#[test]
fn advisory_with_multiple_packages() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // Two packages, each with an old and a new version
    add_package(&mut provider, repo, "alpha", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "alpha", "0", "2.0", "1", "x86_64");
    add_package(&mut provider, repo, "beta", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "beta", "0", "2.0", "1", "x86_64");

    let advisory = make_advisory(
        "RHSA-2024:0003",
        vec![
            make_advisory_pkg("alpha", "0", "2.0", "1", "x86_64"),
            make_advisory_pkg("beta", "0", "2.0", "1", "x86_64"),
        ],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0003", "alpha", "beta"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let resolved: BTreeSet<(String, String)> = result
        .iter()
        .map(|s| {
            let p = solver.provider();
            (
                p.package_name(*s).to_string(),
                p.package_version(*s).to_string(),
            )
        })
        .collect();

    assert!(
        resolved
            .iter()
            .any(|(n, v)| n == "alpha" && v.contains("2.0")),
        "alpha should be at version 2.0, got: {:?}",
        resolved
    );
    assert!(
        resolved
            .iter()
            .any(|(n, v)| n == "beta" && v.contains("2.0")),
        "beta should be at version 2.0, got: {:?}",
        resolved
    );
}

/// Source-arch packages in advisories should be skipped — they're not installable.
#[test]
fn advisory_skips_src_arch() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "baz", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "baz", "0", "2.0", "1", "x86_64");

    // Advisory has both an x86_64 entry and a src entry.
    // Only the x86_64 entry should generate a conflict.
    let advisory = make_advisory(
        "RHSA-2024:0004",
        vec![
            make_advisory_pkg("baz", "0", "2.0", "1", "x86_64"),
            make_advisory_pkg("baz", "0", "2.0", "1", "src"),
        ],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0004", "baz"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();

    assert!(names.contains("patch:RHSA-2024:0004"));
    assert!(names.contains("baz"));
}

/// Advisory with an epoch > 0 should correctly force upgrades past that epoch.
#[test]
fn advisory_with_nonzero_epoch() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // epoch=1 packages
    add_package(&mut provider, repo, "qux", "1", "3.0", "1", "x86_64");
    add_package(&mut provider, repo, "qux", "1", "4.0", "1", "x86_64");

    let advisory = make_advisory(
        "RHSA-2024:0005",
        vec![make_advisory_pkg("qux", "1", "4.0", "1", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0005", "qux"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let resolved: BTreeSet<(String, String)> = result
        .iter()
        .map(|s| {
            let p = solver.provider();
            (
                p.package_name(*s).to_string(),
                p.package_version(*s).to_string(),
            )
        })
        .collect();

    assert!(
        resolved
            .iter()
            .any(|(n, v)| n == "qux" && v.contains("4.0")),
        "qux should be at version 4.0 (epoch 1), got: {:?}",
        resolved
    );
}

/// Advisory conflict should cascade through transitive dependencies:
/// if package "app" requires "lib", and an advisory forces "lib" to upgrade,
/// resolving the advisory + app should pull in the upgraded lib.
#[test]
fn advisory_cascades_through_deps() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // "app" requires "lib"
    add_package_with_requires(
        &mut provider,
        repo,
        "app",
        "0",
        "1.0",
        "1",
        "x86_64",
        &[DependencySpec {
            name: "lib",
            flags: None,
            epoch: None,
            version: None,
            release: None,
            preinstall: false,
        }],
    );

    // Two versions of lib
    add_package(&mut provider, repo, "lib", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "lib", "0", "2.0", "1", "x86_64");

    // Advisory says lib >= 2.0
    let advisory = make_advisory(
        "RHSA-2024:0006",
        vec![make_advisory_pkg("lib", "0", "2.0", "1", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:0006", "app"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let resolved: BTreeSet<(String, String)> = result
        .iter()
        .map(|s| {
            let p = solver.provider();
            (
                p.package_name(*s).to_string(),
                p.package_version(*s).to_string(),
            )
        })
        .collect();

    assert!(
        resolved.iter().any(|(n, _)| n == "app"),
        "app should be in the result"
    );
    assert!(
        resolved
            .iter()
            .any(|(n, v)| n == "lib" && v.contains("2.0")),
        "lib should be upgraded to 2.0 via advisory cascade, got: {:?}",
        resolved
    );
}

/// Loading advisories from a real repo (el9-baseos) should create patch:
/// virtual solvables that are discoverable in the pool.
#[test]
fn load_advisories_from_repo() {
    let load_options = LoadOptions::new().load_advisories(true);

    let mut provider = RpmProvider::new(Some("x86_64"));
    provider.load_repo_with_options(
        &common::repo_path("el9-baseos"),
        "el9-baseos",
        &load_options,
    );

    // Check that at least one advisory solvable was loaded
    let advisory_count = provider
        .pool
        .iter_solvables()
        .filter(|(sid, _)| {
            let name = provider.package_name(*sid);
            name.starts_with("patch:")
        })
        .count();

    assert!(
        advisory_count > 0,
        "should have loaded advisory solvables from el9-baseos"
    );

    // RHSA-2023:2523 (openssl security advisory) should be among them
    let has_openssl_advisory = provider
        .pool
        .iter_solvables()
        .any(|(sid, _)| provider.package_name(sid) == "patch:RHSA-2023:2523");

    assert!(
        has_openssl_advisory,
        "RHSA-2023:2523 (openssl) should be loaded as a virtual solvable"
    );
}

/// Advisory should not be loaded when load_advisories is false (the default).
#[test]
fn advisories_not_loaded_by_default() {
    let mut provider = RpmProvider::new(Some("x86_64"));
    provider.load_repo(&common::repo_path("el9-baseos"), "el9-baseos");

    let advisory_count = provider
        .pool
        .iter_solvables()
        .filter(|(sid, _)| {
            let name = provider.package_name(*sid);
            name.starts_with("patch:")
        })
        .count();

    assert_eq!(
        advisory_count, 0,
        "no advisory solvables should be loaded by default"
    );
}

/// Mixed advisory + package resolution: resolving a patch advisory alongside
/// regular packages should succeed, with the advisory forcing upgrades.
#[test]
fn mixed_advisory_and_package_resolution() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "secure-lib", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "secure-lib", "0", "2.0", "1", "x86_64");
    add_package(
        &mut provider,
        repo,
        "unrelated-pkg",
        "0",
        "1.0",
        "1",
        "x86_64",
    );

    let advisory = make_advisory(
        "RHSA-2024:9999",
        vec![make_advisory_pkg("secure-lib", "0", "2.0", "1", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(
        &mut solver,
        &["patch:RHSA-2024:9999", "secure-lib", "unrelated-pkg"],
        &ResolveOptions::new(),
    )
    .unwrap();

    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();

    assert!(names.contains("patch:RHSA-2024:9999"));
    assert!(names.contains("secure-lib"));
    assert!(names.contains("unrelated-pkg"));
}

// --- Advisory index / query tests ---

/// Manually added advisories should be queryable by ID.
#[test]
fn advisory_index_by_id() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "foo", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "foo", "0", "2.0", "1", "x86_64");

    let advisory = make_advisory(
        "RHSA-2024:1111",
        vec![make_advisory_pkg("foo", "0", "2.0", "1", "x86_64")],
    );
    provider.add_advisory(repo, &advisory);

    let result = provider.advisory_by_id("RHSA-2024:1111");
    assert!(result.is_some());
    assert_eq!(result.unwrap().id, "RHSA-2024:1111");
    assert_eq!(result.unwrap().update_type, "security");

    assert!(provider.advisory_by_id("RHSA-9999:0000").is_none());
}

/// Advisories should be queryable by affected package name.
#[test]
fn advisory_index_by_package() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "alpha", "0", "1.0", "1", "x86_64");
    add_package(&mut provider, repo, "beta", "0", "1.0", "1", "x86_64");

    let advisory1 = make_advisory(
        "RHSA-2024:1001",
        vec![make_advisory_pkg("alpha", "0", "2.0", "1", "x86_64")],
    );
    let advisory2 = make_advisory(
        "RHSA-2024:1002",
        vec![
            make_advisory_pkg("alpha", "0", "3.0", "1", "x86_64"),
            make_advisory_pkg("beta", "0", "2.0", "1", "x86_64"),
        ],
    );
    provider.add_advisory(repo, &advisory1);
    provider.add_advisory(repo, &advisory2);

    let alpha_advisories = provider.advisories_for_package("alpha");
    assert_eq!(alpha_advisories.len(), 2);

    let beta_advisories = provider.advisories_for_package("beta");
    assert_eq!(beta_advisories.len(), 1);
    assert_eq!(beta_advisories[0].id, "RHSA-2024:1002");

    assert!(provider.advisories_for_package("gamma").is_empty());
}

/// Advisories should be queryable by CVE reference.
#[test]
fn advisory_index_by_cve() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "foo", "0", "1.0", "1", "x86_64");

    let mut advisory = make_advisory(
        "RHSA-2024:2001",
        vec![make_advisory_pkg("foo", "0", "2.0", "1", "x86_64")],
    );
    advisory.references = vec![
        UpdateReference {
            href: "https://access.redhat.com/security/cve/CVE-2024-0001".to_owned(),
            id: Some("CVE-2024-0001".to_owned()),
            reftype: "cve".to_owned(),
            title: "CVE-2024-0001".to_owned(),
        },
        UpdateReference {
            href: "https://bugzilla.redhat.com/1234".to_owned(),
            id: Some("1234".to_owned()),
            reftype: "bugzilla".to_owned(),
            title: "some bug".to_owned(),
        },
    ];
    provider.add_advisory(repo, &advisory);

    let by_cve = provider.advisories_by_cve("CVE-2024-0001");
    assert_eq!(by_cve.len(), 1);
    assert_eq!(by_cve[0].id, "RHSA-2024:2001");

    // Bugzilla refs should not appear in CVE index
    assert!(provider.advisories_by_cve("1234").is_empty());
    assert!(provider.advisories_by_cve("CVE-9999-0000").is_empty());
}

/// Advisories should be filterable by type.
#[test]
fn advisory_index_by_type() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    add_package(&mut provider, repo, "foo", "0", "1.0", "1", "x86_64");

    let mut sec_advisory = make_advisory(
        "RHSA-2024:3001",
        vec![make_advisory_pkg("foo", "0", "2.0", "1", "x86_64")],
    );
    sec_advisory.update_type = "security".to_owned();

    let mut bug_advisory = make_advisory(
        "RHBA-2024:3002",
        vec![make_advisory_pkg("foo", "0", "3.0", "1", "x86_64")],
    );
    bug_advisory.update_type = "bugfix".to_owned();

    provider.add_advisory(repo, &sec_advisory);
    provider.add_advisory(repo, &bug_advisory);

    let security = provider.advisories_by_type("security");
    assert_eq!(security.len(), 1);
    assert_eq!(security[0].id, "RHSA-2024:3001");

    let bugfix = provider.advisories_by_type("bugfix");
    assert_eq!(bugfix.len(), 1);
    assert_eq!(bugfix[0].id, "RHBA-2024:3002");

    assert!(provider.advisories_by_type("enhancement").is_empty());
}

/// Loading advisories from a real repo should populate the advisory index.
#[test]
fn advisory_index_from_repo() {
    let load_options = LoadOptions::new().load_advisories(true);

    let mut provider = RpmProvider::new(Some("x86_64"));
    provider.load_repo_with_options(
        &common::repo_path("el9-baseos"),
        "el9-baseos",
        &load_options,
    );

    assert!(
        !provider.advisories().is_empty(),
        "advisories vec should be populated"
    );

    // RHSA-2023:2523 is a security advisory for openssl
    let advisory = provider.advisory_by_id("RHSA-2023:2523");
    assert!(advisory.is_some(), "RHSA-2023:2523 should be in the index");
    let advisory = advisory.unwrap();
    assert_eq!(advisory.update_type, "security");
    assert_eq!(advisory.severity, Some("Low".to_owned()));
    assert_eq!(advisory.title, "Low: openssl security and bug fix update");

    // Should be findable by package name
    let openssl_advisories = provider.advisories_for_package("openssl");
    assert!(
        openssl_advisories.iter().any(|a| a.id == "RHSA-2023:2523"),
        "openssl should be referenced by RHSA-2023:2523"
    );

    // Should be findable by CVE
    let cve_advisories = provider.advisories_by_cve("CVE-2022-3358");
    assert!(
        cve_advisories.iter().any(|a| a.id == "RHSA-2023:2523"),
        "CVE-2022-3358 should map to RHSA-2023:2523"
    );

    // Should be filterable by type
    let security = provider.advisories_by_type("security");
    assert!(
        security.iter().any(|a| a.id == "RHSA-2023:2523"),
        "security filter should include RHSA-2023:2523"
    );
}

/// Advisory index should be empty when load_advisories is false.
#[test]
fn advisory_index_not_loaded_by_default() {
    let mut provider = RpmProvider::new(Some("x86_64"));
    provider.load_repo(&common::repo_path("el9-baseos"), "el9-baseos");

    assert!(provider.advisories().is_empty());
    assert!(provider.advisory_by_id("RHSA-2023:2523").is_none());
    assert!(provider.advisories_for_package("openssl").is_empty());
}
