use resolvo_rpm::{ClosureOptions, DependencySpec, PackageSpec, RequirementType, RpmProvider};

mod common;

fn pkg(name: &str, version: &str) -> PackageSpec<'static> {
    // Leak strings so we get 'static lifetimes — fine for tests.
    let name = String::from(name).leak() as &'static str;
    let version = String::from(version).leak() as &'static str;
    PackageSpec {
        name,
        epoch: "0",
        version,
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
    }
}

fn dep(name: &str) -> DependencySpec<'static> {
    let name = String::from(name).leak() as &'static str;
    DependencySpec {
        name,
        flags: None,
        epoch: None,
        version: None,
        release: None,
        preinstall: false,
    }
}

fn dep_versioned(name: &str, flags: RequirementType, version: &str) -> DependencySpec<'static> {
    let name = String::from(name).leak() as &'static str;
    let version = String::from(version).leak() as &'static str;
    DependencySpec {
        name,
        flags: Some(flags),
        epoch: None,
        version: Some(version),
        release: None,
        preinstall: false,
    }
}

#[test]
fn closure_all_satisfied() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep("lib")];
    app.requires = &reqs;
    provider.add_package(repo, &app);
    provider.add_package(repo, &pkg("lib", "2.0"));

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_missing_dep() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep("missing-lib")];
    app.requires = &reqs;
    provider.add_package(repo, &app);

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert_eq!(unsatisfied.len(), 1);

    let (sid, _vs_id) = unsatisfied[0];
    assert_eq!(provider.package_name(sid), "app");
}

#[test]
fn closure_versioned_satisfied() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep_versioned("lib", RequirementType::GE, "2.0")];
    app.requires = &reqs;
    provider.add_package(repo, &app);
    provider.add_package(repo, &pkg("lib", "3.0"));

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_versioned_unsatisfied() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep_versioned("lib", RequirementType::GE, "5.0")];
    app.requires = &reqs;
    provider.add_package(repo, &app);
    // lib 3.0 is too old for the >= 5.0 requirement
    provider.add_package(repo, &pkg("lib", "3.0"));

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert_eq!(unsatisfied.len(), 1);
    assert_eq!(provider.package_name(unsatisfied[0].0), "app");
}

#[test]
fn closure_virtual_provides() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep("webclient")];
    app.requires = &reqs;
    provider.add_package(repo, &app);

    // "browser" provides the virtual capability "webclient"
    let mut browser = pkg("browser", "1.0");
    let provs = [dep("webclient")];
    browser.provides = &provs;
    provider.add_package(repo, &browser);

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_file_dep() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep("/usr/bin/python3")];
    app.requires = &reqs;
    provider.add_package(repo, &app);

    // "python" ships the file
    let mut python = pkg("python3", "3.12");
    let files: &[&str] = &["/usr/bin/python3"];
    python.files = files;
    provider.add_package(repo, &python);

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

/// check_closure is shallow — it only checks that a direct provider
/// exists for each requirement, not that the provider's own deps are
/// satisfiable. Here A→B is satisfied, B→C is not, but only B→C
/// should be reported.
#[test]
fn closure_is_shallow() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut a = pkg("a", "1.0");
    let a_reqs = [dep("b")];
    a.requires = &a_reqs;
    provider.add_package(repo, &a);

    let mut b = pkg("b", "1.0");
    let b_reqs = [dep("c")];
    b.requires = &b_reqs;
    provider.add_package(repo, &b);

    // "c" is not in the repo

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert_eq!(unsatisfied.len(), 1);
    assert_eq!(
        provider.package_name(unsatisfied[0].0),
        "b",
        "only b's missing dep on c should be reported"
    );
}

