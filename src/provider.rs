use resolvo::{
    Candidates, Condition, ConditionId, ConditionalRequirement, DenseIndex, Dependencies,
    DependencyProvider, HintDependenciesAvailable, Interner, KnownDependencies, NameId, Problem,
    Requirement as ResolvoRequirement, SolvableId, SolverCache, StringId, UnsolvableOrCancelled,
    VersionSetId, VersionSetUnionId,
    utils::{Pool, VersionSet},
};
use rpm_version::Evr;
use rpmrepo_metadata::RequirementType;
use rpmrepo_metadata::{CompsEnvironment, CompsGroup, UpdateRecord};
use std::{cell::RefCell, cmp::Ordering, fmt::Display, hash::Hash, path::PathBuf};

use crate::{ResolveOptions, options::*};

pub(crate) type HashMap<K, V> = ahash::AHashMap<K, V>;

/// A dense map from [`NameId`] to `Vec<SolvableId>`, backed by a flat `Vec`
/// indexed by `NameId.to_usize()`. Provides O(1) lookups with no hashing.
#[derive(Default, Debug, Clone)]
pub struct ProvidesMap(Vec<Vec<SolvableId>>);

impl ProvidesMap {
    /// Look up the solvables that provide a given capability, or `None` if
    /// no package provides it.
    pub(crate) fn get(&self, id: NameId) -> Option<&Vec<SolvableId>> {
        self.0.get(id.to_index()).filter(|v| !v.is_empty())
    }

    /// Return a mutable reference to the solvable list for a capability,
    /// growing the backing vec (with power-of-two sizing) if needed.
    pub(crate) fn entry(&mut self, id: NameId) -> &mut Vec<SolvableId> {
        let idx = id.to_index();
        if idx >= self.0.len() {
            self.0.resize_with((idx + 1).next_power_of_two(), Vec::new);
        }
        &mut self.0[idx]
    }
}

/// The version-set record stored for each solvable in the resolvo pool.
///
/// Contains the package's identity (NEVRA), the repo it came from, and all
/// dependency lists parsed from primary.xml. Provides are not stored here —
/// they are indexed separately in [`RpmProvider::provides_to_package`].
#[derive(Default, Debug, Clone)]
pub struct RpmPackageVersion {
    pub name: String,
    pub epoch: String,
    pub version: String,
    pub release: String,
    pub arch: String,
    pub repo_id: usize,
    pub requires: Vec<VersionSetId>,
    pub conflicts: Vec<VersionSetId>,
    pub obsoletes: Vec<VersionSetId>,
    pub recommends: Vec<VersionSetId>,
    pub suggests: Vec<VersionSetId>,
    pub supplements: Vec<VersionSetId>,
    pub enhances: Vec<VersionSetId>,
}

impl RpmPackageVersion {
    /// Construct a borrowed EVR for version comparison.
    fn evr(&self) -> Evr<'_> {
        Evr::new(
            self.epoch.as_str(),
            self.version.as_str(),
            self.release.as_str(),
        )
    }

    /// Construct a virtual package solvable record with version `0:1.0-1.noarch`.
    ///
    /// Used for groups (`@core`), environments (`@minimal-environment`), and
    /// advisories (`patch:RHSA-2024:1234`). Callers override individual fields
    /// (e.g. `requires`, `conflicts`, `version`) after construction.
    pub(crate) fn virtual_package(name: String, repo_id: usize) -> Self {
        Self {
            name,
            repo_id,
            epoch: "0".to_owned(),
            version: "1.0".to_owned(),
            release: "1".to_owned(),
            arch: "noarch".to_owned(),
            ..Default::default()
        }
    }
}

/// A version-set type for RPM requirements, stored in the resolvo [`Pool`].
///
/// All string fields are pool-interned [`StringId`]s rather than owned strings,
/// making this type `Copy` and eliminating heap allocation during interning.
/// Two requirements are considered equal (and hash the same) when they share
/// the same flags and version — the requirement name is already captured by the
/// pool's `NameId` and is not duplicated here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RpmRequirement {
    pub flags: Option<RequirementType>,
    pub epoch: Option<StringId>,
    pub version: Option<StringId>,
    pub release: Option<StringId>,
    pub preinstall: bool,
}

