//! `babel` : la ligne de commande du projet.

mod doctor;
mod fetch;
mod pipeline;

use anyhow::{Context, Result, bail};
use babel_core::Config;
use babel_core::config::{EMBEDDED_EXCLUSIONS, Exclusions};
use clap::{Parser, Subcommand};
use pipeline::Pipeline;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "babel",
    version,
    about = "Carte de chaleur de l'incertitude de traduction de la Bible"
)]
struct Cli {
    /// Fichier de configuration.
    #[arg(long, global = true, default_value = "config/babel.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// GPU, fournisseurs d'exécution, VRAM, espace disque, état des données.
    Doctor,
    /// Clone ou met à jour eBible (et STEPBible).
    Fetch {
        /// Confirme un téléchargement volumineux.
        #[arg(long)]
        yes: bool,
    },
    /// eBible → corpus.parquet et rapport qualité.
    Ingest {
        #[arg(long)]
        force: bool,
    },
    /// S_omit, S_len, S_intra.
    Signals {
        #[arg(long)]
        force: bool,
    },
    /// S_align et incertitude des mots (phase 2).
    Align,
    /// S_sem sur GPU (phase 2).
    Embed,
    /// Normalisation, composite et tests de santé.
    Score {
        #[arg(long)]
        force: bool,
    },
    /// Rapport de validation (phase 2).
    Validate,
    /// Construit atlas.bin pour le viewer.
    Atlas {
        #[arg(long)]
        force: bool,
    },
    /// Toutes les étapes, avec cache.
    Run {
        /// Recalcule tout, sans tenir compte du cache.
        #[arg(long)]
        force: bool,
    },
    /// Lance l'interface.
    View {
        /// Ouvre directement sur une référence : --at "Jn 1:1".
        #[arg(long)]
        at: Option<String>,
        /// Lance une recherche à l'ouverture et suit la première proposition :
        /// --search "Jn 3:16" ou --search "lumière".
        #[arg(long)]
        search: Option<String>,
        /// Simule un pointeur à cette position de la fenêtre, pour capturer
        /// le survol et l'infobulle : --pointer 800x500.
        #[arg(long, value_name = "XxY", value_parser = parse_point)]
        pointer: Option<[f32; 2]>,
        /// Clique une fois à la position du pointeur simulé.
        #[arg(long, requires = "pointer")]
        click: bool,
        /// Métrique affichée : composite, omit, len ou intra.
        #[arg(long)]
        metric: Option<String>,
        /// Vue de départ : mur ou hilbert.
        #[arg(long)]
        layout: Option<String>,
        /// Enregistre une capture PNG de la fenêtre puis quitte.
        #[arg(long)]
        screenshot: Option<PathBuf>,
        /// Affiche les statistiques de rendu dès l'ouverture (touche F3).
        #[arg(long)]
        stats: bool,
        /// Mesure les images par seconde sur ce nombre d'images (zoom et
        /// panoramique scriptés, VSync coupée), puis quitte.
        #[arg(long, value_name = "IMAGES")]
        bench: Option<u32>,
        /// Garde une fenêtre de taille fixe au lieu de l'agrandir.
        #[arg(long)]
        windowed: bool,
        /// Taille de la fenêtre, en points : --size 2560x1440.
        #[arg(long, value_name = "LxH", value_parser = parse_size)]
        size: Option<[f32; 2]>,
    },
}

/// Lit une position « 800x500 », en points depuis le coin haut gauche.
fn parse_point(text: &str) -> Result<[f32; 2], String> {
    let (x, y) = text
        .split_once(['x', 'X', ','])
        .ok_or("format attendu : XxY, par exemple 800x500")?;
    let parse = |part: &str| {
        part.trim()
            .parse::<f32>()
            .map_err(|_| format!("coordonnée invalide : {part}"))
    };
    Ok([parse(x)?, parse(y)?])
}

/// Lit une taille de fenêtre « 2560x1440 ».
fn parse_size(text: &str) -> Result<[f32; 2], String> {
    let (width, height) = text
        .split_once(['x', 'X', '×'])
        .ok_or("format attendu : LARGEURxHAUTEUR, par exemple 2560x1440")?;
    let parse = |part: &str| {
        part.trim()
            .parse::<f32>()
            .ok()
            .filter(|value| *value >= 320.0)
            .ok_or(format!("dimension invalide : {part}"))
    };
    Ok([parse(width)?, parse(height)?])
}

fn pipeline(config: Config, force: bool) -> Result<Pipeline> {
    let path = &config.paths.exclusions;
    let exclusions_text = if path.exists() {
        std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?
    } else {
        EMBEDDED_EXCLUSIONS.to_string()
    };
    let exclusions = Exclusions::parse(&exclusions_text, path)?;
    Ok(Pipeline {
        config,
        exclusions,
        exclusions_text,
        force,
    })
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let config = Config::load_or_embedded(&cli.config)?;

    match cli.command {
        Command::Doctor => doctor::doctor(&config),
        Command::Fetch { yes } => fetch::fetch(&config, yes),
        Command::Ingest { force } => pipeline(config, force)?.ingest(),
        Command::Signals { force } => pipeline(config, force)?.signals(),
        Command::Score { force } => pipeline(config, force)?.score(),
        Command::Atlas { force } => pipeline(config, force)?.atlas(),
        Command::Run { force } => pipeline(config, force)?.run(),
        Command::Align | Command::Embed | Command::Validate => {
            bail!(
                "cette étape appartient à la phase 2 (alignement, embeddings GPU, validation) et \
                 n'est pas encore écrite. Le prototype couvre les phases 0 et 1 : \
                 fetch, ingest, signals, score, atlas, view."
            )
        }
        Command::View {
            at,
            search,
            pointer,
            click,
            metric,
            layout,
            screenshot,
            stats,
            bench,
            windowed,
            size,
        } => {
            let atlas = config.paths.atlas();
            if !atlas.exists() {
                bail!(
                    "{} est absent : lancer `babel run` d'abord",
                    atlas.display()
                );
            }
            let interactive = screenshot.is_none();
            babel_view::run(babel_view::Options {
                atlas,
                texts: config.paths.texts(),
                state: config.paths.view_state(),
                view: config.view.clone(),
                at,
                search,
                pointer,
                click,
                metric,
                layout,
                screenshot,
                stats,
                bench,
                // Une capture garde une taille fixe pour rester reproductible.
                maximize: !windowed && interactive && size.is_none(),
                size,
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}
