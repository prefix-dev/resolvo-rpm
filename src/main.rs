use resolvo::DefaultSolvableDisplay;
use rpmrepo_metadata::{RepositoryReader, Requirement};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use url::Url;

use clap::Parser;

mod rpm_fetch;
mod rpm_provider;

use rpm_provider::{RPMProvider, RPMRequirement};

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

#[derive(Debug, Parser)]
struct Args {
    #[clap(long, default_value = "./fedora")]
    target_folder: PathBuf,

    #[clap(required = true)]
    packages: Vec<String>,

    #[clap(long)]
    disable_suggest: bool,
}

fn main() {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    if args.packages.is_empty() {
        println!("No packages specified. Add some on the command line.");
        return;
    }

    let target_folder = args.target_folder;

    // let url = Url::parse("https://mirrors.xtom.de/fedora/updates/38/Everything/x86_64/").unwrap();
    // fetch_repodata(url, &target_folder);

    let url =
        Url::parse("https://mirrors.xtom.de/fedora/releases/38/Everything/x86_64/os/").unwrap();
    rpm_fetch::fetch_repodata(url, &target_folder);

    // print_pkgs(&target_folder);

    let provider = RPMProvider::from_repodata(&target_folder, args.disable_suggest);
    println!("Provider created ...");
    let mut solver = resolvo::Solver::new(provider);

    let mut specs = Vec::new();
    for pkg in args.packages {
        let spec = RPMRequirement(Requirement {
            name: pkg.to_string(),
            flags: Some("GT".into()),
            epoch: Some(0.to_string()),
            version: Some("0.0.0".into()),
            ..Default::default()
        });
        println!("Resolving for: {}", spec);
        let name_id = solver.pool().intern_package_name(pkg);
        let spec_id = solver.pool().intern_version_set(name_id, spec);

        specs.push(spec_id);
    }

    let solvables = match solver.solve(specs) {
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
