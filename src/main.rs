use resolvo::SolvableId;
use resolvo_rpm::fetch::{FetchConfig, fetch_repodata};
use resolvo_rpm::{
    ClosureOptions, CompsGroup, GroupInstallOptions, LoadOptions, ResolveOptions, RpmProvider,
    UpdateRecord, resolve,
};
use rpm_version::Evr;
use std::{collections::BTreeSet, path::PathBuf, process};
use url::Url;

use clap::{Parser, Subcommand};

/// Resolve RPM package dependencies using a SAT solver.
///
/// Takes one or more local repodata directories and a list of packages
/// to resolve dependencies for, then prints the full dependency closure.
#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Resolve the full dependency closure for the given packages.
    Resolve {
        /// Paths to local repodata directories (must contain repodata/repomd.xml).
        /// Can be specified multiple times. Repos listed first have higher priority.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Package names, @group-ids, or patch:ADVISORY-IDs to resolve.
        /// Group names must be prefixed with @ (e.g. @core, @development).
        /// Advisory IDs must be prefixed with patch: (e.g. patch:RHSA-2024:1234).
        #[clap(required = true)]
        packages: Vec<String>,

        /// Target architecture. Only packages matching this arch (and noarch)
        /// will be considered. If omitted, all architectures are included.
        #[clap(long)]
        arch: Option<String>,

        /// Include optional packages within groups. By default, only mandatory
        /// and default packages are installed (matching dnf behavior).
        #[clap(long)]
        with_optional: bool,

        /// Disable Recommends. By default, recommended packages are installed
        /// if available (matching dnf behavior). This flag skips them entirely.
        #[clap(long)]
        disable_recommends: bool,

        /// Enable Suggests. By default, suggested packages are not installed
        /// (matching dnf behavior). This flag includes them as soft requirements.
        #[clap(long)]
        enable_suggests: bool,
    },
    /// List or inspect advisories (updateinfo/errata).
    Advisory {
        #[clap(subcommand)]
        action: AdvisoryAction,
    },
    /// Check that all dependencies within the repos are satisfiable.
    Depclose {
        /// Paths to local repodata directories (must contain repodata/repomd.xml).
        /// Can be specified multiple times.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Only check packages from these repos for unsatisfied deps.
        /// Other repos are still used to satisfy dependencies.
        /// Must be a subset of --repo paths.
        #[clap(long)]
        check: Vec<PathBuf>,

        /// Only check the newest version of each package.
        #[clap(long)]
        newest: bool,

        /// Target architecture. Only packages matching this arch (and noarch)
        /// will be considered. If omitted, all architectures are included.
        #[clap(long)]
        arch: Option<String>,
    },
    /// Download RPM repository metadata from a remote URL.
    Fetch {
        /// URL of the RPM repository (e.g. https://example.com/repo/).
        url: String,

        /// Local directory to store the downloaded repodata.
        #[clap(long, default_value = ".")]
        dest: PathBuf,

        /// Authentication token appended as a URL query parameter.
        #[clap(long)]
        auth_token: Option<String>,

        /// Trust environment variables for proxy configuration
        /// (HTTP_PROXY, HTTPS_PROXY, etc.).
        #[clap(long)]
        allow_env_var: bool,

        /// Custom User-Agent string.
        #[clap(long)]
        user_agent: Option<String>,

        /// Disable TLS certificate verification.
        #[clap(long)]
        insecure: bool,

        /// Path to a TLS client certificate (PEM format).
        #[clap(long)]
        tls_client_cert: Option<PathBuf>,

        /// Path to a TLS client private key (PEM format).
        #[clap(long)]
        tls_client_key: Option<PathBuf>,

        /// Path to a CA certificate bundle (PEM format).
        #[clap(long)]
        tls_ca_cert: Option<PathBuf>,

        /// Download all metadata files listed in repomd.xml, not just the
        /// standard set (primary, filelists, other, updateinfo, comps).
        #[clap(long)]
        all: bool,
    },
    /// List or inspect comps package groups.
    Group {
        #[clap(subcommand)]
        action: GroupAction,
    },
    /// List or inspect comps environments.
    Environment {
        #[clap(subcommand)]
        action: EnvironmentAction,
    },
    /// Query packages by their provides or requires.
    Repoquery {
        #[clap(subcommand)]
        action: RepoqueryAction,
    },
}

