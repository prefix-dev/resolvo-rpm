use resolvo::{
    utils::{Pool, VersionSet},
    Candidates, Condition, ConditionId, ConditionalRequirement, Dependencies, DependencyProvider,
    HintDependenciesAvailable, Interner, KnownDependencies, NameId,
    Requirement as ResolvoRequirement, SolvableId, SolverCache, StringId, VersionSetId,
    VersionSetUnionId,
};
use rpmrepo_metadata::{RepositoryReader, Requirement};
use std::{collections::HashMap, fmt::Display, hash::Hash, path::Path};

#[derive(Default, Debug, Clone)]
pub struct RPMPackageVersion {
    pub package: String,
    pub version: String,
    pub epoch: u32,
    pub requires: Vec<Requirement>,
    pub suggests: Vec<Requirement>,
}

#[derive(Debug, Clone)]
pub struct RPMRequirement(pub Requirement);

impl PartialEq for RPMRequirement {
    fn eq(&self, other: &Self) -> bool {
        self.0.name == other.0.name
            && self.0.version == other.0.version
            && self.0.flags == other.0.flags
    }
}

impl Eq for RPMRequirement {}

impl Hash for RPMRequirement {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.name.hash(state);
        self.0.version.hash(state);
        self.0.flags.hash(state);
    }
}

impl Display for RPMRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let req = &self.0;
        write!(
            f,
            "{}-{}",
            req.flags.as_ref().unwrap_or(&"UNDEF".to_string()),
            req.version.as_ref().unwrap_or(&"UNDEF".to_string())
        )
    }
}

impl PartialEq for RPMPackageVersion {
    fn eq(&self, other: &Self) -> bool {
        self.package == other.package && self.version == other.version
    }
}

impl std::cmp::Eq for RPMPackageVersion {}

impl PartialOrd for RPMPackageVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RPMPackageVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self.epoch != other.epoch {
            return self.epoch.cmp(&other.epoch);
        }

        version_compare::compare(&self.version, &other.version)
            .unwrap()
            .ord()
            .unwrap()
    }
}

impl Display for RPMPackageVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.package, self.version)?;
        if self.epoch != 0 {
            write!(f, " ({})", self.epoch)?;
        }
        Ok(())
    }
}

impl VersionSet for RPMRequirement {
    type V = RPMPackageVersion;
}

impl RPMRequirement {
    fn to_cmp(&self) -> Option<version_compare::Cmp> {
        match self.0.flags.as_deref().unwrap_or("") {
            "EQ" => Some(version_compare::Cmp::Eq),
            "GT" => Some(version_compare::Cmp::Gt),
            "GE" => Some(version_compare::Cmp::Ge),
            "LT" => Some(version_compare::Cmp::Lt),
            "LE" => Some(version_compare::Cmp::Le),
            "NE" => Some(version_compare::Cmp::Ne),
            _ => None,
        }
    }

    fn matches(&self, candidate: &RPMPackageVersion) -> bool {
        let cmp = self.to_cmp();
        if cmp.is_none() || self.0.version.is_none() {
            return true;
        }

        let v_test = self.0.version.as_deref().unwrap();
        version_compare::compare_to(&candidate.version, v_test, cmp.unwrap()).unwrap_or(false)
    }
}

#[derive(Default)]
pub struct RPMProvider {
    pub pool: Pool<RPMRequirement>,
    pub provides_to_package: HashMap<String, Vec<SolvableId>>,
    // todo: this should disable individual rules / requirements
    pub disable_suggest: bool,
}

impl RPMProvider {
    pub fn from_repodata(path: &Path, disable_suggest: bool) -> Self {
        let reader = RepositoryReader::new_from_directory(path).unwrap();

        let pool: Pool<RPMRequirement> = Pool::default();
        let mut provides_to_package: HashMap<String, Vec<SolvableId>> = HashMap::new();

        for pkg in reader.iter_packages().unwrap() {
            let pkg = pkg.unwrap();

            let pack = RPMPackageVersion {
                package: pkg.name().to_string(),
                version: pkg.version().to_string(),
                epoch: pkg.epoch(),
                requires: pkg.requires().to_vec(),
                suggests: pkg.suggests().to_vec(),
            };

            let name_id = pool.intern_package_name(pkg.name());
            let solvable = pool.intern_solvable(name_id, pack.clone());

            for p in pkg.provides() {
                provides_to_package
                    .entry(p.name.clone())
                    .or_default()
                    .push(solvable);
            }
        }

        Self {
            pool,
            provides_to_package,
            disable_suggest,
        }
    }

