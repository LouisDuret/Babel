# Babel

Carte de chaleur de l'incertitude de traduction de la Bible, verset par verset.
La référence est [SPEC.md](SPEC.md) (copie du cahier des charges) ; les choix
techniques non fixés par le cahier des charges sont dans
[DECISIONS.md](DECISIONS.md).

## État d'avancement

Prototype fonctionnel : **phases 0 et 1 terminées**, plus une partie de la
phase 3 (vue Hilbert, transitions, inspecteur, panneau Couches, fil d'Ariane,
état persistant). Les phases 2 (alignement, embeddings GPU, validation) et 4
ne sont pas commencées.

Le pipeline a tourné sur un **sous-corpus** d'eBible : les langues qui ont au
moins deux traductions (278 fichiers, 235 traductions ingérées, 74 langues).
Le corpus complet (1 079 fichiers, 2,3 Go) n'a pas été téléchargé : le disque
système de la machine de développement est presque plein. Voir ADR 2.

### Phase 0 — Socle et ingestion

- [x] `babel fetch` puis `babel ingest` tournent sans erreur (sur le sous-corpus ; corpus complet non essayé)
- [x] Chaque traduction ingérée a exactement autant de lignes que vref.txt (les autres fichiers sont écartés et listés)
- [x] Le rapport qualité liste la couverture par livre, les décalages de versification, les doublons et les écritures sans police
- [x] Tests sur le mini-corpus synthétique : lignes vides, `<range>`, normalisation NFC
- [x] `babel doctor` affiche le GPU, la VRAM, le fournisseur d'exécution disponible et l'espace disque
- [x] `cargo fmt`, `cargo clippy -- -D warnings` et `cargo test` passent

### Phase 1 — Signaux sans modèle et premier viewer

- [x] Tests de santé du § 5 passés (5 sur 5, sur le sous-corpus ; voir `data/report/health.md`)
- [x] La Bible entière est visible au lancement et le zoom descend jusqu'au verset
- [x] ≥ 144 images par seconde en zoom et panoramique (mesuré VSync coupée, voir ADR 15)
- [x] La recherche « Jn 3:16 » déclenche un vol vers le verset
- [x] Tests de propriétés : traductions identiques → dispersion nulle ; l'ordre des traductions ne change rien

### Phase 3 — déjà en place

Vue Hilbert et transition animée, inspecteur (référence, U, barres par signal,
traductions de référence, bande du chapitre), panneau Couches (interrupteurs,
poids en direct, filtre « canon protestant »), fil d'Ariane, raccourcis,
capture, état persistant.

Pas encore : vue Piste, mode comparaison, zoom Z4 (mots), minimap, mode
poster, filtres par famille / écriture / base textuelle / licence, texte
façonné par cosmic-text (hébreu de droite à gauche), pyramide d'agrégats.

## Commandes

```bash
cargo build --release -p babel        # le binaire est target/release/babel.exe
cargo run --release -p babel -- doctor
cargo run --release -p babel -- fetch          # clone clairsemé d'eBible
cargo run --release -p babel -- run            # ingest → signals → score → atlas, avec cache
cargo run --release -p babel -- view           # l'interface
cargo run --release -p babel -- view --at "Jn 1:1" --metric len
```

Étapes une par une : `ingest`, `signals`, `score`, `atlas` (option `--force`
pour ignorer le cache). `align`, `embed` et `validate` répondent qu'elles
appartiennent à la phase 2.

Options de `view` utiles au développement :

- `--search "Jn 3:16"` : lance une recherche à l'ouverture et suit la première proposition ;
- `--screenshot fichier.png` : enregistre une capture puis quitte (fenêtre de 1600 × 960) ;
- `--pointer 900x520` : simule un pointeur, pour capturer le survol et l'infobulle ;
- `--bench 3000 --size 2560x1400` : mesure les images par seconde, VSync coupée ;
- `--stats`, `--layout hilbert`, `--windowed`.

Qualité, à lancer avant chaque commit :

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p babel-signals -p babel-score    # chiffres de référence dans DECISIONS.md
```

## Structure

```text
config/babel.toml        tous les paramètres (aucun nombre magique dans le code)
config/exclusions.toml   traductions exclues des signaux, avec la raison
crates/babel-core        VerseId, livres (FR/EN/USFM), versification, références, config, statistiques robustes
crates/babel-ingest      eBible → Parquet, métadonnées, rapport qualité
crates/babel-signals     S_omit, S_len (median polish), S_intra (chrF), redondance
crates/babel-score       rangs percentiles, composite U(v), tests de santé
crates/babel-atlas       atlas.bin (rkyv), dispositions Mur et Hilbert, texts.zst
crates/babel-view        application graphique (eframe/egui + wgpu)
apps/babel               CLI (clap) : doctor, fetch, pipeline avec cache
tests/fixtures/mini      mini-corpus synthétique sur disque
data/                    ignoré par git : corpus brut, Parquet, atlas, rapports
```

Fichiers produits dans `data/` : `corpus.parquet`, `translations.parquet`,
`verses.parquet`, `signals.parquet`, `verse_stats.parquet`,
`per_translation.parquet`, `weights.parquet`, `scores.parquet`, `atlas.bin`,
`texts.zst`, `report/quality.md`, `report/health.md`, `report/coverage.tsv`,
`report/exclusions.suggested.toml`, `view_state.toml`.

Les crates `babel-align`, `babel-embed` et `babel-validate` du cahier des
charges seront créées en phase 2.

## Conventions

- Rust stable, édition 2024. `unsafe` interdit (lint `forbid` du workspace), `unwrap()` interdit hors des tests.
- Les fonctions de calcul sont pures ; les entrées-sorties sont dans les modules `store` et dans `apps/babel`.
- Tout paramètre vit dans `config/babel.toml`, embarqué à la compilation comme valeur par défaut. Un champ inconnu ou manquant est une erreur.
- Code, commentaires, messages et interface en français.
- Erreurs : `thiserror` dans les bibliothèques, `anyhow` dans le binaire.
- Chaque signal a un test sur le corpus synthétique avec une divergence injectée et un classement attendu (`crates/babel-signals/tests`).
- Couleurs et tailles de l'interface : uniquement via `crates/babel-view/src/theme.rs`.
- Cache : chaque étape enregistre une empreinte blake3 de ses entrées et de sa configuration dans `data/cache/`. **Quand le calcul d'une étape change, incrémenter `Stage::version()`** dans `apps/babel/src/pipeline.rs`, sinon le cache sert l'ancien résultat. Idem pour `FORMAT_VERSION` de l'atlas.
- Intégrité scientifique : ne jamais ajuster un signal pour faire passer un test de santé. Un résultat décevant est rapporté tel quel.

## Demander avant de…

Télécharger plus de 2 Go d'un coup, occuper plus de 50 Go de disque, installer
un outil système (CUDA, cuDNN, TensorRT), modifier une décision verrouillée du
§ 13 ou introduire du Python.

## Particularités de la machine de développement

- MSVC est installé dans `C:\VS` sans être déclaré dans le registre. Le fichier `.cargo/config.toml` (non suivi par git) fournit `INCLUDE` et `LIB` pour que `zstd-sys` compile.
- Le disque `C:` est presque plein ; `D:` a de la place. Pour y envoyer les données, changer `paths.data` dans `config/babel.toml`. Le dossier `target/debug` (tests, clippy) pèse environ 3,5 Go et peut être supprimé à tout moment.
- cuDNN est absent du PATH : `babel doctor` retient DirectML pour la phase 2.
- L'écran est à 74 Hz : avec la VSync, le viewer plafonne à 74 images/s.
