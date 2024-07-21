// Initializes the Sandbox Cargo.toml file with the top crates from lib.rs
//
// Inspiration largely taken from Rust Playground
// https://github.com/rust-lang/rust-playground/blob/main/top-crates/src/lib.rs

use anyhow::{anyhow, Result};
use atom_syndication::Feed;
use cargo::{
    core::{Dependency, Package, PackageId, PackageSet, SourceId},
    sources::{
        source::{QueryKind, Source, SourceMap},
        RegistrySource,
    },
    util::{
        cache_lock::CacheLockMode, context::GlobalContext, interning::InternedString, VersionExt,
    },
};
use env_logger;
use log;
use reqwest::blocking::get;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashSet},
    error::Error,
    fs,
    task::Poll,
};
use structopt::StructOpt;
use toml::Value;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "populate_crates",
    about = "Populate Sandbox Cargo.toml with top crates from lib.rs"
)]
struct Opt {
    /// Number of crates to fetch
    #[structopt(long, default_value = "200")]
    num_crates: usize,

    /// Log level
    #[structopt(long, default_value = "info")]
    log_level: String,

    /// File path to Cargo.toml
    #[structopt(long, default_value = "Cargo.toml.sandbox")]
    cargo_toml: String,
}

// Parameterizes a package dependency in Cargo.toml, serialized as a table:
// ```
// [dependency.name]
// version = "version"
// features = ["feature1", "feature2", ...]
// default-features = `default_features`
// ```
#[derive(Serialize)]
struct Crate {
    // Name is already included in `dependencies` as a dotted key subtable
    // (`[dependency.name]`). Do not serialize as a table key to avoid redundancy.
    #[serde(skip)]
    name: String,
    version: String,
    features: BTreeSet<InternedString>,
    default_features: bool,
    #[serde(skip)]
    package_id: PackageId,
}

struct CargoResources<'gctx> {
    registry_source: RegistrySource<'gctx>,
    crates_io_source: SourceId,
}

fn init_cargo_resources(ctx: &GlobalContext) -> Result<CargoResources> {
    // On Cargo `CacheLocker`: https://docs.rs/cargo/0.80.0/cargo/util/cache_lock/index.html
    let _lock = ctx.acquire_package_cache_lock(CacheLockMode::DownloadExclusive)?;
    let crates_io_source = SourceId::crates_io(&ctx)?;
    let yanked_whitelist = HashSet::new();
    // Get data from the the default remote `crates.io` registry
    // https://doc.rust-lang.org/cargo/reference/registries.html?search=GlobalContex
    let mut registry_source = RegistrySource::remote(crates_io_source, &yanked_whitelist, &ctx)?;
    // registry_source.invalidate_cache();
    registry_source.block_until_ready()?;
    log::debug!("Crates IO Source ID: {:?}", crates_io_source);

    std::mem::drop(_lock);
    Ok(CargoResources {
        registry_source,
        crates_io_source,
    })
}

fn fetch_top_crate_names(num_crates: usize) -> Result<Vec<String>> {
    let max_entries = 250;
    if num_crates > max_entries {
        return Err(anyhow!(
            "Number of crates requested exceeds maximum entries in Atom feed: {} > {}",
            num_crates,
            max_entries,
        ));
    }
    // let mut crate_names = Vec::new();
    let mut crate_names = Vec::new();
    // Fetch the Atom feed (returns at most 250 entries)
    let response = get("https://lib.rs/std.atom")?;
    let body = response.text()?;

    // Parse the Atom feed
    let feed = body.parse::<Feed>().unwrap();

    // Print feed title and entries
    log::info!("Feed Title: {:?}", feed.title);
    for entry in &feed.entries {
        log::debug!("Crate: {:?}", entry.title);
        crate_names.push(entry.title.to_string());
    }
    log::info!("Total entries: {}", feed.entries.len());
    log::info!("Total crates fetched: {}", crate_names.len());
    log::info!("Filtering top {} crates", num_crates);
    crate_names.truncate(num_crates);
    return Ok(crate_names);
}

// Uses metadata from the `[package.metadata.playground]` table in crates `Cargo.toml`,
// as the playground does.
//
// Function copied from rust-lang/rust-playground:
// https://github.com/rust-lang/rust-playground/blob/main/top-crates/src/lib.rs
fn playground_metadata_features(pkg: &Package) -> Option<(BTreeSet<InternedString>, bool)> {
    let custom_metadata = pkg.manifest().custom_metadata()?;
    let playground_metadata = custom_metadata.get("playground")?;

    #[derive(Deserialize)]
    #[serde(default, rename_all = "kebab-case")]
    struct Metadata {
        features: BTreeSet<InternedString>,
        default_features: bool,
        all_features: bool,
    }

    // Toggle default features for any crates which do not explicitly specify otherwise
    impl Default for Metadata {
        fn default() -> Self {
            Metadata {
                features: BTreeSet::new(),
                default_features: true,
                all_features: false,
            }
        }
    }

    let metadata = match playground_metadata.clone().try_into::<Metadata>() {
        Ok(metadata) => metadata,
        Err(err) => {
            eprintln!(
                "Failed to parse custom metadata for {} {}: {}",
                pkg.name(),
                pkg.version(),
                err
            );
            return None;
        }
    };

    // If `all-features` is set then we ignore `features`.
    let summary = pkg.summary();
    let enabled_features: BTreeSet<InternedString> = if metadata.all_features {
        summary.features().keys().copied().collect()
    } else {
        metadata.features
    };

    Some((enabled_features, metadata.default_features))
}

