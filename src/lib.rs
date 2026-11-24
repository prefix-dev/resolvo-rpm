//! RPM dependency resolver built on the [`resolvo`] SAT solver.
//!
//! This crate provides [`RPMProvider`], a [`DependencyProvider`] implementation
//! that loads RPM repository metadata (via [`rpmrepo_metadata`]) and feeds it
//! to resolvo's SAT solver. It handles Requires, Provides (including versioned
//! provides), Conflicts, Recommends, file dependencies, architecture filtering,
//! and multi-repo priority.
//!
//! # Quick start
//!
//! ```no_run
//! use resolvo_rpm::{RpmProvider, ResolveOptions};
//! use std::path::Path;
//!
//! let mut provider = RpmProvider::new(Some("x86_64"));
//! provider.load_repo(Path::new("repo/"), "my-repo");
//!
//! let mut solver = resolvo::Solver::new(provider);
//! let result = resolvo_rpm::resolve(&mut solver, &["bash"], &ResolveOptions::new());
//! ```

pub mod fetch;
mod loader;
mod options;
mod provider;

pub use options::{
    ClosureOptions, EnvironmentInstallOptions, GroupInstallOptions, LoadOptions, ResolveOptions,
};

pub use provider::{
    DependencySpec, PackageSpec, ProvidesMap, ProvidesVersion, RpmPackageVersion, RpmProvider,
    RpmRequirement, make_install_requirements, resolve,
};

pub use rpmrepo_metadata::RequirementType;
pub use rpmrepo_metadata::{
    CompsEnvironment, CompsEnvironmentOption, CompsGroup, CompsPackageReq, UpdateCollection,
    UpdateCollectionPackage, UpdateRecord, UpdateReference,
};

pub(crate) use provider::{HashMap, invert_flags};