/// A dependency entry for the programmatic package-addition API.
///
/// Used for Requires, Provides, Conflicts, and Recommends in [`PackageSpec`].
/// For Provides entries, `flags` and `preinstall` are ignored.
///
/// Re-exported from `rpmrepo_metadata::visitor::RequirementData`.
pub use rpmrepo_metadata::visitor::RequirementData as DependencySpec;

/// A complete package description for manual addition to the solver pool.
///
/// Construct this and pass it to [`RpmProvider::add_package()`] to add
/// packages without loading repository metadata files.
#[derive(Debug, Clone)]
pub struct PackageSpec<'a> {
    pub name: &'a str,
    pub epoch: &'a str,
    pub version: &'a str,
    pub release: &'a str,
    pub arch: &'a str,
    pub requires: &'a [DependencySpec<'a>],
    pub provides: &'a [DependencySpec<'a>],
    pub conflicts: &'a [DependencySpec<'a>],
    pub obsoletes: &'a [DependencySpec<'a>],
    pub recommends: &'a [DependencySpec<'a>],
    pub suggests: &'a [DependencySpec<'a>],
    pub supplements: &'a [DependencySpec<'a>],
    pub enhances: &'a [DependencySpec<'a>],
    pub files: &'a [&'a str],
}

impl PartialEq for RpmPackageVersion {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.evr() == other.evr()
    }
}

impl std::cmp::Eq for RpmPackageVersion {}

impl PartialOrd for RpmPackageVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RpmPackageVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.evr().cmp(&other.evr())
    }
}

impl Display for RpmPackageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.evr())
    }
}

/// Test whether a candidate EVR satisfies a version constraint.
fn check_version_constraint(flags: RequirementType, req: &Evr<'_>, candidate: &Evr<'_>) -> bool {
    let ord = candidate.cmp(req);
    match flags {
        RequirementType::EQ => ord == Ordering::Equal,
        RequirementType::LT => ord == Ordering::Less,
        RequirementType::GT => ord == Ordering::Greater,
        RequirementType::LE => ord == Ordering::Less || ord == Ordering::Equal,
        RequirementType::GE => ord == Ordering::Greater || ord == Ordering::Equal,
    }
}

/// Invert a version comparison operator for conflict modeling.
///
/// Used when converting RPM Conflicts into resolvo's `constrains` mechanism.
/// EQ maps to LT as an approximation since RPM has no NE operator (either in
/// librpm or in the XML metadata), and neither does libsolv.
pub(crate) fn invert_flags(flags: RequirementType) -> RequirementType {
    match flags {
        RequirementType::EQ => RequirementType::LT,
        RequirementType::LT => RequirementType::GE,
        RequirementType::GT => RequirementType::LE,
        RequirementType::LE => RequirementType::GT,
        RequirementType::GE => RequirementType::LT,
    }
}

impl VersionSet for RpmRequirement {
    type V = RpmPackageVersion;
}

/// The version at which a solvable provides a specific capability.
///
/// RPM packages can provide capabilities at versions different from their own
/// EVR. For example, `python3-3.12.9` provides `python(abi) = 3.12`. When a
/// requirement like `Requires: python(abi) = 3.12` is checked, the comparison
/// must use this provides version, not the package's own EVR.
#[derive(Clone, Debug)]
pub struct ProvidesVersion {
    pub epoch: String,
    pub version: String,
    pub release: String,
}

