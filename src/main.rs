#![feature(iterator_try_collect)]
use std::{
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
};

use anyhow::*;
use clap::{Parser, ValueEnum};
use ws_cleaner::{
    filtering::{find_unused_pkgs, DepType, Dependency, Package},
    parsing::find,
};

#[derive(ValueEnum, Clone, Debug)]
enum Action {
    /// Print all packages that are unused
    Print,
    /// Place a COLCON_IGNORE file
    ColconIgnore,
    /// Place a CATKIN_IGNORE file
    CatkinIgnore,
    /// Remove the package folder
    Remove,
}

fn touch(path: &Path) -> Result<()> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map(|_| {})
        .with_context(|| format!("Could not create '{}'", path.display()))
}

#[derive(Parser)]
#[command(version, about, next_line_help(true))]
struct Args {
    /// Remove unused packages from this path (usually the upstream workspace)
    #[arg(short, long)]
    upstream: PathBuf,

    /// Find packages whose dependencies to keep from these workspaces (multiple allowed)
    #[arg(short, long)]
    workspace: Vec<PathBuf>,

    /// Only consider these types (multiple allowed)
    #[arg(value_name = "DEPENDENCY TYPE", short = 't', long = "type")]
    dep_type: Vec<DepType>,

    /// Action to perform
    #[arg(short, long, value_enum, default_value_t=Action::Print)]
    action: Action,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mut ws_paths: Vec<PathBuf> = Vec::new();
    if args.workspace.is_empty() {
        println!(
            "Removing packages not used by '.' from upsream workspace '{}'",
            args.upstream.display(),
        );
        let default_path = PathBuf::from(".")
            .canonicalize()
            .with_context(|| "Invalid workspace: could not canonicalize path!")?;
        ws_paths.push(default_path);
    } else {
        // TODO: OK to leak full paths here?
        ws_paths = args
            .workspace
            .iter()
            .map(|x| x.canonicalize())
            .collect::<io::Result<Vec<PathBuf>>>()
            .with_context(|| "Could not normalize workspaces")?;
        ws_paths.sort();
        ws_paths.dedup();
    }
    let upstream_path = args.upstream.canonicalize().with_context(|| {
        format!(
            "Could not check upstream path '{}'",
            args.upstream.display()
        )
    })?;

    let mut ws_pkgs: Vec<Package> = ws_paths
        .iter()
        .map(|x| find(x).context("Could not enumerate workspace"))
        .try_collect::<Vec<Vec<Package>>>()?
        .into_iter()
        .flatten()
        .collect();
    ws_pkgs.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    ws_pkgs.dedup_by(|a, b| a.name.eq(&b.name) && a.path.eq(&b.path));

    let mut upstream_pks =
        find(&upstream_path).context("Could not enumerate upstream workspace")?;
    upstream_pks.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    upstream_pks.dedup_by(|a, b| a.name.eq(&b.name) && a.path.eq(&b.path));
    let need_filter = !args.dep_type.is_empty();
    // TODO: capture an iterator rather than moving the vector in?
    let match_specified = Dependency::matcher(args.dep_type);
    let mut filtered = find_unused_pkgs(
        &ws_pkgs,
        &upstream_pks,
        if need_filter {
            &match_specified
        } else {
            &Dependency::all
        },
    );
    filtered.sort_unstable_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
    println!("Workspace packages:");
    for ws_pkg in ws_pkgs {
        println!("{}", ws_pkg);
    }
    println!("\nUpstream packages:");
    for us_pkg in upstream_pks {
        println!("{}", us_pkg);
    }

    match args.action {
        Action::Print => {
            println!("\nUnused:");
            for unused in filtered {
                println!("{}", unused);
            }
        }
        Action::ColconIgnore => {
            println!("\nSetting up colcon ignore for:");
            for unused in filtered {
                let mut p = unused.path.clone();
                p.push("COLCON_IGNORE");
                println!("Creating '{}'", p.display());
                touch(&p)?;
            }
        }
        Action::CatkinIgnore => {
            println!("\nSetting up catkin ignore for:");
            for unused in filtered {
                let mut p = unused.path.clone();
                p.push("CATKIN_IGNORE");
                println!("Creating '{}'", p.display());
                touch(&p)?;
            }
        }
        Action::Remove => {
            println!("\nRemoving:");
            for unused in filtered {
                println!("rm -r '{}'", unused.path.display());
                fs::remove_dir_all(unused.path)?;
            }
        }
    }

    Ok(())
}