#[derive(Debug, Subcommand)]
enum GroupAction {
    /// List all available groups.
    List {
        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
    /// Show details for a specific group.
    Info {
        /// Group ID or name.
        spec: String,

        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum AdvisoryAction {
    /// List advisories matching the given filters. Filters are combined with AND.
    List {
        /// Paths to local repodata directories (must contain repodata/repomd.xml).
        /// Can be specified multiple times.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture for filtering packages.
        #[clap(long)]
        arch: Option<String>,

        /// Filter advisories by affected package name.
        #[clap(long)]
        package: Option<String>,

        /// Filter advisories by CVE reference.
        #[clap(long)]
        cve: Option<String>,

        /// Filter advisories by type (security, bugfix, enhancement).
        #[clap(long = "type")]
        advisory_type: Option<String>,

        /// Filter advisories by severity (Critical, Important, Moderate, Low).
        #[clap(long)]
        severity: Option<String>,
    },
    /// Show full detail for a single advisory by ID.
    Info {
        /// Advisory ID (e.g. RHSA-2024:1234).
        id: String,

        /// Paths to local repodata directories (must contain repodata/repomd.xml).
        /// Can be specified multiple times.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture for filtering packages.
        #[clap(long)]
        arch: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RepoqueryAction {
    /// Find packages that provide the given capability.
    ///
    /// The capability can be a package name, a virtual provide (e.g.
    /// python(abi)), or a file path (e.g. /usr/bin/python3). File paths
    /// trigger loading of filelists.xml for complete results.
    Whatprovides {
        /// Capability name or file path to search for.
        capability: String,

        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
    /// Find packages that require the given capability.
    Whatrequires {
        /// Capability name to search for.
        capability: String,

        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum EnvironmentAction {
    /// List all available environments.
    List {
        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
    /// Show details for a specific environment.
    Info {
        /// Environment ID or name.
        spec: String,

        /// Paths to local repodata directories.
        #[clap(long, required = true)]
        repo: Vec<PathBuf>,

        /// Target architecture.
        #[clap(long)]
        arch: Option<String>,
    },
}

fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    match args.command {
        Command::Resolve {
            repo,
            packages,
            arch,
            with_optional,
            disable_recommends,
            enable_suggests,
        } => cmd_resolve(
            &repo,
            &packages,
            arch.as_deref(),
            with_optional,
            disable_recommends,
            enable_suggests,
        ),
        Command::Fetch {
            url,
            dest,
            auth_token,
            allow_env_var,
            user_agent,
            insecure,
            tls_client_cert,
            tls_client_key,
            tls_ca_cert,
            all,
        } => {
            let base_url = Url::parse(&url).unwrap_or_else(|e| {
                eprintln!("Invalid URL: {}", e);
                process::exit(1);
            });
            let config = FetchConfig {
                auth_token,
                allow_env_var,
                user_agent,
                verify_tls: !insecure,
                tls_client_cert,
                tls_client_key,
                tls_ca_cert,
                all,
            };
            if let Err(e) = fetch_repodata(base_url, &dest, &config) {
                eprintln!("Error fetching repodata: {}", e);
                process::exit(1);
            }
        }
        Command::Advisory { action } => match action {
            AdvisoryAction::List {
                repo,
                arch,
                package,
                cve,
                advisory_type,
                severity,
            } => cmd_advisory_list(
                &repo,
                arch.as_deref(),
                package.as_deref(),
                cve.as_deref(),
                advisory_type.as_deref(),
                severity.as_deref(),
            ),
            AdvisoryAction::Info { id, repo, arch } => {
                cmd_advisory_info(&id, &repo, arch.as_deref())
            }
        },
        Command::Depclose {
            repo,
            check,
            newest,
            arch,
        } => cmd_depclose(&repo, &check, newest, arch.as_deref()),
        Command::Group { action } => match action {
            GroupAction::List { repo, arch } => cmd_group_list(&repo, arch.as_deref()),
            GroupAction::Info { spec, repo, arch } => cmd_group_info(&spec, &repo, arch.as_deref()),
        },
        Command::Environment { action } => match action {
            EnvironmentAction::List { repo, arch } => cmd_environment_list(&repo, arch.as_deref()),
            EnvironmentAction::Info { spec, repo, arch } => {
                cmd_environment_info(&spec, &repo, arch.as_deref())
            }
        },
        Command::Repoquery { action } => match action {
            RepoqueryAction::Whatprovides {
                capability,
                repo,
                arch,
            } => cmd_whatprovides(&capability, &repo, arch.as_deref()),
            RepoqueryAction::Whatrequires {
                capability,
                repo,
                arch,
            } => cmd_whatrequires(&capability, &repo, arch.as_deref()),
        },
    }
}

/// Resolve the full dependency closure for a set of named packages via the
/// SAT solver, then print the result as an aligned table to stdout.
///
/// Exits with status 1 if resolution fails (e.g. missing or conflicting deps).
fn cmd_resolve(
    repos: &[PathBuf],
    packages: &[String],
    arch: Option<&str>,
    with_optional: bool,
    disable_recommends: bool,
    enable_suggests: bool,
) {
    let has_groups = packages.iter().any(|p| p.starts_with('@'));
    let has_advisories = packages.iter().any(|p| p.starts_with("patch:"));

    let mut load_options = LoadOptions::new()
        .load_groups(has_groups)
        .load_advisories(has_advisories);

    if with_optional {
        load_options =
            load_options.group_options(GroupInstallOptions::new().include_optional(true));
    }

    let provider = load_provider(repos, arch, &load_options);

    let mut solver = resolvo::Solver::new(provider);
    let pkg_names: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
    let options = ResolveOptions::new()
        .enable_recommends(!disable_recommends)
        .enable_suggests(enable_suggests);

    let solvables = match resolve(&mut solver, &pkg_names, &options) {
        Ok(s) => s,
        Err(err) => report_error(&solver, err),
    };

    print_resolution(&solver, &solvables);
}

/// Load an [`RpmProvider`] from one or more local repodata directories.
///
/// Exits with status 1 if any repo fails to load.
fn load_provider(repos: &[PathBuf], arch: Option<&str>, options: &LoadOptions) -> RpmProvider {
    let mut provider = RpmProvider::new(arch);
    for repo_path in repos {
        let repo_label = &repo_path.display().to_string();
        if let Err(e) = provider.load_repo_with_options(repo_path, repo_label, options) {
            eprintln!("Error loading repo {}: {}", repo_path.display(), e);
            process::exit(1);
        }
    }
    provider
}

/// List advisories matching the given filters, printed as an aligned table.
///
/// Filters (package, CVE, type, severity) are combined with AND.
fn cmd_advisory_list(
    repos: &[PathBuf],
    arch: Option<&str>,
    package: Option<&str>,
    cve: Option<&str>,
    advisory_type: Option<&str>,
    severity: Option<&str>,
) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_advisories(true));

    let mut results: Vec<&UpdateRecord> = provider.advisories().iter().collect();

    if let Some(pkg) = package {
        let pkg_set: std::collections::HashSet<&str> = provider
            .advisories_for_package(pkg)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        results.retain(|a| pkg_set.contains(a.id.as_str()));
    }

    if let Some(cve_id) = cve {
        let cve_set: std::collections::HashSet<&str> = provider
            .advisories_by_cve(cve_id)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        results.retain(|a| cve_set.contains(a.id.as_str()));
    }

    if let Some(atype) = advisory_type {
        results.retain(|a| a.update_type == atype);
    }

    if let Some(sev) = severity {
        results.retain(|a| a.severity.as_deref() == Some(sev));
    }

    if results.is_empty() {
        eprintln!("No advisories found.");
        return;
    }

    let id_width = column_width(results.iter().map(|a| a.id.len()));
    let type_width = column_width(results.iter().map(|a| a.update_type.len()));
    let sev_width = column_width(
        results
            .iter()
            .map(|a| a.severity.as_deref().unwrap_or("").len()),
    );

    for advisory in &results {
        let sev = advisory.severity.as_deref().unwrap_or("");
        println!(
            "{:<iw$}  {:<tw$}  {:<sw$}  {}",
            advisory.id,
            advisory.update_type,
            sev,
            advisory.title,
            iw = id_width,
            tw = type_width,
            sw = sev_width,
        );
    }

    eprintln!("\n{} advisories", results.len());
}

/// Print full detail for a single advisory, looked up by ID.
fn cmd_advisory_info(id: &str, repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_advisories(true));
    let advisory = unwrap_or_exit(
        provider.advisory_by_id(id),
        &format!("Advisory not found: {}", id),
    );
    print_advisory_detail(advisory);
}

/// Print full detail for a single advisory.
fn print_advisory_detail(advisory: &UpdateRecord) {
    let sev = advisory.severity.as_deref().unwrap_or("None");
    println!("{}  {}  {}", advisory.id, advisory.update_type, sev);
    println!("{}", advisory.title);

    if let Some(date) = &advisory.issued_date {
        println!("Issued:  {}", date);
    }
    if let Some(date) = &advisory.updated_date {
        println!("Updated: {}", date);
    }

    let cve_refs: Vec<_> = advisory
        .references
        .iter()
        .filter(|r| r.reftype == "cve")
        .collect();
    if !cve_refs.is_empty() {
        println!("\nReferences:");
        let id_width = column_width(cve_refs.iter().map(|r| r.id.as_deref().unwrap_or("").len()));
        for r in &cve_refs {
            let ref_id = r.id.as_deref().unwrap_or("");
            println!("  {:<w$}  {}", ref_id, r.href, w = id_width);
        }
    }

    let packages: Vec<_> = advisory.pkglist.iter().flat_map(|c| &c.packages).collect();
    if !packages.is_empty() {
        println!("\nPackages:");
        let name_width = column_width(packages.iter().map(|p| p.name.len()));
        let evrs: Vec<String> = packages
            .iter()
            .map(|p| Evr::new(&p.epoch, &p.version, &p.release).to_string())
            .collect();
        let evr_width = column_width(evrs.iter().map(|e| e.len()));
        for (p, evr) in packages.iter().zip(&evrs) {
            println!(
                "  {:<nw$}  {:<ew$}  {}",
                p.name,
                evr,
                p.arch,
                nw = name_width,
                ew = evr_width,
            );
        }
    }
}

/// Check dependency closure: verify that every Requires of every package
/// in the repo set can be satisfied by at least one package in the same set.
///
/// This is a shallow check (no SAT solving) — it reports individual
/// broken dependencies, not transitive unsatisfiability. Analogous to
/// `dnf repoclosure`.
///
/// When `check_repos` is non-empty, only packages from those repos are
/// checked for broken deps - other repos still contribute providers.
///
/// When `newest` is true, only the highest-EVR version of each package
/// (per name+arch) is checked.
///
/// Prints unsatisfied deps to stdout as an aligned table, sorted by
/// package name. Exits with status 1 if any are found.
fn cmd_depclose(repos: &[PathBuf], check_repos: &[PathBuf], newest: bool, arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new());

    // Map --check paths to repo_ids by matching against the --repo list.
    let check_repo_ids: Vec<usize> = check_repos
        .iter()
        .map(|check_path| {
            repos
                .iter()
                .position(|r| r == check_path)
                .unwrap_or_else(|| {
                    eprintln!(
                        "Error: --check path {:?} is not in the --repo list",
                        check_path
                    );
                    process::exit(2);
                })
        })
        .collect();

    let options = ClosureOptions::new()
        .check_repos(check_repo_ids)
        .newest_only(newest);
    let unsatisfied = provider.check_closure(&options);

    if unsatisfied.is_empty() {
        eprintln!("All dependencies satisfied.");
        return;
    }

    // Dedup via BTreeSet: the same (package, version, requirement) triple can
    // appear multiple times when a versioned requirement is interned identically
    // across packages, or when multiple versions of a package share the same
    // broken dep. The BTreeSet also gives us sorted output for free.
    let mut problems: BTreeSet<(String, String, String)> = BTreeSet::new();
    for &(sid, vs_id) in &unsatisfied {
        let pkg_name = provider.package_name(sid);
        let pkg_version = provider.package_version(sid).to_string();
        let req_name = provider
            .pool
            .resolve_package_name(provider.pool.resolve_version_set_package_name(vs_id));
        let vs = provider.pool.resolve_version_set(vs_id);
        let ver_str = vs.version.map(|id| provider.pool.resolve_string(id));
        let req_display = match (vs.flags, ver_str) {
            (Some(flags), Some(ver)) => format!("{} {} {}", req_name, flags.as_operator(), ver),
            _ => req_name.to_string(),
        };
        problems.insert((pkg_name.to_string(), pkg_version, req_display));
    }

    let pkg_width = column_width(problems.iter().map(|(name, _, _)| name.len()));
    let ver_width = column_width(problems.iter().map(|(_, ver, _)| ver.len()));

    for (pkg, ver, req) in &problems {
        println!(
            "{:<pw$}  {:<vw$}  requires: {}",
            pkg,
            ver,
            req,
            pw = pkg_width,
            vw = ver_width,
        );
    }

    eprintln!("\n{} unsatisfied dependencies", problems.len());
    process::exit(1);
}

/// List all comps groups as an aligned ID/name table.
fn cmd_group_list(repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_groups(true));
    let mut groups: Vec<&CompsGroup> = provider.groups().iter().collect();
    groups.sort_by(|a, b| a.id.cmp(&b.id));

    if groups.is_empty() {
        eprintln!("No groups found.");
        return;
    }

    let id_width = column_width(groups.iter().map(|g| g.id.len()));
    for group in &groups {
        println!("{:<w$}  {}", group.id, group.name, w = id_width);
    }
    eprintln!("\n{} groups", groups.len());
}

/// Print detail for a single comps group, looked up by ID or name.
fn cmd_group_info(spec: &str, repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_groups(true));
    let group = unwrap_or_exit(
        provider
            .groups()
            .iter()
            .find(|g| g.id == spec || g.name == spec),
        &format!("Group not found: {}", spec),
    );

    print_header(&group.id, &group.name, &group.description);

    let types = ["mandatory", "default", "optional", "conditional"];
    for reqtype in &types {
        let pkgs: Vec<&str> = group
            .packages
            .iter()
            .filter(|p| p.reqtype == *reqtype)
            .map(|p| p.name.as_str())
            .collect();
        if pkgs.is_empty() {
            continue;
        }
        println!("\n{}:", reqtype);
        for pkg in &pkgs {
            println!("  {}", pkg);
        }
    }
}

/// List all comps environments as an aligned ID/name table.
fn cmd_environment_list(repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_groups(true));
    let mut envs: Vec<_> = provider.environments().iter().collect();
    envs.sort_by(|a, b| a.id.cmp(&b.id));

    if envs.is_empty() {
        eprintln!("No environments found.");
        return;
    }

    let id_width = column_width(envs.iter().map(|e| e.id.len()));
    for env in &envs {
        println!("{:<w$}  {}", env.id, env.name, w = id_width);
    }
    eprintln!("\n{} environments", envs.len());
}

/// Print detail for a single comps environment, looked up by ID or name.
fn cmd_environment_info(spec: &str, repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new().load_groups(true));
    let env = unwrap_or_exit(
        provider
            .environments()
            .iter()
            .find(|e| e.id == spec || e.name == spec),
        &format!("Environment not found: {}", spec),
    );