/// RPM dependency provider for the resolvo SAT solver.
///
/// Loads RPM repository metadata and implements [`DependencyProvider`] so that
/// resolvo can resolve package dependencies. Supports multiple repos (with
/// priority ordering), architecture filtering, versioned Provides, file
/// dependencies (with lazy filelists.xml parsing), Conflicts, and Recommends.
///
/// # Usage
///
/// 1. Create with [`RpmProvider::new()`]
/// 2. Load one or more repos with [`RpmProvider::load_repo()`] (or
///    [`RpmProvider::load_repo_with_options()`])
/// 3. Pass to [`resolvo::Solver::new()`], then call [`resolve()`]
pub struct RpmProvider {
    /// The resolvo interning pool. Stores all solvables and interned strings.
    pub pool: Pool<RpmRequirement>,
    /// Maps capability [`NameId`] to the solvables that provide it.
    pub provides_to_package: RefCell<ProvidesMap>,
    /// Maps `(SolvableId, NameId)` to the version at which that solvable
    /// provides a capability. Only populated for explicit Provides entries
    /// with version info; missing entries fall back to the package EVR.
    pub(crate) provides_versions: HashMap<(SolvableId, NameId), ProvidesVersion>,
    /// Human-readable labels for each loaded repo, in load order.
    pub repo_labels: Vec<String>,
    /// Target architecture for filtering. When set, only packages matching
    /// this arch or `noarch` are loaded. Applied uniformly to all repos.
    pub(crate) target_arch: Option<String>,
    pub(crate) filelists_paths: Vec<PathBuf>,
    pub(crate) filelists_loaded: RefCell<bool>,
    /// All loaded advisories, in insertion order.
    pub(crate) advisories: Vec<UpdateRecord>,
    /// Maps advisory ID (e.g. "RHSA-2024:1234") to its index in `advisories`.
    pub(crate) advisory_id_to_index: HashMap<String, usize>,
    /// Maps package name to the indices of advisories that reference it.
    pub(crate) advisory_pkg_index: HashMap<String, Vec<usize>>,
    /// Maps CVE ID to the indices of advisories that reference it.
    pub(crate) advisory_cve_index: HashMap<String, Vec<usize>>,
    /// All loaded comps groups, in insertion order.
    pub(crate) groups: Vec<CompsGroup>,
    /// All loaded comps environments, in insertion order.
    pub(crate) environments: Vec<CompsEnvironment>,
}

impl RpmProvider {
    /// Create an empty provider. Call [`load_repo()`](Self::load_repo) to add
    /// package metadata before solving.
    ///
    /// If `target_arch` is set, only packages whose arch matches that value
    /// or `noarch` are loaded from each repo. This significantly reduces pool
    /// size and avoids cross-arch conflicts. Pass `None` to load all
    /// architectures.
    pub fn new(target_arch: Option<&str>) -> Self {
        Self {
            pool: Pool::default(),
            provides_to_package: RefCell::new(ProvidesMap::default()),
            provides_versions: HashMap::new(),
            repo_labels: Vec::new(),
            target_arch: target_arch.map(|s| s.to_string()),
            filelists_paths: Vec::new(),
            filelists_loaded: RefCell::new(false),
            advisories: Vec::new(),
            advisory_id_to_index: HashMap::new(),
            advisory_pkg_index: HashMap::new(),
            advisory_cve_index: HashMap::new(),
            groups: Vec::new(),
            environments: Vec::new(),
        }
    }

    /// Collect candidate solvable IDs for weak deps of the given resolved packages.
    ///
    /// For each resolved solvable, reads the dependency list returned by `f`,
    /// finds candidate solvables that provide each capability, and returns the
    /// deduplicated list. The caller can pass these to `Problem::soft_requirements`
    /// so the solver tries to include them without failing if they're unsatisfiable.
    #[inline(always)]
    fn collect_soft_dep_solvables(
        &self,
        resolved: &[SolvableId],
        f: impl Fn(&RpmPackageVersion) -> &[VersionSetId],
    ) -> Vec<SolvableId> {
        let provides_map = self.provides_to_package.borrow();
        let mut soft = Vec::new();

        for &sid in resolved {
            let pack = &self.pool.resolve_solvable(sid).record;
            for &vs_id in f(pack) {
                let req_name_id = self.pool.resolve_version_set_package_name(vs_id);
                if let Some(candidates) = provides_map.get(req_name_id) {
                    for &candidate in candidates {
                        if self.version_set_contains(vs_id, candidate) {
                            soft.push(candidate);
                        }
                    }
                }
            }
        }

        soft.sort();
        soft.dedup();
        soft
    }

    /// Collect candidate solvable IDs for all Recommends of the given resolved packages.
    pub fn collect_recommended_solvables(&self, resolved: &[SolvableId]) -> Vec<SolvableId> {
        self.collect_soft_dep_solvables(resolved, |pack| &pack.recommends)
    }

    /// Collect candidate solvable IDs for all Suggests of the given resolved packages.
    pub fn collect_suggested_solvables(&self, resolved: &[SolvableId]) -> Vec<SolvableId> {
        self.collect_soft_dep_solvables(resolved, |pack| &pack.suggests)
    }

