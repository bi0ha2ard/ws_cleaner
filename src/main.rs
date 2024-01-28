use std::path::PathBuf;

use anyhow::*;
use clap::Parser;
use ws_cleaner::{
    filtering::{find_unused_pkgs, DepType, Dependency},
    parsing::find,
};

#[derive(Parser)]
#[command()]
struct Args {
    /// Only log what would happen
    #[arg(short, long, default_value_t = false)]
    dry_run: bool,
    /// Only consider these types
    #[arg(value_name = "DEPENDENCY TYPE", short = 't', long = "type")]
    dep_type: Vec<DepType>,
    /// Upstream workspace to be filtered
    #[arg(short, long)]
    upstream: PathBuf,
    /// Workspace for which to filter
    #[arg(short, long)]
    workspace: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let ws_path = args.workspace.unwrap_or(".".into());
    println!(
        "Filtering upsream workspace '{}' for workspace '{}'",
        args.upstream.display(),
        ws_path.display(),
    );

    // TODO: OK to leak full paths here?
    let abs_ws = ws_path
        .canonicalize()
        .with_context(|| format!("Could not check workspace '{}'", ws_path.display()))?;
    let upstream_path = args.upstream.canonicalize().with_context(|| {
        format!(
            "Could not check upstream path '{}'",
            args.upstream.display()
        )
    })?;

    if abs_ws == upstream_path {
        return Err(anyhow!(
            r#"Workspace and upstream must not be equal!
  Workspace: '{}' -> '{}'
  Upstream:  '{}' -> '{}'"#,
            ws_path.display(),
            abs_ws.display(),
            args.upstream.display(),
            upstream_path.display()
        ));
    }

    if abs_ws.starts_with(&upstream_path) {
        return Err(anyhow!(
            r#"Workspace must not be contained in upstream!
  Workspace: '{}' -> '{}'
  Upstream:  '{}' -> '{}'"#,
            ws_path.display(),
            abs_ws.display(),
            args.upstream.display(),
            upstream_path.display()
        ));
    }

    let ws_pkgs = find(&abs_ws)?;
    let upstream_pks = find(&upstream_path)?;
    // TODO: could sort + dedup here too
    let need_filter = !args.dep_type.is_empty();
    // TODO: capture an iterator rather than moving the vector in?
    let match_specified = Dependency::matcher(args.dep_type);
    let filtered = find_unused_pkgs(
        &ws_pkgs,
        &upstream_pks,
        if need_filter {
            &Dependency::all
        } else {
            &match_specified
        },
    );
    println!("Workspace packages:");
    for ws_pkg in ws_pkgs {
        println!("{}", ws_pkg);
    }
    println!("\nUpstream packages:");
    for us_pkg in upstream_pks {
        println!("{}", us_pkg);
    }

    if args.dry_run {
        println!("Dry-run!");
    }
    println!("\nRemoving:");
    for unused in filtered {
        println!("rm {}", unused);
    }

    Ok(())
}
