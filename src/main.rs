use reqwest::blocking::Client;
use resolvo::{
    Candidates, DefaultSolvableDisplay, Dependencies, DependencyProvider, NameId, Pool, SolvableId,
    SolverCache, VersionSet,
};
use rpmrepo_metadata::{RepositoryReader, Requirement};
use std::{
    collections::{BTreeSet, HashMap},
    fmt::Display,
    hash::Hash,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Default, Debug, Clone)]
struct RPMPackageVersion {
    package: String,
    version: String,
    epoch: i32,
    requires: Vec<Requirement>,
    // provides: Vec<Requirement>,
}

#[derive(Debug, Clone)]
struct RPMRequirement(Requirement);

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
        write!(f, "{}", self.version)?;
        if self.epoch != 0 {
            write!(f, " ({})", self.epoch)?;
        }
        Ok(())
    }
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
}

impl VersionSet for RPMRequirement {
    type V = RPMPackageVersion;

    fn contains(&self, other: &Self::V) -> bool {
        let v_package = &other.version;
        let cmp = self.to_cmp();
        if cmp.is_none() || self.0.version.is_none() {
            return true;
        }

        let v_test = self.0.version.as_deref().unwrap();

        println!("Comparing: {} {:?} {}", v_package, cmp.unwrap(), v_test);

        version_compare::compare_to(v_package, v_test, cmp.unwrap()).unwrap()
    }
}

#[derive(Default)]
struct RPMProvider {
    pool: Pool<RPMRequirement>,
    provides_to_package: HashMap<String, Vec<SolvableId>>,
}

impl RPMProvider {
    pub fn from_repodata(path: &Path) -> Self {
        let reader = RepositoryReader::new_from_directory(path).unwrap();

        let pool = Pool::default();
        let mut provides_to_package = HashMap::new();

        for pkg in reader.iter_packages().unwrap() {
            let pkg = pkg.unwrap();
            let name = pkg.name().to_string();
            let version = pkg.version().to_string();
            let epoch = pkg.epoch();
            let provides = pkg.provides();
            let requires = pkg.requires();
            let pack = RPMPackageVersion {
                package: name.clone(),
                version: version.clone(),
                epoch,
                requires: requires.to_vec(),
            };

            let name_id = pool.intern_package_name(&name);
            let solvable = pool.intern_solvable(name_id, pack.clone());

            for p in provides {
                println!("{} provides {}", name, p.name);

                let provides = provides_to_package
                    .entry(p.name.clone())
                    .or_insert_with(Vec::new);
                provides.push(solvable);
            }
        }

        Self {
            pool,
            provides_to_package,
        }
    }
}

impl DependencyProvider<RPMRequirement> for RPMProvider {
    fn pool(&self) -> &Pool<RPMRequirement> {
        &self.pool
    }

    fn sort_candidates(
        &self,
        _solver: &SolverCache<RPMRequirement, String, Self>,
        solvables: &mut [SolvableId],
    ) {
        solvables.sort_by(|a, b| {
            let a = self.pool.resolve_solvable(*a).inner();
            let b = self.pool.resolve_solvable(*b).inner();

            if a.epoch != b.epoch {
                return a.epoch.cmp(&b.epoch);
            }

            version_compare::compare(&a.version, &b.version)
                .unwrap()
                .ord()
                .unwrap()
        });
    }

    fn get_candidates(&self, name: NameId) -> Option<Candidates> {
        let package_name = self.pool.resolve_package_name(name);
        let _package = self.provides_to_package.get(package_name)?;
        let candidates = match self.provides_to_package.get(package_name) {
            Some(candidates) => candidates.clone(),
            None => Vec::default(),
        };
        let mut candidates = Candidates {
            candidates,
            ..Candidates::default()
        };

        candidates.hint_dependencies_available = candidates.candidates.clone();

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

        Some(candidates)
    }