    /// Build a top-level requirement asking for any version of `pkg` (epoch 0,
    /// version > 0.0.0), mirroring the original CLI behavior.
    pub fn root_requirement(&self, pkg: &str) -> ConditionalRequirement {
        let name_id = self.pool.intern_package_name(pkg);
        let vs_id = self.pool.intern_version_set(
            name_id,
            RPMRequirement(Requirement {
                name: pkg.to_string(),
                flags: Some("GT".into()),
                epoch: Some(0.to_string()),
                version: Some("0.0.0".into()),
                ..Default::default()
            }),
        );
        ResolvoRequirement::Single(vs_id).into()
    }
}

impl Interner for RPMProvider {
    fn display_solvable(&self, solvable: SolvableId) -> impl Display + '_ {
        self.pool.resolve_solvable(solvable).record.clone()
    }

    fn display_name(&self, name: NameId) -> impl Display + '_ {
        self.pool.resolve_package_name(name).clone()
    }

    fn display_version_set(&self, version_set: VersionSetId) -> impl Display + '_ {
        self.pool.resolve_version_set(version_set).clone()
    }

    fn display_string(&self, string_id: StringId) -> impl Display + '_ {
        self.pool.resolve_string(string_id).to_owned()
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

    fn resolve_condition(&self, _condition: ConditionId) -> Condition {
        unreachable!("conditional requirements are not used by this provider")
    }
}

impl DependencyProvider for RPMProvider {
    async fn filter_candidates(
        &self,
        candidates: &[SolvableId],
        version_set: VersionSetId,
        inverse: bool,
    ) -> Vec<SolvableId> {
        let vs = self.pool.resolve_version_set(version_set);
        candidates
            .iter()
            .copied()
            .filter(|s| {
                let record = &self.pool.resolve_solvable(*s).record;
                vs.matches(record) != inverse
            })
            .collect()
    }

    async fn sort_candidates(&self, _solver: &SolverCache<Self>, solvables: &mut [SolvableId]) {
        solvables.sort_by(|a, b| {
            let a = &self.pool.resolve_solvable(*a).record;
            let b = &self.pool.resolve_solvable(*b).record;

            if a.epoch != b.epoch {
                return b.epoch.cmp(&a.epoch);
            }

            // Highest version first.
            version_compare::compare(&b.version, &a.version)
                .unwrap()
                .ord()
                .unwrap()
        });
    }

    async fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let package_name = self.pool.resolve_package_name(name);
        let solvables = self.provides_to_package.get(package_name)?;

        let candidates = Candidates {
            candidates: solvables.clone(),
            hint_dependencies_available: HintDependenciesAvailable::All,
            ..Candidates::default()
        };

        Some(candidates)
    }

    async fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let record = &self.pool.resolve_solvable(solvable).record;

        let mut result = KnownDependencies::default();

        for req in &record.requires {
            if req.name.starts_with('/') || req.name.contains(" if ") {
                continue;
            }
            let dep_name = self.pool.intern_package_name(&req.name);
            let dep_spec = self
                .pool
                .intern_version_set(dep_name, RPMRequirement(req.clone()));
            result.requirements.push(dep_spec.into());
        }

        if !self.disable_suggest {
            for req in &record.suggests {
                if req.name.starts_with('/') || req.name.contains(" if ") {
                    continue;
                }
                let dep_name = self.pool.intern_package_name(&req.name);
                let dep_spec = self
                    .pool
                    .intern_version_set(dep_name, RPMRequirement(req.clone()));
                result.requirements.push(dep_spec.into());
            }
        }

        Dependencies::Known(result)
    }
}