#[test]
fn closure_multiple_versions_any_satisfies() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    let mut app = pkg("app", "1.0");
    let reqs = [dep_versioned("lib", RequirementType::GE, "2.0")];
    app.requires = &reqs;
    provider.add_package(repo, &app);

    // lib 1.0 doesn't satisfy >= 2.0, but lib 3.0 does
    provider.add_package(repo, &pkg("lib", "1.0"));
    provider.add_package(repo, &pkg("lib", "3.0"));

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_empty_repo() {
    let provider = RpmProvider::new(None);
    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_no_deps() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");
    provider.add_package(repo, &pkg("standalone", "1.0"));

    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_real_repos() {
    let provider = common::load_cs10_provider();
    let unsatisfied = provider.check_closure(&ClosureOptions::default());
    // These are partial test repos so some deps will be missing.
    // Just verify it runs and returns a non-trivial result.
    assert!(
        !unsatisfied.is_empty(),
        "partial test repos should have some unsatisfied deps"
    );
}

#[test]
fn closure_check_repos_scoped() {
    let mut provider = RpmProvider::new(None);
    let repo_a = provider.add_repo("a");
    let repo_b = provider.add_repo("b");

    // repo_a: "app" requires "lib" (which is missing from both repos)
    let mut app = pkg("app", "1.0");
    let reqs = [dep("lib")];
    app.requires = &reqs;
    provider.add_package(repo_a, &app);

    // repo_b: "tool" requires "missing" (also missing)
    let mut tool = pkg("tool", "1.0");
    let tool_reqs = [dep("missing")];
    tool.requires = &tool_reqs;
    provider.add_package(repo_b, &tool);

    // Check only repo_a — should only report app's broken dep
    let opts = ClosureOptions::new().check_repos(vec![repo_a]);
    let unsatisfied = provider.check_closure(&opts);
    assert_eq!(unsatisfied.len(), 1);
    assert_eq!(provider.package_name(unsatisfied[0].0), "app");
}

#[test]
fn closure_check_repos_cross_satisfy() {
    let mut provider = RpmProvider::new(None);
    let repo_a = provider.add_repo("a");
    let repo_b = provider.add_repo("b");

    // repo_a: "app" requires "lib"
    let mut app = pkg("app", "1.0");
    let reqs = [dep("lib")];
    app.requires = &reqs;
    provider.add_package(repo_a, &app);

    // repo_b: "lib" satisfies the dep (cross-repo)
    provider.add_package(repo_b, &pkg("lib", "2.0"));

    // Check only repo_a — lib from repo_b should satisfy app's dep
    let opts = ClosureOptions::new().check_repos(vec![repo_a]);
    let unsatisfied = provider.check_closure(&opts);
    assert!(unsatisfied.is_empty());
}

#[test]
fn closure_newest_only_skips_old_versions() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // "app" 1.0 requires "old-lib" (missing)
    let mut app_old = pkg("app", "1.0");
    let old_reqs = [dep("old-lib")];
    app_old.requires = &old_reqs;
    provider.add_package(repo, &app_old);

    // "app" 2.0 has no deps — all satisfied
    provider.add_package(repo, &pkg("app", "2.0"));

    // Without newest_only: app 1.0's broken dep is reported
    let all = provider.check_closure(&ClosureOptions::default());
    assert_eq!(all.len(), 1);

    // With newest_only: only app 2.0 is checked, which has no deps
    let opts = ClosureOptions::new().newest_only(true);
    let newest = provider.check_closure(&opts);
    assert!(newest.is_empty());
}

#[test]
fn closure_newest_only_reports_newest_broken() {
    let mut provider = RpmProvider::new(None);
    let repo = provider.add_repo("test");

    // "app" 1.0 has no deps
    provider.add_package(repo, &pkg("app", "1.0"));

    // "app" 2.0 requires "new-lib" (missing)
    let mut app_new = pkg("app", "2.0");
    let reqs = [dep("new-lib")];
    app_new.requires = &reqs;
    provider.add_package(repo, &app_new);

    let opts = ClosureOptions::new().newest_only(true);
    let unsatisfied = provider.check_closure(&opts);
    assert_eq!(unsatisfied.len(), 1);
    assert_eq!(provider.package_name(unsatisfied[0].0), "app");
}

#[test]
fn closure_newest_and_check_repos_combined() {
    let mut provider = RpmProvider::new(None);
    let repo_a = provider.add_repo("a");
    let repo_b = provider.add_repo("b");

    // repo_a: "app" 1.0 requires "missing-old" (broken)
    let mut app_old = pkg("app", "1.0");
    let old_reqs = [dep("missing-old")];
    app_old.requires = &old_reqs;
    provider.add_package(repo_a, &app_old);

    // repo_a: "app" 2.0 — no deps
    provider.add_package(repo_a, &pkg("app", "2.0"));

    // repo_b: "tool" requires "missing-tool" (broken)
    let mut tool = pkg("tool", "1.0");
    let tool_reqs = [dep("missing-tool")];
    tool.requires = &tool_reqs;
    provider.add_package(repo_b, &tool);

    // newest_only + check only repo_a: should check only app 2.0, which is clean
    let opts = ClosureOptions::new()
        .check_repos(vec![repo_a])
        .newest_only(true);
    let unsatisfied = provider.check_closure(&opts);
    assert!(unsatisfied.is_empty());
}