    print_header(&env.id, &env.name, &env.description);

    if !env.group_ids.is_empty() {
        println!("\nmandatory groups:");
        for gid in &env.group_ids {
            println!("  {}", gid);
        }
    }

    if !env.option_ids.is_empty() {
        println!("\noptional groups:");
        for opt in &env.option_ids {
            if opt.default {
                println!("  {} [default]", opt.group_id);
            } else {
                println!("  {}", opt.group_id);
            }
        }
    }
}

/// Find and print all packages that provide the given capability.
fn cmd_whatprovides(capability: &str, repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new());
    let solvables = provider.whatprovides(capability);

    if solvables.is_empty() {
        eprintln!("No packages provide '{}'.", capability);
        return;
    }

    let count = print_solvable_table(&provider, &solvables);
    eprintln!("\n{} packages", count);
}

/// Find and print all packages that have a dependency on the given capability.
fn cmd_whatrequires(capability: &str, repos: &[PathBuf], arch: Option<&str>) {
    let provider = load_provider(repos, arch, &LoadOptions::new());
    let solvables = provider.whatrequires(capability);

    if solvables.is_empty() {
        eprintln!("No packages require '{}'.", capability);
        return;
    }

    let count = print_solvable_table(&provider, &solvables);
    eprintln!("\n{} packages", count);
}