// Downloads all packages for the given crates
fn download_packages<'gctx>(
    ctx: &'gctx GlobalContext,
    cargo_resources: &'gctx mut CargoResources,
    crates: &[Crate],
) -> Result<PackageSet<'gctx>> {
    let mut source_map = SourceMap::new();
    source_map.insert(Box::new(&mut cargo_resources.registry_source));
    let package_ids = crates.iter().map(|c| c.package_id).collect::<Vec<_>>();
    let package_set = PackageSet::new(&package_ids, source_map, ctx)?;
    Ok(package_set)
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments
    let opt = Opt::from_args();

    // Init logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&opt.log_level))
        .init();
    // Various metadata for Cargo (e.g. local Cargo installation)
    let ctx = GlobalContext::default()?;
    let mut cargo_resources = init_cargo_resources(&ctx)?;

    let top_crate_names = fetch_top_crate_names(opt.num_crates)?;
    let mut crates = Vec::new();
    for (idx, name) in top_crate_names.iter().enumerate() {
        log::debug!("Top crate #{}: {}", idx, name);

        let version = None;
        let dep = Dependency::parse(name, version, cargo_resources.crates_io_source)?;
        log::trace!("Name in toml: {:?}", dep.name_in_toml());
        let matches = {
            // `query_vec` requires mutable access to the `RegistrySource`
            let _lock = ctx.acquire_package_cache_lock(CacheLockMode::MutateExclusive)?;
            match cargo_resources
                .registry_source
                .query_vec(&dep, QueryKind::Exact)
            {
                Poll::Ready(Ok(v)) => v,
                Poll::Ready(Err(e)) => {
                    panic!("Unable to query registry for {}: {}", dep.name_in_toml(), e)
                }
                Poll::Pending => panic!("Registry not ready to query"),
            }
        };
        // Select newest version that is not yanked, and not a pre-release
        let newest_valid_version = matches
            .into_iter()
            .filter(|m| !m.is_yanked())
            .filter(|m| !m.as_summary().version().is_prerelease())
            .max_by_key(|m| m.as_summary().version().clone())
            // unwrap: at least one valid version should exist
            .unwrap_or_else(|| panic!("No valid versions found for {}", dep.name_in_toml()));

        log::trace!("Newest valid version: {:?}", newest_valid_version);
        log::trace!(
            "Newest valid version version: {:?}",
            newest_valid_version.as_summary().version()
        );

        log::trace!("Dependency Features: {:?}", dep.features());
        for f in newest_valid_version.as_summary().features().iter() {
            log::trace!("Index Summary Features: {:?}", f);
        }
        let package_id = newest_valid_version.package_id();

        let cargo_crate = Crate {
            name: dep.name_in_toml().to_string(),
            version: newest_valid_version.as_summary().version().to_string(),
            features: BTreeSet::new(),
            default_features: true,
            package_id,
        };
        crates.push(cargo_crate);
    }
    let packages = download_packages(&ctx, &mut cargo_resources, &crates)?;

    // Properly populate `features` and `default_features` fields
    for c in crates.iter_mut() {
        let package = packages.get_one(c.package_id).unwrap();
        if let Some((features, default_features)) = playground_metadata_features(&package) {
            c.features = features;
            c.default_features = default_features;
        }
    }

    // Overwrite the existing Cargo.toml file
    let cargo_toml_path = opt.cargo_toml;
    let mut cargo_toml: Value = fs::read_to_string(&cargo_toml_path)?.parse()?;
    log::info!("Writing to Cargo.toml path: {}", cargo_toml_path);

    let dependencies = cargo_toml
        .get_mut("dependencies")
        .and_then(Value::as_table_mut)
        // unwrap: dependencies will exist and is a table
        .unwrap();

    dependencies.clear();

    // Number of unique crates should match the number of crates fetched
    let unique_crates: HashSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(unique_crates.len(), opt.num_crates);

    for c in crates {
        dependencies.insert(c.name.to_string(), Value::try_from(c).unwrap());
    }

    // Write back to the Cargo.toml file
    log::debug!("Cargo.toml output:\n{}", toml::to_string(&cargo_toml)?);
    fs::write(cargo_toml_path, toml::to_string(&cargo_toml)?)?;

    log::info!("Successfully appended crates to Cargo.toml.sandbox");

    Ok(())
}
