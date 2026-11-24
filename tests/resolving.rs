use resolvo::Interner;
use resolvo_rpm::{ResolveOptions, RpmProvider, resolve};
use std::collections::BTreeSet;

mod common;

#[test]
fn resolve_bash() {
    let provider = common::load_cs10_provider();
    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["bash"], &ResolveOptions::new()).unwrap();
    let set: BTreeSet<String> = result
        .iter()
        .map(|s| solver.provider().display_solvable(*s).to_string())
        .collect();

    assert!(
        set.iter().any(|s| s.starts_with("bash ")),
        "bash must be in the result"
    );
    assert!(
        set.iter().any(|s| s.starts_with("glibc ")),
        "glibc must be in the result"
    );
}

#[test]
fn resolve_dnf() {
    let provider = common::load_cs10_provider();
    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["dnf"], &ResolveOptions::new()).unwrap();
    let set: BTreeSet<String> = result
        .iter()
        .map(|s| solver.provider().display_solvable(*s).to_string())
        .collect();

    assert!(
        set.iter().any(|s| s.starts_with("dnf ")),
        "dnf must be in the result"
    );
    assert!(
        set.iter().any(|s| s.starts_with("python3-dnf ")),
        "python3-dnf must be in the result"
    );
}

#[test]
fn resolve_manual_packages() {
    use resolvo_rpm::{DependencySpec, PackageSpec};
    use rpmrepo_metadata::RequirementType;

    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // Package "app" requires "lib"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "app",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[DependencySpec {
                name: "lib",
                flags: Some(RequirementType::GE),
                epoch: None,
                version: Some("1.0"),
                release: None,
                preinstall: false,
            }],
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

    // Package "lib" provides itself
    provider.add_package(
        repo,
        &PackageSpec {
            name: "lib",
            epoch: "0",
            version: "2.0",
            release: "1",
            arch: "x86_64",
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

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["app"], &ResolveOptions::new()).unwrap();
    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();

    assert_eq!(names, BTreeSet::from(["app", "lib"]));
}

#[test]
fn resolve_manual_provides() {
    use resolvo_rpm::{DependencySpec, PackageSpec};
    use rpmrepo_metadata::RequirementType;

    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // Package "app" requires capability "libfoo"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "app",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[DependencySpec {
                name: "libfoo",
                flags: None,
                epoch: None,
                version: None,
                release: None,
                preinstall: false,
            }],
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

    // Package "foo-impl" provides "libfoo" at version 3.0
    provider.add_package(
        repo,
        &PackageSpec {
            name: "foo-impl",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[],
            provides: &[DependencySpec {
                name: "libfoo",
                flags: Some(RequirementType::EQ),
                epoch: None,
                version: Some("3.0"),
                release: None,
                preinstall: false,
            }],
            conflicts: &[],
            obsoletes: &[],
            recommends: &[],
            suggests: &[],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["app"], &ResolveOptions::new()).unwrap();
    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();

    assert_eq!(names, BTreeSet::from(["app", "foo-impl"]));
}

#[test]
fn resolve_unversioned_conflict() {
    use resolvo_rpm::{DependencySpec, PackageSpec};

    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // "app" requires both "lib-a" and "lib-b"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "app",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[
                DependencySpec {
                    name: "lib-a",
                    flags: None,
                    epoch: None,
                    version: None,
                    release: None,
                    preinstall: false,
                },
                DependencySpec {
                    name: "lib-b",
                    flags: None,
                    epoch: None,
                    version: None,
                    release: None,
                    preinstall: false,
                },
            ],
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

    // "lib-a" has an unversioned conflict with "lib-b"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "lib-a",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[],
            provides: &[],
            conflicts: &[DependencySpec {
                name: "lib-b",
                flags: None,
                epoch: None,
                version: None,
                release: None,
                preinstall: false,
            }],
            obsoletes: &[],
            recommends: &[],
            suggests: &[],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );

    provider.add_package(
        repo,
        &PackageSpec {
            name: "lib-b",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
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

    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["app"], &ResolveOptions::new());
    assert!(
        result.is_err(),
        "unversioned conflict should make resolution fail"
    );
}

#[test]
fn resolve_obsoletes_excludes_old_version() {
    use resolvo_rpm::{DependencySpec, PackageSpec};
    use rpmrepo_metadata::RequirementType;

    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // "app" requires "lib"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "app",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[DependencySpec {
                name: "lib",
                flags: None,
                epoch: None,
                version: None,
                release: None,
                preinstall: false,
            }],
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

    // "lib" version 1.0
    provider.add_package(
        repo,
        &PackageSpec {
            name: "lib",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
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

    // "lib" version 2.0
    provider.add_package(
        repo,
        &PackageSpec {
            name: "lib",
            epoch: "0",
            version: "2.0",
            release: "1",
            arch: "x86_64",
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

    // "new-lib" obsoletes lib < 2.0, and "app" also requires "new-lib"
    provider.add_package(
        repo,
        &PackageSpec {
            name: "new-lib",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[],
            provides: &[],
            conflicts: &[],
            obsoletes: &[DependencySpec {
                name: "lib",
                flags: Some(RequirementType::LT),
                epoch: None,
                version: Some("2.0"),
                release: None,
                preinstall: false,
            }],
            recommends: &[],
            suggests: &[],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );

    // Require both app and new-lib — the obsoletes should force lib >= 2.0
    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["app", "new-lib"], &ResolveOptions::new()).unwrap();
    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();

    assert!(names.contains("lib"), "lib should be in the solution");
    assert!(
        names.contains("new-lib"),
        "new-lib should be in the solution"
    );

    // Verify lib 2.0 was selected (not 1.0, which is obsoleted)
    let lib_version = result
        .iter()
        .find(|&&s| solver.provider().package_name(s) == "lib")
        .map(|s| solver.provider().package_version(*s).to_string())
        .unwrap();
    assert_eq!(
        lib_version, "0:2.0-1",
        "lib 2.0 should be selected, not 1.0"
    );
}

#[test]
fn resolve_suggests_off_by_default() {
    use resolvo_rpm::{DependencySpec, PackageSpec};

    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    provider.add_package(
        repo,
        &PackageSpec {
            name: "app",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
            requires: &[],
            provides: &[],
            conflicts: &[],
            obsoletes: &[],
            recommends: &[],
            suggests: &[DependencySpec {
                name: "optional-tool",
                flags: None,
                epoch: None,
                version: None,
                release: None,
                preinstall: false,
            }],
            supplements: &[],
            enhances: &[],
            files: &[],
        },
    );

    provider.add_package(
        repo,
        &PackageSpec {
            name: "optional-tool",
            epoch: "0",
            version: "1.0",
            release: "1",
            arch: "x86_64",
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

    // Default: suggests off
    let mut solver = resolvo::Solver::new(provider);
    let result = resolve(&mut solver, &["app"], &ResolveOptions::new()).unwrap();
    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["app"]),
        "suggests should not be pulled in by default"
    );

    // With suggests enabled
    let result = resolve(
        &mut solver,
        &["app"],
        &ResolveOptions::new().enable_suggests(true),
    )
    .unwrap();
    let names: BTreeSet<&str> = result
        .iter()
        .map(|s| solver.provider().package_name(*s))
        .collect();
    assert_eq!(
        names,
        BTreeSet::from(["app", "optional-tool"]),
        "suggests should be pulled in when enabled"
    );
}
