//! gddbg: a disassembler for Geometry Dash levels.
//!
//! ```text
//! gddbg --gmd level.gmd                         # text disassembly to stdout
//! gddbg --gmd level.gmd -f dot -o graph.dot     # Graphviz call graph
//! gddbg --gmd level.gmd -f mermaid              # Mermaid flowchart
//! gddbg --gmd level.gmd --group 12 --depth 2    # only the neighbourhood of group 12
//! gddbg --gmd level.gmd -l tasm                 # listing as TASM routines (also: raw, details)
//! gddbg --gmd level.gmd --gui                   # interactive explorer
//! ```
//!
//! How it fits together: this file loads the level with gdlib and hands its objects to
//! [`analysis`], which builds a [`Model`] (groups, triggers, edges, items, diagnostics). Everything
//! else only reads that model:
//! * [`decode`]: gdlib's typed structs per object, and a one-line summary of each trigger.
//! * [`listing`]: a trigger as a raw / details / tasm line; [`highlight`] colours such lines.
//! * [`report`]: the CLI outputs (text listing, Graphviz, Mermaid).
//! * [`gui`]: the egui explorer (graph, map, listing and items tabs).

mod analysis;
mod decode;
mod gui;
mod highlight;
mod listing;
mod report;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use gdlib::cclocallevels::gdlevel::GDLevel;

use analysis::Model;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Format {
    /// Annotated listing: entry points, every group with its callers and triggers, items, diagnostics.
    Text,
    /// Graphviz call graph (render with `dot -Tsvg`).
    Dot,
    /// Mermaid flowchart (renders on GitHub, in Markdown previews, ...).
    Mermaid,
}

#[derive(Parser, Debug)]
#[command(
    name = "gddbg",
    version,
    about = "Disassembler and call-graph explorer for Geometry Dash levels"
)]
struct Cli {
    /// Level exported as .gmd
    #[arg(long, value_name = "FILE", conflicts_with_all = ["gdlvl", "file"])]
    gmd: Option<PathBuf>,

    /// Level exported as .gdlvl
    #[arg(long, value_name = "FILE", conflicts_with = "file")]
    gdlvl: Option<PathBuf>,

    /// Level file; the format is picked from the extension (.gdlvl, otherwise .gmd)
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Open the interactive explorer instead of printing a report
    #[arg(long)]
    gui: bool,

    /// Report format
    #[arg(short, long, value_enum, default_value_t = Format::Text)]
    format: Format,

    /// How triggers are written in the text listing
    #[arg(short, long, value_enum, default_value_t = listing::Mode::Details)]
    listing: listing::Mode,

    /// Write the report here instead of stdout
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Only show groups within --depth hops of this group
    #[arg(short, long, value_name = "GROUP")]
    group: Option<i16>,

    /// Hops to follow from --group (in both directions)
    #[arg(short, long, default_value_t = 2, requires = "group")]
    depth: usize,

    /// Also list groups that have no logic (pure decoration)
    #[arg(long)]
    all_groups: bool,
}

fn load(cli: &Cli) -> Result<(Model, PathBuf), String> {
    let (path, gdlvl) = match (&cli.gmd, &cli.gdlvl, &cli.file) {
        (Some(p), _, _) => (p.clone(), false),
        (_, Some(p), _) => (p.clone(), true),
        (_, _, Some(p)) => (
            p.clone(),
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("gdlvl")),
        ),
        _ => {
            return Err(
                "no level given: use --gmd <FILE>, --gdlvl <FILE> or pass a file path".into(),
            );
        }
    };
    Ok((load_level(&path, gdlvl)?, path))
}

fn load_level(path: &std::path::Path, gdlvl: bool) -> Result<Model, String> {
    let mut level = if gdlvl {
        GDLevel::from_gdlvl(path)
    } else {
        GDLevel::from_gmd(path)
    }
    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let name = level.identity.name.clone();
    let creator = level.identity.creator.clone();
    let data = level.get_decrypted_data_ref().ok_or_else(|| {
        format!(
            "{} has no level data (or it could not be decoded)",
            path.display()
        )
    })?;
    let objects = std::mem::take(&mut data.objects);
    Ok(Model::build(name, creator, objects))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (model, path) = match load(&cli) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("gddbg: {e}");
            return ExitCode::FAILURE;
        }
    };

    if cli.gui {
        return match gui::run(model, path) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("gddbg: GUI failed: {e}");
                ExitCode::FAILURE
            }
        };
    }

    if let Some(g) = cli.group
        && !model.groups.contains_key(&g)
    {
        eprintln!("gddbg: group {g} is not used or referenced anywhere in this level");
        return ExitCode::FAILURE;
    }
    let filter = report::Filter {
        only: cli.group.map(|g| model.neighbourhood(g, cli.depth)),
        all_groups: cli.all_groups,
    };
    let out = match cli.format {
        Format::Text => report::text(&model, &filter, cli.listing),
        Format::Dot => report::dot(&model, &filter),
        Format::Mermaid => report::mermaid(&model, &filter),
    };
    match &cli.output {
        Some(p) => {
            if let Err(e) = std::fs::write(p, out) {
                eprintln!("gddbg: could not write {}: {e}", p.display());
                return ExitCode::FAILURE;
            }
            eprintln!("gddbg: wrote {}", p.display());
        }
        None => {
            // colours only for a terminal: not when piped, and not if NO_COLOR is set
            use std::io::IsTerminal;
            let colour = matches!(cli.format, Format::Text)
                && std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
            if colour {
                print!("{}", highlight::ansi(&out))
            } else {
                print!("{out}")
            }
        }
    }
    ExitCode::SUCCESS
}
