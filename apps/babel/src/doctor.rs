//! `babel doctor` : détecte la configuration réelle de la machine et l'état
//! des données. Rien n'est supposé : tout est mesuré au lancement.

use anyhow::Result;
use babel_core::Config;
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{Disks, System};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
/// Dimension et taille d'un vecteur d'embedding pour l'estimation de
/// stockage du § 6 : 1 024 dimensions en f16, ou 256 après réduction.
const EMBEDDING_DIM_FULL: u64 = 1024;
const EMBEDDING_DIM_REDUCED: u64 = 256;
const F16_BYTES: u64 = 2;

/// Carte graphique NVIDIA vue par `nvidia-smi`.
struct Gpu {
    name: String,
    vram_total_mib: u64,
    vram_free_mib: u64,
    driver: String,
}

fn nvidia_gpus() -> Vec<Gpu> {
    let output = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            Some(Gpu {
                name: (*fields.first()?).to_string(),
                vram_total_mib: fields.get(1)?.parse().ok()?,
                vram_free_mib: fields.get(2)?.parse().ok()?,
                driver: (*fields.get(3)?).to_string(),
            })
        })
        .collect()
}

/// Cherche dans le PATH une bibliothèque dont le nom commence par `prefix`.
fn find_library(prefix: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.to_ascii_lowercase().starts_with(prefix))
            })
    })
}

fn directml_library() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot")?;
    let path = Path::new(&root).join("System32").join("DirectML.dll");
    path.exists().then_some(path)
}

/// Espace libre du disque qui porte `path`, en octets.
fn free_space(path: &Path) -> Option<u64> {
    let absolute = std::path::absolute(path).ok()?;
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| absolute.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

fn describe(path: &Path) -> String {
    match file_size(path) {
        Some(size) => format!("{:.1} Mo", size as f64 / (1024.0 * 1024.0)),
        None => "absent".to_string(),
    }
}

pub fn doctor(config: &Config) -> Result<()> {
    println!("Babel doctor\n");

    println!("Système");
    println!(
        "  OS            : {} {}",
        System::name().unwrap_or_default(),
        System::os_version().unwrap_or_default()
    );
    let mut system = System::new();
    system.refresh_memory();
    let logical = std::thread::available_parallelism().map_or(0, usize::from);
    println!(
        "  CPU           : {} cœurs physiques, {} threads",
        System::physical_core_count().map_or("?".to_string(), |n| n.to_string()),
        logical
    );
    println!(
        "  RAM           : {:.1} Go",
        system.total_memory() as f64 / GIB
    );

    println!("\nGPU");
    let gpus = nvidia_gpus();
    if gpus.is_empty() {
        println!("  Aucun GPU NVIDIA détecté par nvidia-smi.");
    }
    for gpu in &gpus {
        println!(
            "  {} : {:.1} Go de VRAM ({:.1} Go libres), pilote {}",
            gpu.name,
            gpu.vram_total_mib as f64 / 1024.0,
            gpu.vram_free_mib as f64 / 1024.0,
            gpu.driver
        );
    }

    println!("\nFournisseurs d'exécution pour l'inférence (phase 2)");
    let cudart = find_library("cudart64");
    let cudnn = find_library("cudnn");
    let cuda_ready = !gpus.is_empty() && cudart.is_some() && cudnn.is_some();
    let missing = |lib: &Option<PathBuf>| {
        if lib.is_some() {
            "trouvé"
        } else {
            "absent du PATH"
        }
    };
    println!(
        "  CUDA          : {} (runtime CUDA {}, cuDNN {})",
        if cuda_ready {
            "disponible"
        } else {
            "indisponible"
        },
        missing(&cudart),
        missing(&cudnn)
    );
    let directml = directml_library();
    println!(
        "  DirectML      : {}",
        if directml.is_some() {
            "disponible"
        } else {
            "indisponible"
        }
    );
    println!("  CPU           : disponible");
    let chosen = if cuda_ready {
        "CUDA"
    } else if directml.is_some() {
        "DirectML"
    } else {
        "CPU"
    };
    println!("  → fournisseur retenu : {chosen} (ordre de repli : CUDA, DirectML, CPU)");

    println!("\nDisque");
    let data = &config.paths.data;
    match free_space(data) {
        Some(free) => println!("  {} : {:.1} Go libres", data.display(), free as f64 / GIB),
        None => println!("  {} : espace libre inconnu", data.display()),
    }

    println!("\nDonnées");
    let corpus_dir = config.paths.raw_ebible().join("corpus");
    let raw: Vec<u64> = std::fs::read_dir(&corpus_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "txt"))
                .filter_map(|e| e.metadata().ok().map(|m| m.len()))
                .collect()
        })
        .unwrap_or_default();
    if raw.is_empty() {
        println!("  eBible        : aucun fichier (lancer `babel fetch`)");
    } else {
        println!(
            "  eBible        : {} traductions, {:.2} Go",
            raw.len(),
            raw.iter().sum::<u64>() as f64 / GIB
        );
    }
    println!("  corpus.parquet: {}", describe(&config.paths.corpus()));
    println!("  signals       : {}", describe(&config.paths.signals()));
    println!("  scores        : {}", describe(&config.paths.scores()));
    println!("  atlas.bin     : {}", describe(&config.paths.atlas()));
    println!("  texts.zst     : {}", describe(&config.paths.texts()));

    // Estimation du stockage des embeddings (phase 2), recalculée à partir
    // du corpus ingéré.
    let paths = babel_ingest::CorpusPaths::in_dir(data);
    if paths.corpus.exists()
        && let Ok(rows) = corpus_rows(&paths.corpus)
    {
        let bytes = |dim: u64| rows * dim * F16_BYTES;
        println!(
            "\nEmbeddings (phase 2) : {} couples (traduction, verset) → {:.1} Go en {} dimensions, {:.1} Go en {}",
            rows,
            bytes(EMBEDDING_DIM_FULL) as f64 / GIB,
            EMBEDDING_DIM_FULL,
            bytes(EMBEDDING_DIM_REDUCED) as f64 / GIB,
            EMBEDDING_DIM_REDUCED
        );
        if let Some(free) = free_space(data)
            && bytes(EMBEDDING_DIM_FULL) > free
        {
            println!("  ⚠ l'espace libre ne suffit pas pour {EMBEDDING_DIM_FULL} dimensions.");
        }
    }
    Ok(())
}

/// Nombre de lignes de corpus.parquet, lu dans les métadonnées du fichier.
fn corpus_rows(path: &Path) -> Result<u64> {
    Ok(babel_ingest::store::row_count(path)?)
}