    /// Check the dependency closure of packages in the loaded repos.
    ///
    /// For every package selected by `options`, checks whether each of its
    /// Requires can be satisfied by at least one package in the full repo set.
    /// Returns a list of unsatisfied dependencies as
    /// `(package_solvable_id, requirement_version_set_id)` pairs.
    ///
    /// Use [`ClosureOptions`] to narrow the scope:
    /// - `check_repos`: only check packages from the listed repos (all repos
    ///   remain available for satisfying deps)
    /// - `newest_only`: only check the newest version of each (name, arch) pair
    ///
    /// This is a *shallow* check: for each requirement, it only verifies that
    /// at least one candidate provider exists and matches the version constraint.
    /// It does NOT perform full transitive resolution (no SAT solving), so it
    /// won't detect cases where a provider exists but its own dependencies are
    /// unsatisfiable. This matches the behavior of tools like `repoclosure`.
    ///
    /// Rich/boolean dependencies (e.g. `(foo or bar)`) are skipped during
    /// loading and will not be checked here.
    ///
    /// Filelists are loaded eagerly so that file-path dependencies (e.g.
    /// `Requires: /usr/bin/python3`) are checked against the full file index,
    /// not just the subset in primary.xml.
    pub fn check_closure(&self, options: &ClosureOptions) -> Vec<(SolvableId, VersionSetId)> {
        self.load_filelists();

        let provides_map = self.provides_to_package.borrow();

        let solvables_to_check: Vec<(SolvableId, &RpmPackageVersion)> = if options.newest_only {
            // Group by (NameId, arch), keeping only the highest-EVR solvable.
            let mut newest: HashMap<(NameId, &str), (SolvableId, &RpmPackageVersion)> =
                HashMap::new();
            for (sid, solvable) in self.pool.iter_solvables() {
                let record = &solvable.record;
                if !options.check_repos.is_empty() && !options.check_repos.contains(&record.repo_id)
                {
                    continue;
                }
                let key = (solvable.name, record.arch.as_str());
                newest
                    .entry(key)
                    .and_modify(|(prev_sid, prev_record)| {
                        if record > *prev_record {
                            *prev_sid = sid;
                            *prev_record = record;
                        }
                    })
                    .or_insert((sid, record));
            }
            newest.into_values().collect()
        } else {
            self.pool
                .iter_solvables()
                .filter(|(_, solvable)| {
                    options.check_repos.is_empty()
                        || options.check_repos.contains(&solvable.record.repo_id)
                })
                .map(|(sid, solvable)| (sid, &solvable.record))
                .collect()
        };

        let mut unsatisfied = Vec::new();
        for (sid, record) in &solvables_to_check {
            for &vs_id in &record.requires {
                let req_name_id = self.pool.resolve_version_set_package_name(vs_id);
                let satisfied = provides_map.get(req_name_id).is_some_and(|candidates| {
                    candidates
                        .iter()
                        .any(|&cand| self.version_set_contains(vs_id, cand))
                });
                if !satisfied {
                    unsatisfied.push((*sid, vs_id));
                }
            }
        }

        unsatisfied
    }

    /// Return the package name for a resolved solvable (e.g. "bash").
    pub fn package_name(&self, solvable: SolvableId) -> &str {
        let record = &self.pool.resolve_solvable(solvable).record;
        &record.name
    }

