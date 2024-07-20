use atom_syndication::{Feed, Text};
use env_logger;
use log;
use reqwest::blocking::get;
use std::{error::Error, fs};
use structopt::StructOpt;
use toml::Value;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "populate_crates",
    about = "Populate Sandbox Cargo.toml with top crates from lib.rs"
)]
struct Opt {
    /// Number of crates to fetch
    #[structopt(long, default_value = "100")]
    num_crates: usize,

    /// Log level
    #[structopt(long, default_value = "info")]
    log_level: String,

    /// File path to Cargo.toml
    #[structopt(long, default_value = "sandbox/Cargo.toml.sandbox")]
    cargo_toml: String,
}

struct Crate {
    name: Text,
    version: Text,
}

fn main() -> Result<(), Box<dyn Error>> {
    // Parse command line arguments
    let opt = Opt::from_args();

    // Init logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&opt.log_level))
        .init();

    // Fetch the Atom feed
    let response = get("https://lib.rs/std.atom")?;
    let body = response.text()?;

    // Parse the Atom feed
    let feed = body.parse::<Feed>().unwrap();

    let mut crates = Vec::new();
    // Print feed title and entries
    log::info!("Feed Title: {:?}", feed.title);
    for entry in &feed.entries {
        log::debug!("Crate: {:?}", entry.title);
        log::debug!("Version: {:?}", entry.summary);
        crates.push(Crate {
            name: entry.title.clone(),
            // unwrap: all crates should have a version
            version: entry.summary.as_ref().unwrap().clone(),
        });
    }
    log::info!("Total entries: {}", feed.entries.len());
    log::info!("Filtering top {} crates", opt.num_crates);
    crates.truncate(opt.num_crates);

    // Overwrite the existing Cargo.toml file
    let cargo_toml_path = opt.cargo_toml;
    let mut cargo_toml: Value = fs::read_to_string(&cargo_toml_path)?.parse()?;
    log::info!("Writing to Cargo.toml path: {}", cargo_toml_path);
    log::debug!("Cargo.toml input:\n{}", toml::to_string(&cargo_toml)?);

    let dependencies = cargo_toml
        .get_mut("dependencies")
        .and_then(Value::as_table_mut)
        // unwrap: dependencies will exist and is a table
        .unwrap();

    dependencies.clear();

    // Append new dependencies
    for c in crates {
        // unwrap: each version has a `v` prefix
        let version = c.version.strip_prefix("v").unwrap().to_string();
        dependencies.insert(c.name.to_string(), Value::String(version));
    }

    // Write back to the Cargo.toml file
    log::debug!("Cargo.toml output:\n{}", toml::to_string(&cargo_toml)?);
    fs::write(cargo_toml_path, toml::to_string(&cargo_toml)?)?;

    log::info!("Successfully appended crates to Cargo.toml.sandbox");

    Ok(())
}