    fn get_dependencies(&self, solvable: SolvableId) -> Dependencies {
        let candidate = self.pool.resolve_solvable(solvable);
        let _package_name = self.pool.resolve_package_name(candidate.name_id());
        let pack = candidate.inner();

        let requirements = &pack.requires;

        let mut result = Dependencies::default();

        for req in requirements {
            if req.name.starts_with('/') || req.name.contains(" if ") {
                continue;
            };
            let dep_name = self.pool.intern_package_name(&req.name);
            let dep_spec = self
                .pool
                .intern_version_set(dep_name, RPMRequirement(req.clone()));
            result.requirements.push(dep_spec);
        }

        result
    }
}

fn fetch_repodata(base_url: Url, target_folder: &Path) {
    let client = Client::new();

    // check if the repomd.xml exists
    if target_folder.join("repodata/repomd.xml").exists() {
        println!("repomd.xml already exists");
        return;
    }

    let url = base_url.join("repodata/repomd.xml").unwrap();
    let mut resp = client.get(url).send().unwrap();

    let path = target_folder.to_path_buf();
    std::fs::create_dir_all(path.join("repodata")).unwrap();
    let mut file = std::fs::File::create(path.join("repodata/repomd.xml")).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let reader = RepositoryReader::new_from_directory(target_folder).unwrap();
    let repomd = reader.repomd();
    // download the other files
    let data = repomd.get_filelist_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_other_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();

    let data = repomd.get_primary_data();
    let url = base_url
        .join(&data.location_href.to_string_lossy())
        .unwrap();
    let mut resp = client.get(url).send().unwrap();
    let mut file = std::fs::File::create(target_folder.join(&data.location_href)).unwrap();
    std::io::copy(&mut resp, &mut file).unwrap();
}

#[allow(dead_code)]
fn print_pkgs(path: &Path) {
    let reader = RepositoryReader::new_from_directory(path).unwrap();

    for pkg in reader.iter_packages().unwrap() {
        let pkg = pkg.unwrap();
        println!(
            "{}-{}-{}-{}",
            pkg.name(),
            pkg.version(),
            pkg.release(),
            pkg.arch()
        );

        println!("Provides:   {:?}", pkg.provides());
        println!("Requires:   {:?}", pkg.requires());
        println!("Recommends: {:?}", pkg.recommends());
        println!("Suggests:   {:?}", pkg.suggests());
        println!("Conflicts:  {:?}", pkg.conflicts());
    }
}

fn main() {
    tracing_subscriber::fmt::init();

    let target_folder = PathBuf::from("./fedora");
    // let url = Url::parse("https://mirrors.xtom.de/fedora/updates/38/Everything/x86_64/").unwrap();
    // fetch_repodata(url, &target_folder);

    let url =
        Url::parse("https://mirrors.xtom.de/fedora/releases/38/Everything/x86_64/os/").unwrap();
    fetch_repodata(url, &target_folder);

    // print_pkgs(&target_folder);

    let provider = RPMProvider::from_repodata(&target_folder);
    println!("Provider created ...");
    let mut solver = resolvo::Solver::new(provider);
    let name = "rust";
    let spec = RPMRequirement(Requirement {
        name: name.to_string(),
        flags: Some("GT".into()),
        epoch: Some(0.to_string()),
        version: Some("0.0.0".into()),
        ..Default::default()
    });
    println!("Resolving for: {}", spec);
    let name_id = solver.pool().intern_package_name(name);
    let spec_id = solver.pool().intern_version_set(name_id, spec);

    let candidates = solver.pool().resolve_package_name(name_id);
    println!("Candidates: {:?}", candidates);
    let requirements = vec![spec_id];
    let solvables = match solver.solve(requirements) {
        Ok(solvables) => solvables,
        Err(problem) => {
            println!(
                "Error: {}",
                problem.display_user_friendly(&solver, &DefaultSolvableDisplay)
            );
            return;
        }
    };

    let resolved: BTreeSet<String> = solvables
        .iter()
        .map(|s| s.display(solver.pool()).to_string())
        .collect();

    println!("Resolved:\n");

    for r in resolved {
        println!("- {}", r);
    }
}