    /// Return the version (EVR) for a resolved solvable (e.g. "0:5.2.26-4.el10").
    pub fn package_version(&self, solvable: SolvableId) -> impl Display + '_ {
        let record = &self.pool.resolve_solvable(solvable).record;
        record.evr()
    }

    /// Return the human-readable repo label for a resolved solvable.
    pub fn repo_label(&self, solvable: SolvableId) -> &str {
        let record = &self.pool.resolve_solvable(solvable).record;
        &self.repo_labels[record.repo_id]
    }

    /// Return all loaded advisories.
    pub fn advisories(&self) -> &[UpdateRecord] {
        &self.advisories
    }

    /// Look up an advisory by its ID (e.g. "RHSA-2024:1234").
    pub fn advisory_by_id(&self, id: &str) -> Option<&UpdateRecord> {
        self.advisory_id_to_index
            .get(id)
            .map(|&idx| &self.advisories[idx])
    }

    /// Return all advisories that reference the given package name.
    pub fn advisories_for_package(&self, name: &str) -> Vec<&UpdateRecord> {
        self.advisory_pkg_index
            .get(name)
            .map(|indices| indices.iter().map(|&i| &self.advisories[i]).collect())
            .unwrap_or_default()
    }

    /// Return all advisories of the given type (e.g. "security", "bugfix", "enhancement").
    pub fn advisories_by_type(&self, update_type: &str) -> Vec<&UpdateRecord> {
        self.advisories
            .iter()
            .filter(|a| a.update_type == update_type)
            .collect()
    }

    /// Return all advisories that reference the given CVE ID (e.g. "CVE-2024-1234").
    pub fn advisories_by_cve(&self, cve_id: &str) -> Vec<&UpdateRecord> {
        self.advisory_cve_index
            .get(cve_id)
            .map(|indices| indices.iter().map(|&i| &self.advisories[i]).collect())
            .unwrap_or_default()
    }

    /// Return all loaded comps groups.
    pub fn groups(&self) -> &[CompsGroup] {
        &self.groups
    }

    /// Return all loaded comps environments.
    pub fn environments(&self) -> &[CompsEnvironment] {
        &self.environments
    }

    /// Find all solvables that provide the given capability.
    ///
    /// The capability can be a package name, a virtual provide (e.g.
    /// `python(abi)`), or a file path (e.g. `/usr/bin/python3`). File paths
    /// trigger lazy loading of filelists.xml.
    pub fn whatprovides(&self, capability: &str) -> Vec<SolvableId> {
        if capability.starts_with('/') {
            self.load_filelists();
        }

        let name_id = match self.pool.lookup_package_name(&capability.to_string()) {
            Some(id) => id,
            None => return Vec::new(),
        };

        let provides_map = self.provides_to_package.borrow();
        provides_map.get(name_id).cloned().unwrap_or_default()
    }

    /// Find all solvables that require the given capability.
    ///
    /// Scans every solvable's Requires list and returns those that have at
    /// least one requirement matching the given capability name.
    pub fn whatrequires(&self, capability: &str) -> Vec<SolvableId> {
        let name_id = match self.pool.lookup_package_name(&capability.to_string()) {
            Some(id) => id,
            None => return Vec::new(),
        };

        let mut result = Vec::new();
        for (sid, solvable) in self.pool.iter_solvables() {
            let record = &solvable.record;
            let has_dep = record
                .requires
                .iter()
                .any(|&vs_id| self.pool.resolve_version_set_package_name(vs_id) == name_id);
            if has_dep {
                result.push(sid);
            }
        }
        result
    }

    /// Check whether a solvable satisfies a version set (requirement).
    ///
    /// If the solvable has an explicit provides version for the capability
    /// named by `version_set`, that version is used for the comparison.
    /// Otherwise, the package's own EVR is used.
    fn version_set_contains(&self, version_set: VersionSetId, solvable: SolvableId) -> bool {
        let vs = self.pool.resolve_version_set(version_set);

        let flags = match vs.flags {
            Some(f) => f,
            None => return true,
        };
        let req_version = match vs.version {
            Some(id) => self.pool.resolve_string(id),
            None => return true,
        };

        let record = &self.pool.resolve_solvable(solvable).record;
        let capability_name_id = self.pool.resolve_version_set_package_name(version_set);

        let cand_evr = if let Some(pv) = self.provides_versions.get(&(solvable, capability_name_id))
        {
            Evr::new(&pv.epoch, &pv.version, &pv.release)
        } else {
            record.evr()
        };

        let req_evr = Evr::new(
            vs.epoch
                .map(|id| self.pool.resolve_string(id))
                .unwrap_or(""),
            req_version,
            vs.release
                .map(|id| self.pool.resolve_string(id))
                .unwrap_or(""),
        );

        check_version_constraint(flags, &req_evr, &cand_evr)
    }
}

/// Build install requirements for the given package names.
///
/// Creates a `Requires: name > 0:0.0.0` entry for each name, which matches
/// any version of the package. Returns entries ready for
/// [`Problem::requirements()`].
pub fn make_install_requirements(
    pool: &Pool<RpmRequirement>,
    packages: &[&str],
) -> Vec<ConditionalRequirement> {
    packages
        .iter()
        .map(|pkg| {
            let name_id = pool.intern_package_name(*pkg);
            let spec = RpmRequirement {
                flags: Some(RequirementType::GT),
                epoch: Some(pool.intern_string("0")),
                version: Some(pool.intern_string("0.0.0")),
                release: None,
                preinstall: false,
            };
            let spec_id = pool.intern_version_set(name_id, spec);
            ConditionalRequirement {
                condition: None,
                requirement: ResolvoRequirement::Single(spec_id),
            }
        })
        .collect()
}

