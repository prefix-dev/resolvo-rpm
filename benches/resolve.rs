use criterion::{Criterion, criterion_group, criterion_main};
use resolvo_rpm::{ClosureOptions, LoadOptions, RpmProvider, resolve};
use std::path::Path;

const ASSETS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/assets/");

fn repo_path(name: &str) -> std::path::PathBuf {
    Path::new(ASSETS).join(name)
}

fn load_cs10_provider() -> RpmProvider {
    let mut provider = RpmProvider::new(Some("x86_64"));
    provider
        .load_repo(&repo_path("cs10-baseos"), "cs10-baseos")
        .unwrap();
    provider
        .load_repo(&repo_path("cs10-appstream"), "cs10-appstream")
        .unwrap();
    provider
}

fn load_cs10_provider_with_filelists() -> RpmProvider {
    let opts = LoadOptions::new().load_filelists(true);
    let mut provider = RpmProvider::new(Some("x86_64"));
    provider
        .load_repo_with_options(&repo_path("cs10-baseos"), "cs10-baseos", &opts)
        .unwrap();
    provider
        .load_repo_with_options(&repo_path("cs10-appstream"), "cs10-appstream", &opts)
        .unwrap();
    provider
}

fn bench_load_repo(c: &mut Criterion) {
    let mut group = c.benchmark_group("load_repo");
    group.sample_size(10);
    group.bench_function("cs10", |b| {
        b.iter(|| load_cs10_provider());
    });
    group.bench_function("cs10_with_filelists", |b| {
        b.iter(|| load_cs10_provider_with_filelists());
    });
    group.finish();
}

fn bench_solve_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("solve_only");

    let provider = load_cs10_provider();
    let mut solver = resolvo::Solver::new(provider);
    let configurations = [
        ("bash", &["bash"][..]),
        ("dnf", &["dnf"][..]),
        ("rpm-build", &["rpm-build"][..]),
    ];

    for (label, pkgs) in configurations {
        group.bench_function(label, |b| {
            b.iter(|| resolve(&mut solver, pkgs, &Default::default()).unwrap());
        });
    }

    group.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");
    group.sample_size(10);
    let configurations = [
        ("bash", &["bash"][..]),
        ("dnf", &["dnf"][..]),
        ("rpm-build", &["rpm-build"][..]),
    ];

    for (label, pkgs) in configurations {
        group.bench_function(label, |b| {
            b.iter(|| {
                let provider = load_cs10_provider();
                let mut solver = resolvo::Solver::new(provider);
                resolve(&mut solver, pkgs, &Default::default()).unwrap()
            });
        });
    }

    group.finish();
}

fn bench_check_closure(c: &mut Criterion) {
    let mut group = c.benchmark_group("check_closure");
    group.sample_size(10);

    group.bench_function("cs10", |b| {
        b.iter(|| {
            let provider = load_cs10_provider_with_filelists();
            provider.check_closure(&ClosureOptions::default())
        });
    });

    // Solve-only: measure just the closure check with pre-loaded repos
    let provider = load_cs10_provider_with_filelists();
    group.bench_function("cs10_check_only", |b| {
        b.iter(|| provider.check_closure(&ClosureOptions::default()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_load_repo,
    bench_solve_only,
    bench_end_to_end,
    bench_check_closure
);
criterion_main!(benches);
