//! `babel fetch` : clone ou met à jour eBible (et STEPBible) avec git.
//!
//! Le clone est partiel (`--filter=blob:none`, profondeur 1) et clairsemé :
//! seuls les fichiers retenus par la sélection sont téléchargés.

use anyhow::{Context, Result, bail};
use babel_core::Config;
use babel_core::config::Selection;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Motifs toujours extraits du dépôt eBible : les métadonnées, sans les
/// pages de licence ni l'historique des nettoyages.
const EBIBLE_BASE_PATTERNS: &[&str] = &[
    "/metadata/*",
    "!/metadata/licenses/",
    "!/metadata/update-change-log/",
    "/README.md",
];

/// Dossier de STEPBible qui contient TAGNT et TAHOT.
const STEPBIBLE_PATTERNS: &[&str] = &["/Translators Amalgamated OT+NT/*", "/README.md"];

fn git(dir: Option<&Path>, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    command.args(args);
    command
}

fn run(mut command: Command, what: &str) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("lancement de git impossible ({what})"))?;
    if !status.success() {
        bail!("git a échoué : {what}");
    }
    Ok(())
}

fn output(mut command: Command, what: &str) -> Result<String> {
    let out = command
        .output()
        .with_context(|| format!("lancement de git impossible ({what})"))?;
    if !out.status.success() {
        bail!(
            "git a échoué : {what}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Clone le dépôt sans rien extraire, ou récupère la dernière révision.
fn clone_or_fetch(url: &str, dir: &Path) -> Result<()> {
    if dir.join(".git").exists() {
        println!("  mise à jour de {}", dir.display());
        run(
            git(
                Some(dir),
                &["fetch", "--depth", "1", "--filter=blob:none", "origin"],
            ),
            "fetch",
        )?;
    } else {
        println!("  clonage de {url}");
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = dir.to_string_lossy();
        run(
            git(
                None,
                &[
                    "clone",
                    "--filter=blob:none",
                    "--no-checkout",
                    "--depth",
                    "1",
                    url,
                    &target,
                ],
            ),
            "clone",
        )?;
    }
    Ok(())
}

/// Révision à extraire : la dernière récupérée, ou HEAD après un clone.
fn target_revision(dir: &Path) -> &'static str {
    if dir.join(".git").join("FETCH_HEAD").exists() {
        "FETCH_HEAD"
    } else {
        "HEAD"
    }
}

/// Fixe les motifs du checkout clairsemé puis extrait les fichiers.
fn sparse_checkout(dir: &Path, patterns: &[String]) -> Result<()> {
    let mut child = git(
        Some(dir),
        &["sparse-checkout", "set", "--no-cone", "--stdin"],
    )
    .stdin(Stdio::piped())
    .spawn()
    .context("lancement de git sparse-checkout impossible")?;
    if let Some(stdin) = child.stdin.as_mut() {
        for pattern in patterns {
            writeln!(stdin, "{pattern}")?;
        }
    }
    if !child.wait()?.success() {
        bail!("git sparse-checkout a échoué");
    }
    run(
        git(Some(dir), &["checkout", "--detach", target_revision(dir)]),
        "checkout",
    )
}

/// Choisit les fichiers de traduction à télécharger.
///
/// `files` sont les noms présents dans `corpus/` (`fra-fraLSG.txt`).
pub fn select(files: &[String], selection: Selection, always: &[String]) -> Vec<String> {
    let mut by_language: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for file in files {
        let language = file.split('-').next().unwrap_or(file);
        by_language.entry(language).or_default().push(file);
    }
    let mut selected: Vec<String> = by_language
        .values()
        .filter(|members| match selection {
            Selection::All => true,
            Selection::Multi => members.len() >= 2,
        })
        .flat_map(|members| members.iter().map(|f| (*f).clone()))
        .collect();
    for stem in always {
        let file = format!("{stem}.txt");
        if files.contains(&file) && !selected.contains(&file) {
            selected.push(file);
        }
    }
    selected.sort();
    selected
}

pub fn fetch(config: &Config, yes: bool) -> Result<()> {
    let dir = config.paths.raw_ebible();
    println!("eBible → {}", dir.display());
    clone_or_fetch(&config.fetch.ebible_url, &dir)?;

    let revision = target_revision(&dir);
    let listing = output(
        git(
            Some(&dir),
            &["ls-tree", "-r", "--name-only", revision, "corpus/"],
        ),
        "ls-tree",
    )?;
    let files: Vec<String> = listing
        .lines()
        .filter_map(|l| l.trim().trim_matches('"').strip_prefix("corpus/"))
        .filter(|f| f.ends_with(".txt"))
        .map(str::to_string)
        .collect();
    let selected = select(&files, config.fetch.selection, &config.fetch.always_include);
    println!(
        "  {} fichiers de traduction dans le dépôt, {} retenus (sélection « {:?} »)",
        files.len(),
        selected.len(),
        config.fetch.selection
    );
    if selected.len() > config.fetch.confirm_above_files && !yes {
        bail!(
            "{} fichiers à télécharger, au-delà du seuil de confirmation ({}). Le corpus complet \
             pèse plusieurs Go : relancer avec `babel fetch --yes` pour confirmer, ou régler \
             fetch.selection dans config/babel.toml.",
            selected.len(),
            config.fetch.confirm_above_files
        );
    }

    let mut patterns: Vec<String> = EBIBLE_BASE_PATTERNS
        .iter()
        .map(|p| (*p).to_string())
        .collect();
    patterns.extend(selected.iter().map(|f| format!("/corpus/{f}")));
    sparse_checkout(&dir, &patterns)?;
    println!("  eBible à jour.");

    if config.fetch.stepbible {
        let dir = config.paths.raw_stepbible();
        println!("STEPBible → {}", dir.display());
        clone_or_fetch(&config.fetch.stepbible_url, &dir)?;
        let patterns: Vec<String> = STEPBIBLE_PATTERNS
            .iter()
            .map(|p| (*p).to_string())
            .collect();
        sparse_checkout(&dir, &patterns)?;
        println!("  STEPBible à jour.");
    } else {
        println!(
            "STEPBible : ignoré (fetch.stepbible = false ; nécessaire à partir de la phase 2)."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<String> {
        [
            "eng-a.txt",
            "eng-b.txt",
            "fra-fraLSG.txt",
            "deu-x.txt",
            "deu-y.txt",
            "lat-latVUC.txt",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect()
    }

    #[test]
    fn selection_multi() {
        let selected = select(&files(), Selection::Multi, &["fra-fraLSG".to_string()]);
        assert_eq!(
            selected,
            vec![
                "deu-x.txt",
                "deu-y.txt",
                "eng-a.txt",
                "eng-b.txt",
                "fra-fraLSG.txt"
            ]
        );
    }

    #[test]
    fn selection_complete() {
        let selected = select(&files(), Selection::All, &["absente-du-depot".to_string()]);
        assert_eq!(selected.len(), 6);
    }
}