/// Resolve RPM package dependencies.
///
/// Runs the SAT solver to find the full dependency closure for the given
/// package names. When [`ResolveOptions::enable_recommends`] is true, performs
/// a two-pass solve: first resolving hard dependencies, then re-solving with
/// Recommends added as soft requirements.
///
/// Returns the list of solvable IDs in the solution, or the solver error.
pub fn resolve(
    solver: &mut resolvo::Solver<RpmProvider>,
    packages: &[&str],
    options: &ResolveOptions,
) -> Result<Vec<SolvableId>, UnsolvableOrCancelled> {
    let requirements = make_install_requirements(&solver.provider().pool, packages);

    if options.enable_recommends || options.enable_suggests {
        let hard = solver.solve(Problem::new().requirements(requirements.clone()))?;

        let mut soft = Vec::new();
        if options.enable_recommends {
            soft.extend(solver.provider().collect_recommended_solvables(&hard));
        }
        if options.enable_suggests {
            soft.extend(solver.provider().collect_suggested_solvables(&hard));
        }
        soft.sort();
        soft.dedup();

        if soft.is_empty() {
            return Ok(hard);
        }

        solver.solve(
            Problem::new()
                .requirements(requirements)
                .soft_requirements(soft),
        )
    } else {
        solver.solve(Problem::new().requirements(requirements))
    }
}

/// Interner implementation delegates to the underlying [`Pool`] for all
/// string/name/version-set resolution, formatting them for human-readable
/// solver output (error messages, debug traces).
impl Interner for RpmProvider {
    type NameId = resolvo::NameId;
    type SolvableId = resolvo::SolvableId;

    /// Format as "name epoch:version-release" (e.g. "bash 0:5.2.26-4.el10").
    fn display_solvable(&self, solvable: SolvableId) -> impl Display + '_ {
        let s = self.pool.resolve_solvable(solvable);
        let name = self.pool.resolve_package_name(s.name);
        format!("{} {}", name, s.record)
    }

    fn display_name(&self, name: NameId) -> impl Display + '_ {
        self.pool.resolve_package_name(name)
    }

    /// Format as "operator version" (e.g. ">= 2.17"), or "*" if unversioned.
    fn display_version_set(&self, version_set: VersionSetId) -> impl Display + '_ {
        let vs = self.pool.resolve_version_set(version_set);
        let ver_str = vs.version.map(|id| self.pool.resolve_string(id));
        match (vs.flags, ver_str) {
            (Some(flags), Some(ver)) => format!("{} {}", flags.as_operator(), ver),
            (Some(flags), None) => format!("{} ???", flags.as_operator()),
            (None, Some(ver)) => format!("??? {}", ver),
            (None, None) => "*".to_string(),
        }
    }

    fn display_string(&self, string_id: StringId) -> impl Display + '_ {
        self.pool.resolve_string(string_id)
    }

    fn version_set_name(&self, version_set: VersionSetId) -> NameId {
        self.pool.resolve_version_set_package_name(version_set)
    }

    fn solvable_name(&self, solvable: SolvableId) -> NameId {
        self.pool.resolve_solvable(solvable).name
    }

    fn version_sets_in_union(
        &self,
        version_set_union: VersionSetUnionId,
    ) -> impl Iterator<Item = VersionSetId> {
        self.pool.resolve_version_set_union(version_set_union)
    }

    fn resolve_condition(&self, condition: ConditionId) -> Condition {
        self.pool.resolve_condition(condition).clone()
    }
}

/// DependencyProvider implementation that feeds RPM package metadata to the
/// resolvo SAT solver.
impl DependencyProvider for RpmProvider {
    // type SolvableIdLayout = resolvo::solvable_id::Dense;

