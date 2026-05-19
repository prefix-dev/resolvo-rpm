use resolvo::{Interner, Problem, Solver, UnsolvableOrCancelled};
use rpmrepo_metadata::RepositoryReader;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use url::Url;

use clap::Parser;

mod rpm_fetch;
mod rpm_provider;

use rpm_provider::RPMProvider;

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

    let url =
        Url::parse("https://mirrors.xtom.de/fedora/releases/38/Everything/x86_64/os/").unwrap();
    rpm_fetch::fetch_repodata(url, &target_folder);

    let provider = RPMProvider::from_repodata(&target_folder, args.disable_suggest);
    println!("Provider created ...");

    let requirements: Vec<_> = args
        .packages
        .iter()
        .map(|pkg| {
            println!("Resolving for: {}", pkg);
            provider.root_requirement(pkg)
        })
        .collect();

    let mut solver = Solver::new(provider);
    let problem = Problem::new().requirements(requirements);

    let solvables = match solver.solve(problem) {
        Ok(solvables) => solvables,
        Err(UnsolvableOrCancelled::Unsolvable(conflict)) => {
            println!("Error: {}", conflict.display_user_friendly(&solver));
            return;
        }
        Err(UnsolvableOrCancelled::Cancelled(_)) => {
            println!("Solver cancelled");
            return;
        }
    };

    let provider = solver.provider();
    let resolved: BTreeSet<String> = solvables
        .iter()
        .map(|s| provider.display_solvable(*s).to_string())
        .collect();

    println!("Resolved:\n");

    for r in resolved {
        println!("- {}", r);
    }
}