/// Print solvables as an aligned name/version/repo table, sorted and
/// deduplicated. Returns the number of unique rows printed.
fn print_solvable_table(provider: &RpmProvider, solvables: &[SolvableId]) -> usize {
    let rows: BTreeSet<(String, String, &str)> = solvables
        .iter()
        .map(|&s| {
            let name = provider.package_name(s).to_string();
            let version = provider.package_version(s).to_string();
            let repo = provider.repo_label(s);
            (name, version, repo)
        })
        .collect();

    let name_width = column_width(rows.iter().map(|(name, _, _)| name.len()));
    let ver_width = column_width(rows.iter().map(|(_, ver, _)| ver.len()));
    for (name, version, repo) in &rows {
        println!(
            "{:<nw$}  {:<vw$}  [{}]",
            name,
            version,
            repo,
            nw = name_width,
            vw = ver_width
        );
    }
    rows.len()
}

/// Print the resolved packages in alphabetical order, aligned in columns,
/// with repo labels and a total count on stderr. Virtual solvables
/// (advisories, groups) are listed separately from real packages.
fn print_resolution(solver: &resolvo::Solver<RpmProvider>, solvables: &[resolvo::SolvableId]) {
    let provider = solver.provider();

    let mut packages = Vec::new();
    let mut advisories = Vec::new();
    let mut groups = Vec::new();

    for &sid in solvables {
        let name = provider.package_name(sid);
        if name.starts_with("patch:") {
            advisories.push(sid);
        } else if name.starts_with('@') {
            groups.push(sid);
        } else {
            packages.push(sid);
        }
    }

    let pkg_count = print_solvable_table(provider, &packages);

    if !advisories.is_empty() {
        let labels: Vec<&str> = advisories
            .iter()
            .map(|&s| provider.package_name(s).strip_prefix("patch:").unwrap())
            .collect();
        eprintln!("\nAdvisories: {}", labels.join(", "));
    }

    if !groups.is_empty() {
        let labels: Vec<&str> = groups.iter().map(|&s| provider.package_name(s)).collect();
        eprintln!("\nGroups: {}", labels.join(", "));
    }

    eprintln!("\n{} packages resolved", pkg_count);
}

/// Return the maximum value in an iterator, for column-width calculation.
fn column_width(widths: impl Iterator<Item = usize>) -> usize {
    widths.max().unwrap_or(0)
}

/// Unwrap an `Option`, or print a message to stderr and exit with status 1.
fn unwrap_or_exit<T>(option: Option<T>, message: &str) -> T {
    match option {
        Some(v) => v,
        None => {
            eprintln!("{}", message);
            process::exit(1);
        }
    }
}

/// Print a standard ID/Name/Description header block.
fn print_header(id: &str, name: &str, description: &str) {
    println!("ID:          {}", id);
    println!("Name:        {}", name);
    if !description.is_empty() {
        println!("Description: {}", description);
    }
}

/// Print the solver error to stderr and exit with status 1.
fn report_error(solver: &resolvo::Solver<RpmProvider>, err: resolvo::UnsolvableOrCancelled) -> ! {
    match err {
        resolvo::UnsolvableOrCancelled::Unsolvable(conflict) => {
            eprintln!("Error: {}", conflict.display_user_friendly(solver));
        }
        resolvo::UnsolvableOrCancelled::Cancelled(_) => {
            eprintln!("Cancelled");
        }
    }
    process::exit(1);
}