    /// Filter candidates to those whose version matches (or doesn't match, if
    /// `inverse`) the given version set constraint.
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        version_set: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        candidates
            .iter()
            .copied()
            .filter(|&s| self.version_set_contains(version_set, s) != inverse)
            .collect()
    }

    /// Sort candidates by EVR so the solver prefers the highest version first.
    async fn sort_candidates(&self, _solver: &SolverCache<Self>, solvables: &mut [SolvableId]) {
        solvables.sort_by(|a, b| {
            let a = &self.pool.resolve_solvable(*a).record;
            let b = &self.pool.resolve_solvable(*b).record;
            a.cmp(b)
        });
    }

    /// Return all solvables that provide the given capability name.
    ///
    /// If no candidates are found and the name looks like a file path (starts
    /// with `/`), lazily loads filelists.xml for all repos before retrying.
    /// This avoids the cost of parsing the large filelists metadata unless a
    /// file dependency is actually needed.
    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        {
            let map = self.provides_to_package.borrow();
            if map.get(name).is_none() && self.pool.resolve_package_name(name).starts_with('/') {
                // Must drop the borrow before load_filelists takes its own borrow.
                drop(map);
                self.load_filelists();
            }
        }

        let provides_map = self.provides_to_package.borrow();
        let candidates = match provides_map.get(name) {
            Some(candidates) => candidates.clone(),
            None => return None,
        };
        let mut result = Candidates {
            candidates,
            ..Candidates::default()
        };

        result.hint_dependencies_available = HintDependenciesAvailable::All;

        // TODO: populate result.favored, result.locked, result.excluded
        // to support version pinning, already-installed preferences, and
        // package exclusion (e.g. --exclude flag).

        // favored: prefer a specific version (e.g. already-installed packages in upgrade scenarios)
        // locked: pin a package to a specific version (prevent upgrades)
        // excluded: block specific packages from being selected (e.g. --exclude flag)

        // let favor = self.favored.get(package_name);
        // let locked = self.locked.get(package_name);
        // let excluded = self.excluded.get(package_name);
        // for pack in package {
        //     let solvable = self.pool.resolve_solvable(*pack);
        //     candidates.candidates.push(solvable);
        //     // if Some(pack) == favor {
        //     //     candidates.favored = Some(solvable);
        //     // }
        //     // if Some(pack) == locked {
        //     //     candidates.locked = Some(solvable);
        //     // }
        //     // if let Some(excluded) = excluded.and_then(|d| d.get(pack)) {
        //     //     candidates
        //     //         .excluded
        //     //         .push((solvable, self.pool.intern_string(excluded)));
        //     // }
        // }

        Some(result)
    }

    /// Return the hard dependencies (Requires, Conflicts) for a solvable.
    ///
    /// Requires are emitted as `requirements`, Conflicts as `constrains` (with
    /// inverted version sets — see `intern_inverted_requirement`). Recommends
    /// and Suggests are weak dependencies handled outside the solver's core
    /// dependency loop (see `collect_recommended_solvables`).
    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let pack = &self.pool.resolve_solvable(solvable).record;
        let mut result = KnownDependencies::default();

        for &vs_id in &pack.requires {
            result.requirements.push(ConditionalRequirement {
                condition: None,
                requirement: ResolvoRequirement::Single(vs_id),
            });
        }

        // Recommends and Suggests are NOT hard requirements.
        // Recommends are handled via soft_requirements at the Problem level
        // (see collect_recommended_solvables). Suggests are not installed by
        // default, matching dnf behavior.

        // Supplements and Enhances are reverse weak dependencies. Their data is
        // parsed and stored, but the reverse index and collection logic are
        // deferred until rich/boolean dependency support is implemented — real
        // repos use boolean expressions almost exclusively for these types.

        for &vs_id in &pack.conflicts {
            result.constrains.push(vs_id);
        }

        // TODO: Obsoletes have different semantics from Conflicts, they indicate
        // that the obsoleting package replaces the obsoleted one during upgrades.
        // This is transaction/installer-level logic, not pure dependency resolution.
        // For now, obsoletes data is not loaded by the solver.

        // Obsoletes are modeled as constrains (same as Conflicts) at the resolver
        // level: if this package is in the solution, the obsoleted versions are
        // excluded. The install/transaction-level replacement semantics are not
        // handled here.
        for &vs_id in &pack.obsoletes {
            result.constrains.push(vs_id);
        }

        Dependencies::Known(result)
    }
}
