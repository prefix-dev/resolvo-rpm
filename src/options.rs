/// Options controlling repository loading behavior.
///
/// Defaults load only primary.xml, deferring filelists.xml until a file
/// dependency is encountered during resolution. Use the builder methods to
/// customize:
///
/// ```
/// use resolvo_rpm::LoadOptions;
///
/// let opts = LoadOptions::new().load_filelists(true).load_groups(true);
/// ```
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    pub(crate) load_filelists: bool,
    pub(crate) load_groups: bool,
    pub(crate) group_options: GroupInstallOptions,
    pub(crate) environment_options: EnvironmentInstallOptions,
    pub(crate) load_advisories: bool,
}

impl LoadOptions {
    /// Create options with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether filelists.xml should be parsed eagerly during
    /// [`RpmProvider::load_repo()`].
    ///
    /// When false (the default), filelists are loaded lazily on the first
    /// file dependency lookup. When true, filelists are parsed immediately,
    /// front-loading the cost.
    pub fn load_filelists(mut self, load: bool) -> Self {
        self.load_filelists = load;
        self
    }

    /// Set whether comps.xml (package group metadata) should be parsed during
    /// [`RpmProvider::load_repo()`].
    ///
    /// When true, groups are loaded and registered as virtual solvables
    /// named `@group-id`. When false (the default), group metadata is ignored.
    pub fn load_groups(mut self, load: bool) -> Self {
        self.load_groups = load;
        self
    }

    /// Set which package types within groups are included as requirements.
    ///
    /// Only meaningful when [`load_groups`](Self::load_groups) is true.
    /// Defaults to [`GroupInstallOptions::default()`] (mandatory + default).
    pub fn group_options(mut self, options: GroupInstallOptions) -> Self {
        self.group_options = options;
        self
    }

    /// Set which optional groups within environments are included as requirements.
    ///
    /// Only meaningful when [`load_groups`](Self::load_groups) is true.
    /// Defaults to [`EnvironmentInstallOptions::default()`] (mandatory + default-flagged optional groups).
    pub fn environment_options(mut self, options: EnvironmentInstallOptions) -> Self {
        self.environment_options = options;
        self
    }

    /// Set whether updateinfo.xml (advisory/errata metadata) should be parsed
    /// during [`RpmProvider::load_repo()`].
    ///
    /// When true, advisories are loaded and registered as virtual solvables
    /// named `patch:ADVISORY-ID`. Each advisory solvable conflicts with
    /// pre-fix versions of affected packages, so resolving a patch forces the
    /// solver to upgrade those packages past the fixed version.
    /// When false (the default), advisory metadata is ignored.
    pub fn load_advisories(mut self, load: bool) -> Self {
        self.load_advisories = load;
        self
    }
}

/// Options controlling which package types within a group are included.
///
/// RPM comps groups categorize their packages as mandatory, default, or
/// optional. This struct controls which categories are pulled in as
/// requirements of the virtual group solvable.
///
/// Defaults match dnf's `groupinstall`: mandatory and default are included,
/// optional is excluded.
#[derive(Debug, Clone)]
pub struct GroupInstallOptions {
    pub(crate) include_mandatory: bool,
    pub(crate) include_default: bool,
    pub(crate) include_optional: bool,
}

impl GroupInstallOptions {
    /// Create options with default settings (mandatory + default included).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether mandatory packages are included.
    pub fn include_mandatory(mut self, include: bool) -> Self {
        self.include_mandatory = include;
        self
    }

    /// Set whether default packages are included.
    pub fn include_default(mut self, include: bool) -> Self {
        self.include_default = include;
        self
    }

    /// Set whether optional packages are included.
    pub fn include_optional(mut self, include: bool) -> Self {
        self.include_optional = include;
        self
    }
}

impl Default for GroupInstallOptions {
    fn default() -> Self {
        Self {
            include_mandatory: true,
            include_default: true,
            include_optional: false,
        }
    }
}

/// Options controlling which optional groups within an environment are included.
///
/// Comps environments have a mandatory group list (always included) and an
/// optional group list where each entry carries a `default` flag. This struct
/// controls which optional groups are pulled in as requirements of the virtual
/// environment solvable.
///
/// Defaults match dnf's `environment install`: mandatory groups are always
/// included, optional groups marked `default: true` are included, and
/// non-default optional groups are excluded.
#[derive(Debug, Clone)]
pub struct EnvironmentInstallOptions {
    pub(crate) include_default_options: bool,
    pub(crate) include_all_options: bool,
}

impl EnvironmentInstallOptions {
    /// Create options with default settings (mandatory + default-flagged optional groups).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether optional groups marked `default: true` are included.
    pub fn include_default_options(mut self, include: bool) -> Self {
        self.include_default_options = include;
        self
    }

    /// Set whether all optional groups are included, regardless of their default flag.
    pub fn include_all_options(mut self, include: bool) -> Self {
        self.include_all_options = include;
        self
    }
}

impl Default for EnvironmentInstallOptions {
    fn default() -> Self {
        Self {
            include_default_options: true,
            include_all_options: false,
        }
    }
}

/// Options controlling dependency resolution behavior.
///
/// Defaults match dnf: Recommends are enabled. Use the builder methods to
/// customize:
///
/// ```
/// use resolvo_rpm::ResolveOptions;
///
/// let opts = ResolveOptions::new().enable_recommends(false);
/// ```
#[derive(Debug, Clone)]
pub struct ResolveOptions {
    pub(crate) enable_recommends: bool,
    pub(crate) enable_suggests: bool,
}

impl ResolveOptions {
    /// Create options with default settings (Recommends enabled, Suggests disabled).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether Recommends are pulled in as soft requirements.
    ///
    /// When true (the default), performs a two-pass solve adding Recommends
    /// as soft requirements. When false, only hard Requires are resolved.
    pub fn enable_recommends(mut self, enable: bool) -> Self {
        self.enable_recommends = enable;
        self
    }

    /// Set whether Suggests are pulled in as soft requirements.
    ///
    /// When true, suggested packages are added as soft requirements alongside
    /// Recommends. When false (the default, matching dnf), Suggests are ignored.
    pub fn enable_suggests(mut self, enable: bool) -> Self {
        self.enable_suggests = enable;
        self
    }
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            enable_recommends: true,
            enable_suggests: false,
        }
    }
}

/// Options controlling dependency closure checking behavior.
///
/// Defaults check all packages in all repos. Use the builder methods to
/// narrow the scope:
///
/// ```
/// use resolvo_rpm::ClosureOptions;
///
/// let opts = ClosureOptions::new()
///     .check_repos(vec![1])
///     .newest_only(true);
/// ```
#[derive(Debug, Clone, Default)]
pub struct ClosureOptions {
    pub(crate) check_repos: Vec<usize>,
    pub(crate) newest_only: bool,
}

impl ClosureOptions {
    /// Create options with default settings (check all repos, all versions).
    pub fn new() -> Self {
        Self::default()
    }

    /// Only check packages from these repos (by repo_id) for unsatisfied deps.
    /// Other repos are still used to satisfy dependencies. An empty list
    /// (the default) checks all repos.
    pub fn check_repos(mut self, repos: Vec<usize>) -> Self {
        self.check_repos = repos;
        self
    }

    /// Only check the newest version of each (name, arch) pair.
    /// Older versions are skipped during the closure check but remain
    /// available for satisfying other packages' dependencies.
    pub fn newest_only(mut self, newest: bool) -> Self {
        self.newest_only = newest;
        self
    }
}
