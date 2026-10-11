# Babel — Cahier des charges

*Carte de chaleur de l’incertitude de traduction de la Bible, verset par verset*

11 octobre 2026

## 1. Vision et livrables

Babel (nom de travail) calcule un score d’incertitude pour chacun des quelque 41 900 versets de la Bible, à partir du désaccord entre plus de 1 000 traductions. Il l’affiche sur une carte zoomable en dark mode, de la Bible entière jusqu’au mot.

L’hypothèse de départ : là où des traducteurs indépendants divergent, le texte source est ambigu, difficile ou textuellement disputé. La question scientifique centrale : le désaccord entre traducteurs, mesuré sans regarder l’original, prédit-il les variantes manuscrites connues ?

Livrables attendus :

- babel : une CLI Rust qui ingère le corpus, calcule les signaux et produit un « atlas » de résultats.
- babel view : une application de bureau native (Rust + wgpu) qui affiche la carte de chaleur de toute la Bible, avec un zoom continu.
- Un rapport de validation (Markdown + figures) qui confronte le score à la vérité terrain.
- Les résultats en Parquet, lisibles avec Polars ou DuckDB pour l’exploration libre.

Hors périmètre : toute interprétation théologique, la traduction automatique, et la publication web (seulement en extension, phase 4).

## 2. Matériel cible et contraintes

Le projet vise un PC gamer sous Windows, avec un GPU NVIDIA pour les embeddings et le rendu. babel doctor détecte la configuration réelle au lancement et adapte les réglages ; rien n’est codé en dur.

| Composant | Hypothèse par défaut | Usage dans Babel |
|---|---|---|
| OS | Windows 11 64 bits (Linux supporté) | Cible principale |
| GPU | NVIDIA RTX, ≥ 8 Go de VRAM (12–16 Go idéal) | Embeddings + rendu du viewer |
| CPU | 8 à 16 cœurs | Ingestion, alignement, scoring (rayon) |
| RAM | 32 Go | Tables en mémoire, memmap |
| Disque | SSD NVMe, ≥ 60 Go libres | Corpus, embeddings, cache |
| Écran | 2560×1440 à 144 Hz | Cible de performance du viewer |

Règles de langage :

- Rust stable, édition 2024, workspace Cargo. Tout ce qui tourne en production est en Rust.
- Python toléré dans deux cas seulement : l’export ONNX ponctuel du modèle d’embedding, et en phase 4 une projection UMAP si aucune crate Rust ne convient. Jamais dans le pipeline de production.
- Aucun service en ligne au runtime. Une fois les données et le modèle téléchargés, tout tourne en local.
- Dégradation gracieuse pour l’inférence : CUDA, puis DirectML (Windows, tout GPU DirectX 12), puis CPU. Le viewer n’exige qu’un GPU compatible DirectX 12 ou Vulkan.
- Machine utilisable pendant les calculs : une option --gpu-limit plafonne la taille des lots pour garder le PC réactif.

## 3. Données sources

Toutes les données sont ouvertes et téléchargeables par git clone. Le corpus de traductions vient d’eBible ; le grec, l’hébreu et la vérité terrain viennent de STEPBible.

| Source | Contenu | Rôle dans Babel | Licence |
|---|---|---|---|
| [eBible corpus](https://github.com/BibleNLP/ebible) | 1 000+ traductions, un verset par ligne, alignées sur vref.txt | Matière première | Variable, par traduction (metadata/licenses) |
| [STEPBible TAGNT](https://github.com/STEPBible/STEPBible-Data) | NT grec ; chaque mot marqué des éditions qui le contiennent (NA27/28, TR, Byz, SBL, WH…), lemmes, morphologie | Source NT de l’alignement + vérité terrain NT | CC BY 4.0 |
| [STEPBible TAHOT](https://github.com/STEPBible/STEPBible-Data) | AT hébreu (Leningrad), lemmes, morphologie, marqueurs de variantes (Ketiv/Qere, Alep, Qumrân…) | Source AT de l’alignement + vérité terrain AT | CC BY 4.0 |
| Textes grecs et hébreux d’eBible (grctr, grcsbl, grcbyz, hboWLC…) | Éditions critiques et Texte reçu | Contrôle croisé de la vérité terrain | Variable |
| Références croisées OpenBible.info (phase 4) | ~340 000 liens entre versets | Couche d’arcs dans le viewer | CC BY |

### Format eBible à respecter

- Chaque fichier <langue>-<projet>.txt contient une ligne par verset, dans l’ordre exact de vref.txt (versification « Original », références au format GEN 1:1).
- Ligne vide = verset absent de la traduction source.
- <range> = verset fusionné : le texte d’une plage est sur la ligne du premier verset, et chaque verset suivant de la plage contient le jeton <range>.
- Seul le texte des versets est conservé ; introductions, notes et titres ont été retirés.

### Règles d’ingestion

- VerseId = index de ligne dans vref.txt, stocké en u16 (moins de 65 536 lignes). Tout le pipeline travaille sur cet index.
- Plages fusionnées : compter le verset comme « présent mais fusionné ». Les signaux de longueur et de sémantique excluent ces versets pour cette traduction.
- Couverture par livre : une traduction ne participe aux signaux d’un livre que si elle en couvre au moins 90 %. Sinon, les traductions « NT seul » ou partielles fausseraient le signal d’omission.
- Contrôle qualité : détecter les décalages de versification (le verset v ressemble plus au consensus de v±1 qu’à celui de v), les traductions en double et les lignes non textuelles. Le résultat est un rapport et une liste d’exclusion éditable (config/exclusions.toml).
- Métadonnées par traduction : code ISO 639-3, nom de langue, écriture (détectée par Unicode), famille de langues, licence, année si connue, base textuelle inférée (voir 4.4).
- Normalisation Unicode NFC de tous les textes à l’ingestion.
- Aucun compte codé en dur : le nombre de traductions, de langues et de versets est recalculé à chaque ingestion.

### Licences

Le corpus mêle domaine public et licences Creative Commons, dont certaines NC-ND. Calculer des scores à partir de tous les textes ne pose pas de problème pour un usage personnel. En revanche, afficher le texte intégral d’une traduction dans une application partagée doit respecter sa licence : le viewer propose un mode publication qui n’affiche que le texte des traductions redistribuables.

## 4. Méthode : cinq signaux, un score composite

Aucun signal n’est fiable seul. Babel calcule cinq signaux indépendants, les normalise par traduction, puis les combine en un score U(v). Chaque signal reste visible séparément dans l’interface.

Notation : v = verset, t = traduction, x(t,v) = texte, T(v) = traductions éligibles pour v (couverture du livre ≥ 90 %).

### 4.1 Les signaux

| Code | Signal | Ce qui est mesuré | Couverture | Coût |
|---|---|---|---|---|
| S_omit | Omission | Part des traductions éligibles où v est vide ou fusionné | Toutes | Négligeable |
| S_len | Écart de longueur | Dispersion des résidus de longueur | Toutes | Négligeable |
| S_intra | Divergence intra-langue | 1 − chrF moyen entre traductions d’une même langue | Langues avec ≥ 2 traductions | Faible, CPU |
| S_align | Non-littéralité | Vraisemblance d’alignement avec le grec ou l’hébreu, par mot | Toutes les écritures segmentables | Moyen, CPU |
| S_sem | Dispersion sémantique | Distance au consensus dans un espace d’embedding multilingue | Langues couvertes par le modèle | Élevé, GPU |

S_len. Modèle additif robuste sur la longueur L en graphèmes Unicode :

    log L(t,v) = a(t) + b(v) + ε(t,v)

a(t) capte la verbosité de la traduction, b(v) la longueur propre du verset. Estimation par médianes itérées (median polish). S_len(v) = MAD des résidus sur t. Ce signal capte les ajouts, les explicitations et les omissions partielles.

S_intra. chrF (n-grammes de caractères de 1 à 6, β = 2), implémenté en Rust. Pour chaque langue à au moins deux traductions : moyenne des 1 − chrF par paires, normalisée par langue, puis moyenne pondérée sur les langues. C’est le signal le plus propre, car il ne franchit aucune barrière de langue.

S_align. Côté source : lemmes TAGNT (NT) et TAHOT (AT). Pour chaque traduction, un aligneur IBM Model 1 avec a priori diagonal (style fast_align), écrit en Rust, 5 itérations EM, parallélisé par traduction. Mesure par verset : log-vraisemblance moyenne par mot cible. Les langues sans espaces (chinois, japonais, thaï, lao, khmer, birman) sont tokenisées en caractères ou exclues ; la décision est consignée dans DECISIONS.md.

S_sem. Embeddings normalisés e(t,v). Distance au consensus pondéré, calculé sans la traduction elle-même (leave-one-out) :

    d(t,v) = 1 − cos( e(t,v), μ(v)^(−t) )        μ(v)^(−t) = Σ_{u≠t} w(u)·e(u,v) / Σ_{u≠t} w(u)

### 4.2 Normalisation par traduction

C’est l’étape décisive. Chaque mesure brute d(t,v) est centrée-réduite de façon robuste par traduction :

    z(t,v) = ( d(t,v) − med_v d(t,·) ) / ( 1,4826 · MAD_v d(t,·) )

Une langue mal servie par le modèle produit des distances élevées partout. Ce bruit est constant, donc la normalisation l’efface. Il reste ce qui compte : les versets où une traduction s’écarte plus que d’habitude.

Agrégation : S(v) = moyenne tronquée à 10 % des z(t,v), pondérée par w(t).

Poids w(t) contre la redondance. Les traductions ne sont pas indépendantes : révisions successives, même équipe, même langue-pont. Regrouper les traductions quasi identiques d’une même langue (chrF moyen > 0,8, seuil configurable) et donner un poids total de 1 à chaque groupe. Pour S_sem, multiplier aussi par la qualité du modèle sur la langue.

### 4.3 Score composite

    U(v) = Σ_k ω_k · rang%( S_k(v) )        Σ_k ω_k = 1

Les rangs percentiles rendent les signaux comparables. Les poids ω sont égaux par défaut et réglables en direct dans l’interface. Option : poids appris par régression logistique sur la vérité terrain, uniquement sur la moitié « calibration » des livres (voir 5).

### 4.4 Décomposition : incertitude textuelle ou interprétative

À Actes 8:37, les traductions divergent parce qu’elles suivent des textes grecs différents, pas parce qu’elles interprètent différemment. Babel sépare les deux.

- Inférer la base textuelle de chaque traduction du NT à partir de sa présence ou absence sur des versets diagnostiques, absents des éditions critiques : Mt 17:21, Mt 18:11, Mt 23:14, Mc 7:16, Mc 9:44, Mc 9:46, Mc 11:26, Mc 15:28, Lc 17:36, Lc 23:17, Jn 5:4, Ac 8:37, Ac 15:34, Ac 24:7, Ac 28:29, Rm 16:24.
- Classer chaque traduction en « Texte reçu / byzantin », « texte critique » ou « mixte ».
- Décomposer la variance comme une ANOVA : U_textuel(v) = divergence entre groupes, U_interprétatif(v) = divergence moyenne à l’intérieur des groupes.

Pour l’AT, la phase 2 se limite à marquer les versets Ketiv/Qere ; une décomposition complète (texte massorétique contre Septante) reste une extension.

### 4.5 Incertitude au niveau du mot

Pour chaque occurrence d’un lemme source w dans v, à partir des probabilités d’alignement :

    u(w) = 1 − (1 / |T(v)|) · Σ_{t∈T(v)} max_j P_t(a_j = w)

C’est la part des traductions qui ne rendent pas ce mot de façon nette. Étant une probabilité, elle se compare d’une langue à l’autre. Elle alimente le dernier niveau de zoom du viewer.

### 4.6 Incertitude sur l’incertitude

Bootstrap : rééchantillonner les traductions par grappes de langues, B = 200 fois. On obtient un intervalle à 90 % pour chaque S_k(v) et pour U(v). L’inspecteur l’affiche, et le viewer peut griser les versets dont l’intervalle est trop large.

## 5. Validation

Le score doit battre des baselines naïves sur des livres qu’il n’a jamais vus. Le critère d’acceptation est la rigueur du rapport, pas une valeur d’AUC : un résultat négatif bien mesuré est un résultat.

### Vérité terrain

- NT : G(v) = nombre de mots TAGNT absents d’au moins une édition majeure (marqueurs N, K, O). Version binaire : G(v) > 0. Contrôle croisé par un diff mot à mot entre grctr et grcsbl d’eBible.
- AT : G(v) = nombre de marqueurs de variantes TAHOT, Ketiv/Qere en priorité.

### Métriques

- AUC-ROC de U(v) et de chaque signal pour prédire G(v) > 0.
- Corrélation de Spearman avec G(v).
- Corrélation partielle en contrôlant la longueur du verset et la rareté lexicale. Sans ce contrôle, on risque de mesurer seulement que les longs versets sont incertains.
- Baselines obligatoires : longueur seule, nombre de hapax du verset, score aléatoire.

### Protocole contre le surapprentissage

- Séparation par livres fixée avant tout calcul, avec une graine : par exemple livres d’index pair = calibration, impair = test. Les poids appris et les seuils ne voient jamais le test.
- Le rapport présente tous les signaux, y compris ceux qui échouent.

### Tests de santé

Ces résultats sont attendus mais pas garantis. Un échec déclenche une enquête (bug d’ingestion, versification), jamais un réglage du score pour le faire passer.

- S_omit : Ac 8:37, Mt 17:21, Mt 18:11 et Lc 17:36 dans le top 1 % du NT.
- S_len : 1 Jn 5:7 (le Comma johannique) dans le top 1 % du NT.

### Test de sensibilité du modèle d’embedding

Sur 50 traductions × 500 versets, injecter des perturbations contrôlées : négation, inversion de deux noms propres, suppression d’une proposition. La distance au consensus doit augmenter nettement plus que pour une paraphrase tirée d’une autre traduction de la même langue. Un modèle qui échoue à ce test est rejeté, quelle que soit sa couverture.

## 6. Architecture logicielle

Un workspace Cargo découpé en crates à responsabilité unique, avec un binaire CLI et un binaire viewer. Chaque étape du pipeline est idempotente et mise en cache par empreinte (blake3) de ses entrées et de la configuration.

### Arborescence

```text
babel/
├── Cargo.toml              # workspace
├── README.md               # commandes, conventions (tenu à jour)
├── DECISIONS.md            # journal des décisions techniques (format ADR)
├── config/
│   ├── babel.toml          # chemins, seuils, poids, modèle
│   └── exclusions.toml     # traductions exclues + raison
├── crates/
│   ├── babel-core/         # VerseId, livres (noms FR/EN/USFM), versification, types
│   ├── babel-ingest/       # eBible + STEPBible → Parquet, rapport qualité
│   ├── babel-signals/      # S_omit, S_len, S_intra (chrF)
│   ├── babel-align/        # aligneur IBM1 + diagonale, S_align, u(w)
│   ├── babel-embed/        # ort + tokenizers, pipeline GPU, S_sem
│   ├── babel-score/        # normalisation, composite, bootstrap, décomposition
│   ├── babel-validate/     # vérité terrain, métriques, rapport
│   ├── babel-atlas/        # format binaire du viewer (rkyv), layouts
│   └── babel-view/         # application graphique
├── apps/babel/             # binaire CLI (clap)
├── tests/fixtures/         # mini-corpus synthétique
└── data/                   # ignoré par git
```

### Crates recommandées

Le développeur vérifie les dernières versions stables au démarrage, et la compatibilité entre egui-wgpu et wgpu (versions couplées).

| Besoin | Crate | Remarque |
|---|---|---|
| Parallélisme CPU | rayon | Par traduction, par verset |
| Canaux de pipeline | crossbeam-channel | Bornés, pour la contre-pression |
| Tables | arrow + parquet | Lisibles par Polars et DuckDB |
| Chargement zéro-copie | rkyv + memmap2 | Atlas du viewer |
| Compression des textes | zstd | Blocs par traduction × livre |
| Inférence | ort | CUDA, TensorRT, DirectML, CPU |
| Tokenisation | tokenizers | Rust natif (Hugging Face) |
| Unicode | unicode-segmentation, unicode-script, unicode-normalization | Graphèmes, écritures, NFC |
| Cache | blake3 | Empreinte des étapes |
| CLI, logs | clap, tracing, indicatif | Barres de progression avec ETA |
| Erreurs | thiserror (libs), anyhow (bins) |  |
| Fenêtre + panneaux | eframe / egui (backend wgpu) |  |
| Rendu du canevas | wgpu | Shaders WGSL |
| Texte complexe | cosmic-text + glyphon | Hébreu RTL, écritures complexes |
| Figures du rapport | plotters |  |
| Tests | proptest, criterion, insta | Propriétés, benchmarks, snapshots |

### Fichiers produits

| Fichier | Contenu | Taille estimée |
|---|---|---|
| data/raw/ | Clones eBible et STEPBible | Plusieurs Go |
| corpus.parquet | translation_id, verse_id, text, statut | Quelques Go |
| texts.zst + index | Textes compressés par blocs pour le viewer | ~1 Go |
| embeddings.f16 | Memmap [vecteurs × dimension] en f16 | ~30 Go en 1024 dim, ~8 Go en 256 dim |
| per_translation.parquet | z par signal, par traduction et verset | ~1 Go |
| signals.parquet | verse_id, signal, valeur, IC bas, IC haut | < 50 Mo |
| words.parquet | verse_id, position, lemme, u(w) | < 100 Mo |
| atlas.bin | Tout ce que le viewer charge au démarrage | < 50 Mo |
| report/ | validation.md + figures |  |

L’estimation des embeddings suppose ~15 millions de paires (traduction, verset) non vides ; elle est recalculée et affichée par babel doctor avant le calcul.

### Commandes de la CLI

```text
babel doctor     # GPU, CUDA/DirectML, VRAM, espace disque, estimation des temps
babel fetch      # clone ou met à jour eBible + STEPBible
babel ingest     # → corpus.parquet + rapport qualité
babel signals    # S_omit, S_len, S_intra
babel align      # S_align + incertitude des mots
babel embed      # S_sem sur GPU, reprise automatique
babel score      # normalisation, composite, bootstrap, décomposition
babel validate   # rapport de validation
babel atlas      # construit atlas.bin pour le viewer
babel run        # toutes les étapes, avec cache
babel view       # lance l'interface
```

## 7. Pipeline de calcul et performance

Le seul goulot est S_sem : environ 15 millions de phrases à encoder sur le GPU. Tout le reste tourne en quelques minutes sur CPU.

*Figure : Flux de données, de l’ingestion au viewer.*

Les trois branches du milieu sont indépendantes : signals et align peuvent tourner sur le CPU pendant que embed occupe le GPU.

En texte, pour référence : eBible + STEPBible → ingest → (signals ‖ align ‖ embed) → score → (validate → rapport) et (atlas → view).

### Choix du modèle d’embedding

C’est une décision que le développeur prend en suivant ce protocole, et qu’il consigne dans DECISIONS.md.

- Candidat principal : OmniSONAR (Meta, mars 2026). Il couvre plus de 4 200 variétés de langues dans un espace de 1 024 dimensions, en tailles de 1,5 milliard à 39 millions de paramètres. Vérifier d’abord que les poids sont publiés et sous quelle licence. ([source](https://arxiv.org/html/2603.16606v1))
- Biais à surveiller : la Bible fait partie de ses données d’entraînement, et son benchmark principal est évalué sur l’Évangile de Jean. Le modèle risque de rapprocher les traductions d’un même verset par mémorisation, ce qui écraserait le désaccord qu’on veut mesurer. Le test de sensibilité du § 5 tranche, et les autres signaux servent de garde-fou.
- Replis : SONAR (200 langues), LaBSE, MEXMA.
- Itérer petit, finir grand : développer avec une petite variante (Small ou Tiny), lancer la grande pour le calcul final, la nuit.
- Critères de choix : couverture des langues du corpus, réussite du test de sensibilité, débit mesuré sur la machine.

### Pipeline GPU

- Lecture des traductions en flux depuis corpus.parquet.
- Tokenisation sur 2 à 4 threads (tokenizers), avec tri par longueur dans chaque traduction pour limiter le padding.
- Lots dynamiques par budget de tokens, pas par nombre de phrases. Le budget est calibré automatiquement sur la VRAM libre au démarrage.
- Inférence ort en FP16 avec IoBinding (pas de copie hôte ↔ GPU inutile). Ordre de repli : CUDA, TensorRT en option, DirectML, CPU.
- Accumulation : écriture des vecteurs dans embeddings.f16 (memmap) et mise à jour en flux des sommes par verset pour le consensus.
- Reprise : un point de contrôle par traduction. Un arrêt (Ctrl+C, plantage) reprend là où il s’était arrêté.

Des canaux bornés relient les étapes. Objectif : le GPU n’attend jamais le CPU (utilisation > 90 % dans nvidia-smi pendant embed).

Option de réduction : ajuster une ACP sur 500 000 vecteurs échantillonnés et projeter en 256 dimensions avant écriture. Le stockage passe d’environ 30 Go à environ 8 Go.

### Budgets de temps (objectifs à mesurer)

| Étape | Objectif sur la machine cible |
|---|---|
| ingest + signals + align | < 30 min au total |
| embed, petit modèle | < 2 h |
| embed, grand modèle | Une nuit (< 10 h) |
| score avec bootstrap B = 200 | < 5 min |

babel doctor mesure le débit réel sur un échantillon et affiche une estimation avant de lancer le calcul complet.

## 8. Interface graphique

Toute la Bible tient sur un seul écran : chaque verset est une cellule colorée d’environ 7 à 9 px en 2560×1440. On zoome en continu jusqu’au mot grec ou hébreu, et l’information affichée change avec l’échelle (zoom sémantique).

### Disposition de l’écran

```text
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ BABEL  [ Rechercher « Jn 3:16 »…  Ctrl+K ]  [Mur|Hilbert|Piste]  [Composite ▾]  [Palette]│
├───────────────┬──────────────────────────────────────────────────────┬───────────────────┤
│ COUCHES       │ Pentateuque    Historiques        Poétiques          │ JEAN 1:1          │
│ ▣ Composite   │ ┌─GEN──┐┌EXO┐  ┌──JOS──┐┌JDG┐    ┌───PSA────┐        │ U 0,87   p97      │
│ Omission ──○─ │ │▓▒░░▒▓││░▒░│  │▒░░░▒░░││░▒▓│    │░▒▒░░▒░░▒▒│        │ IC [0,81–0,92]    │
│ Longueur ─○── │ │░░▒▓▓▒││▒▒░│  │░░▒░░░▒││▒░░│    │▒░░▒░░▒▓▒░│        │ ▇▇▇▇▇  Omission   │
│ Intra    ──○─ │ └──────┘└───┘  └───────┘└───┘    └──────────┘        │ ▇▇▇    Longueur   │
│ Alignem. ─○── │ Évangiles          Actes  Épîtres     Apoc.          │ ▇▇▇▇▇▇ Sémant.    │
│ Sémant.  ──○─ │ ┌MAT┐┌MRK┐┌LUK┐┌JHN┐ ┌ACT┐  ┌ROM┐┌1CO┐…  ┌REV┐       │                   │
│               │ │▒▓░││░▒░││▒░▒││▓▒░│ │░▒▒│  │▒░░││░▒░│   │░▒░│       │ Ἐν ἀρχῇ ἦν ὁ      │
│ FILTRES       │ └───┘└───┘└───┘└───┘ └───┘  └───┘└───┘   └───┘       │ λόγος  [tuiles]   │
│ Testament     │                                                      │                   │
│ Familles      │                               ┌─minimap─┐            │ Distribution      │
│ Écritures     │                               │ ▢       │            │  ·:·:|:·:·        │
│ Base textuelle│                               └─────────┘            │ Top divergents    │
│ Licence       │                                                      │                   │
├───────────────┴──────────────────────────────────────────────────────┴───────────────────┤
│ Bible › NT › Jean › 1 › 1     [░▒▓█ 0 ── 1]       1 014 traductions · 144 fps            │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

Les valeurs affichées dans ce croquis (0,87, 1 014 traductions…) sont des exemples de mise en page, pas des résultats.

### Les vues

Chaque verset est une instance GPU qui possède une position dans chaque vue. Changer de vue anime le passage : les 41 900 cellules glissent vers leur nouvelle place en 600 ms (ease-in-out).

- Mur (vue par défaut). Disposition en étagères : les sections du canon forment des bandes, les livres des blocs, chaque chapitre une rangée de cellules (retour à la ligne après 40 versets). Un écart de 8 à 12 % de la taille de cellule laisse voir le fond et donne un effet de mosaïque.
- Hilbert. L’ordre canonique suit une courbe de Hilbert sur une grille de 256×256. Les passages contigus forment des zones compactes ; les livres deviennent des territoires avec frontières, comme une carte de pays. Le nom de chaque livre s’affiche à son centroïde.
- Piste. Façon navigateur de génome : l’axe horizontal suit l’ordre canonique, et des pistes empilées montrent U, chaque signal, U_textuel, U_interprétatif, la vérité terrain et la couverture. Zoom horizontal seul, de la Bible entière à un verset. Une sélection de plage affiche ses statistiques.
- Galaxie (phase 4). Projection 2D des embeddings moyens par verset : les versets proches par le sens se regroupent, colorés par U.

### Zoom sémantique

Les étiquettes apparaissent en fondu selon leur taille à l’écran, avec une hystérésis pour éviter les clignotements aux seuils.

| Niveau | Taille d’une cellule à l’écran | Ce qui apparaît |
|---|---|---|
| Z0 — Bible | < 12 px | Couleurs seules ; noms des sections et des livres. Sous 2 px, agrégat par chapitre (pyramide précalculée) |
| Z1 — Livre | 12 à 40 px | Numéros de chapitres en marge, contours des chapitres |
| Z2 — Chapitre | 40 à 160 px | Numéro de verset + cinq micro-barres, une par signal |
| Z3 — Verset | 160 à 600 px | La cellule devient une carte : texte de référence, texte source, U et son intervalle |
| Z4 — Mot | > 600 px | Mots grecs ou hébreux en tuiles colorées par u(w), glose sous chaque mot ; le survol montre le rendu dans cinq langues choisies |

### Les panneaux

- Barre supérieure (48 px). Recherche (Ctrl+K) qui accepte les références en français et en anglais (« Jn 3:16 », « Jean 3 », « 1 Co 13 »), les noms de livres et le texte libre de la traduction de référence. Puis le choix de vue (contrôle segmenté), le choix de métrique, la palette et la capture.
- Panneau gauche « Couches » (280 px, repliable). Chaque signal a un interrupteur et un curseur de poids ; le composite se recalcule en direct. Les filtres (testament, sections, familles de langues, écritures, base textuelle, licence) recalculent les scores sur le sous-ensemble de traductions choisi, en moins de 200 ms. On peut donc demander : « que disent seulement les langues bantoues ? »
- Mode comparaison. Deux métriques, ou deux sous-ensembles de traductions (par exemple langues austronésiennes contre indo-européennes). Affichage de la différence en palette divergente, ou écran partagé avec caméras synchronisées.
- Inspecteur droit (380 px), à la sélection d’un verset. Il contient, de haut en bas :
  - la référence, U en grand avec son percentile et son intervalle ;
  - une barre par signal avec son percentile ;
  - le texte source (grec, ou hébreu de droite à gauche) en tuiles colorées par u(w) ;
  - deux à quatre traductions de référence choisies par l’utilisateur ;
  - la distribution des écarts par traduction (nuage de points) ; le survol d’un point montre son texte et sa langue ;
  - les dix traductions les plus divergentes ;
  - la vérité terrain : variante attestée ou non, et les éditions concernées ;
  - une mini-bande de chaleur du chapitre pour le contexte.
- Barre inférieure (28 px). Fil d’Ariane cliquable (« Bible › NT › Jean › 1 › 1 »), légende de la palette avec ses quantiles, nombre de traductions actives, fps (touche F3).
- Minimap en bas à droite du canevas, avec le rectangle de la vue courante ; on la fait glisser pour naviguer.
- Infobulle au survol après 150 ms : référence, U, percentile, nombre de traductions, micro-barres.

### Mode poster

Export PNG de la carte entière en très haute résolution (rendu par tuiles, jusqu’à 16 384 px de côté), avec titre et légende. De quoi imprimer la Bible entière en carte de chaleur.

## 9. Design system (dark mode)

Une règle structure tout le design : les couleurs chaudes sont réservées aux données, le cyan à l’interaction. L’interface reste en gris-bleu très sombres pour que la carte de chaleur soit la seule chose qui brille.

### Couleurs de l’interface

Tous les composants lisent ces jetons dans un module theme.rs ; aucune couleur n’est écrite en dur ailleurs.

| Jeton | Valeur | Usage |
|---|---|---|
| bg.canvas | #0A0C10 | Fond du canevas |
| bg.panel | #12151B | Panneaux latéraux et barres |
| bg.elevated | #1A1E26 | Infobulles, menus, cartes de verset |
| border.subtle | #252A34 | Séparateurs |
| border.strong | #343B48 | Contours des contrôles actifs |
| text.primary | #E7E9EE | Texte principal |
| text.secondary | #9AA3B2 | Libellés, légendes |
| text.tertiary | #646D7C | Indications non essentielles uniquement |
| accent | #22D3EE | Sélection, focus clavier, curseurs actifs |
| hover | #FFFFFF à 60 % | Contour de la cellule survolée |
| missing | Hachures #2A2F3A | Verset sans données |

Contraste : tout texte essentiel respecte WCAG AA (≥ 4,5:1 sur son fond). text.tertiary est réservé aux éléments décoratifs.

### Palettes de données

- « Braise » (par défaut). De l’indigo nuit au blanc chaud : #262B52 → #4B2C73 → #9C2F5E → #E0583A → #F9A23B → #FFE9A8. Interpolation en OKLab, sur une texture LUT de 256 texels.
- Plancher de luminosité : la valeur la plus basse garde une luminosité OKLab ≥ 22 %. Les versets « sûrs » restent visibles au lieu de se fondre dans le fond.
- Luminosité strictement croissante : un test unitaire vérifie que la luminosité OKLab augmente sur les 256 texels, et que la palette reste lisible en simulation de deutéranopie.
- Alternatives : Viridis, Magma, et Cividis pour le daltonisme.
- Divergente « Glace / Feu » pour le mode comparaison : #4CC9F0 ← #1A1E26 → #F77F00, centrée sur un neutre sombre.
- Option « Lueur » (désactivée par défaut) : un léger halo additif sur le 1 % de versets les plus incertains.

### Typographie

| Rôle | Police | Taille |
|---|---|---|
| Interface | Inter, chiffres tabulaires | 13 px (base), 11 px (secondaire) |
| Références, valeurs | JetBrains Mono | 12 px |
| Titres de l’inspecteur | Inter semi-gras | 20 px et 28 px |
| Grec | Gentium Plus (SIL, OFL) | 16 px et plus |
| Hébreu | Ezra SIL ou Noto Serif Hebrew (OFL) | 18 px et plus, de droite à gauche |
| Autres écritures | Famille Noto, chargée à la demande par écriture | Selon contexte |

Aucun « tofu » (glyphe manquant) n’est toléré : l’ingestion produit la liste des écritures du corpus, et un test vérifie que chacune a une police. Les polices sont livrées dans assets/fonts/ avec leurs licences.

### Formes et mouvement

- Rayons : 8 px pour les panneaux, 6 px pour les contrôles. Les cellules ont des coins arrondis à 15 % de leur taille à partir de Z2, et des coins vifs avant, pour la netteté.
- Ombres : aucune, sauf une ombre légère sous les menus et infobulles.
- Interface : transitions de 150 à 200 ms, ease-out.
- Caméra : les vols vers un livre ou un verset suivent l’algorithme de zoom-panoramique fluide de van Wijk et Nuij, en 450 à 700 ms selon la distance. Le zoom à la molette a de l’inertie.
- Accessibilité : si le système demande de réduire les animations, toutes les transitions deviennent instantanées.
- Thème clair : hors périmètre, mais les jetons permettent de l’ajouter sans toucher aux composants.

## 10. Interactions et raccourcis

Tout se fait à la souris, et tout se fait aussi au clavier. Le zoom est toujours ancré sous le curseur.

| Action | Souris / pavé tactile | Clavier |
|---|---|---|
| Zoomer, dézoomer | Molette, pincement | + / - |
| Se déplacer | Glisser (clic gauche ou milieu) | Flèches avec Maj, ou Espace maintenu + souris |
| Voler vers un élément | Double-clic sur un livre, un chapitre, un verset | Entrée sur la sélection |
| Tout afficher | — | F |
| Sélectionner un verset | Clic | — |
| Verset précédent / suivant | — | ← / → |
| Chapitre précédent / suivant | — | ↑ / ↓ |
| Rechercher | Clic sur la barre | Ctrl+K ou / |
| Changer de vue | Contrôle segmenté | 1 Mur, 2 Hilbert, 3 Piste, 4 Galaxie |
| Changer de métrique | Menu déroulant | Tab / Maj+Tab |
| Mode comparaison | Bouton | C |
| Afficher ou masquer les panneaux | Boutons | [ gauche, ] droit |
| Statistiques de rendu | — | F3 |
| Capture et mode poster | Bouton | Ctrl+S / Ctrl+Maj+S |
| Désélectionner, fermer | Clic dans le vide | Échap |

État persistant. Le viewer mémorise la dernière vue, la caméra, la métrique, les poids et les filtres dans un fichier de config. Une commande babel view --at "Jn 1:1" --metric sem ouvre directement un état donné.

## 11. Rendu et performance du viewer

41 900 cellules, c’est peu pour un GPU de jeu : tout le canevas se dessine en un seul appel instancié. L’effort porte sur la réactivité (rien ne se recalcule inutilement) et sur le texte multi-écritures.

### Architecture du rendu

- Coquille : eframe / egui avec le backend wgpu pour les panneaux. Le canevas central est un rendu wgpu personnalisé intégré via les callbacks de egui-wgpu.
- Cellules : des quads instanciés, un seul draw call. Coins arrondis et anticrénelage calculés dans le fragment shader par fonction de distance signée (SDF), sans MSAA.
- Buffers GPU :
  - positions par vue et par verset (vec2), soit environ 1,3 Mo pour quatre vues ;
  - valeurs par métrique et par verset (f32), environ 1,7 Mo pour dix métriques ;
  - une texture LUT par palette ;
  - des uniformes : caméra, vue de départ et d’arrivée, avancement de la transition, métrique, palette, verset survolé et sélectionné.
- Changer de métrique, de palette ou de vue ne touche qu’aux uniformes : aucune géométrie n’est renvoyée au GPU. La transition entre vues est interpolée dans le vertex shader.
- Composite à poids variables : recalculé sur CPU (41 900 × 5 valeurs, moins d’une milliseconde) puis écrit dans le buffer de valeurs.
- Filtres de langues : les z par traduction des signaux principaux restent en mémoire (environ 150 Mo en f16). Le ré-agrégat se fait sur CPU avec rayon.
- Pyramide d’agrégats : moyennes par chapitre et par livre précalculées pour les niveaux de zoom où une cellule fait moins de 2 px.
- Sélection au survol : inversion analytique de la disposition (grille, courbe de Hilbert, recherche binaire pour la Piste). Aucune relecture GPU.
- Texte dans le canevas : glyphon (sur cosmic-text) pour les noms de livres, numéros et cartes de verset. Le façonnage passe par cosmic-text, qui gère le grec, l’hébreu de droite à gauche et les écritures complexes.
- Texte dans l’inspecteur : le rendu de texte d’egui gère mal le façonnage complexe et le sens droite-à-gauche. Les blocs de texte non latins sont donc façonnés par cosmic-text et affichés comme textures dans egui, sauf si le développeur trouve une intégration plus directe et la justifie dans DECISIONS.md.
- Textes à la demande : texts.zst est découpé en blocs (traduction × livre) chargés par un thread d’arrière-plan, avec un cache LRU de 256 Mo.
- Rendu réactif : une image n’est produite que sur une entrée ou pendant une animation. Au repos, le GPU ne travaille pas. VSync activée ; mode non plafonné pour les mesures.
- Écrans haute densité : prise en charge de la mise à l’échelle Windows de 100 à 200 %.
- Profilage : puffin ou tracing-tracy. L’overlay F3 affiche le temps CPU et GPU par image (requêtes d’horodatage wgpu si le matériel les prend en charge).

### Objectifs de performance

| Mesure | Objectif sur la machine cible |
|---|---|
| Images par seconde en zoom et panoramique, vue Mur, 2560×1440 | ≥ 144, jamais sous 120 |
| Temps GPU par image | ≤ 3 ms |
| Démarrage à froid jusqu’à la première image | < 1,5 s |
| Changement de métrique ou de palette | Visible à l’image suivante |
| Recalcul du composite après un curseur de poids | < 16 ms |
| Recalcul après un filtre de langues | < 200 ms |
| Ouverture de l’inspecteur, textes compris | < 100 ms |
| Mémoire vive du viewer | < 1,5 Go |
| Charge GPU au repos | Proche de 0 % |

## 12. Phases et critères d’acceptation

Cinq phases, chacune livrable et démontrable. La phase 1 donne déjà une carte de chaleur utilisable sans GPU pour le calcul ; les phases suivantes l’enrichissent. Une phase n’est terminée que lorsque toutes ses cases sont cochées.

### Phase 0 — Socle et ingestion

Workspace, README.md, DECISIONS.md, babel doctor, fetch, ingest, rapport qualité.

- [ ] babel fetch puis babel ingest tournent sans erreur sur le corpus complet
- [ ] Chaque traduction ingérée a exactement autant de lignes que vref.txt
- [ ] Le rapport qualité liste la couverture par livre, les décalages de versification, les doublons et les écritures sans police
- [ ] Tests sur le mini-corpus synthétique : lignes vides, <range>, normalisation NFC
- [ ] babel doctor affiche le GPU, la VRAM, le fournisseur d’exécution disponible et l’espace disque
- [ ] cargo fmt, cargo clippy -- -D warnings et cargo test passent

### Phase 1 — Signaux sans modèle et premier viewer

S_omit, S_len, S_intra, composite sans bootstrap, atlas.bin, vue Mur avec zoom, panoramique, survol, infobulle, recherche de références, thème sombre et palette Braise.

- [ ] Tests de santé du § 5 passés, ou écart expliqué dans DECISIONS.md
- [ ] La Bible entière est visible au lancement et le zoom descend jusqu’au verset
- [ ] ≥ 144 images par seconde en zoom et panoramique
- [ ] La recherche « Jn 3:16 » déclenche un vol vers le verset
- [ ] Tests de propriétés : des traductions identiques donnent une dispersion nulle ; l’ordre des traductions ne change rien

### Phase 2 — Signaux lourds et validation

Alignement (S_align et u(w)), embeddings GPU (S_sem), bootstrap, décomposition textuel / interprétatif, rapport de validation.

- [ ] Choix du modèle consigné dans DECISIONS.md, avec les résultats du test de sensibilité
- [ ] babel embed reprend après une interruption sans refaire le travail déjà fait
- [ ] Utilisation GPU > 90 % pendant embed
- [ ] Rapport de validation complet : AUC, Spearman, corrélations partielles, baselines, séparation calibration / test
- [ ] Pipeline complet dans les budgets du § 7, ou écarts mesurés et expliqués

### Phase 3 — Interface complète

Inspecteur, vues Hilbert et Piste, transitions animées, panneau Couches (poids et filtres), mode comparaison, zoom Z3 et Z4, minimap, fil d’Ariane, raccourcis, mode poster, état persistant.

- [ ] Tous les objectifs du § 11 mesurés et atteints
- [ ] L’hébreu s’affiche correctement de droite à gauche (capture de référence dans les tests)
- [ ] Aucun glyphe manquant sur un échantillon d’une traduction par écriture
- [ ] Le mode poster exporte une image de 16 384 px de côté
- [ ] Tous les raccourcis du § 10 fonctionnent

### Phase 4 — Extensions optionnelles

- Vue Galaxie (projection des embeddings)
- Arcs de références croisées OpenBible
- Décomposition pour l’AT (texte massorétique contre Septante)
- Comparaison avec la confiance d’un LLM traducteur : hésite-t-il là où les humains divergent ?
- Version web du viewer (WebAssembly + WebGPU via eframe), en mode publication

## 13. Règles de travail pour le développeur

Ce document est la référence. Copie-le à la racine du dépôt sous le nom SPEC.md, lis-le en entier avant d’écrire du code, et avance phase par phase.

### Déroulement

- Une phase à la fois, dans l’ordre. Chaque phase se termine par une commande de démonstration, ses cases cochées et un commit.
- README.md reste à jour : commandes, structure, conventions, état d’avancement.
- Chaque décision technique non fixée ici est consignée dans DECISIONS.md au format ADR : contexte, options envisagées, choix, conséquences, mesures à l’appui.

### Décisions verrouillées

Ne pas les changer sans accord explicite :

- Rust pour tout le runtime ; Python seulement dans les deux exceptions du § 2
- eBible et STEPBible comme sources ; VerseId = ligne de vref.txt
- Normalisation robuste par traduction avant toute agrégation
- Validation avec séparation calibration / test par livres
- wgpu + egui pour le viewer ; dark mode, palette Braise par défaut
- Aucun service en ligne au runtime

### Décisions déléguées

Le développeur choisit, mesure et documente :

- le modèle d’embedding, selon le protocole du § 7 ;
- la tokenisation des langues sans espaces ;
- les seuils (redondance, couverture, intervalles) ;
- la disposition exacte de la vue Mur ;
- l’intégration du texte complexe dans egui ;
- l’outil de profilage.

### Demander avant de…

- télécharger plus de 2 Go d’un coup (un modèle, par exemple) ;
- occuper plus de 50 Go de disque ;
- installer un outil système (CUDA, cuDNN, TensorRT) ;
- modifier une décision verrouillée ou introduire du Python.

### Qualité du code

- Pas de unwrap() hors des tests. unsafe interdit, sauf FFI imposée et commentée.
- Les fonctions de calcul sont pures et testées séparément des entrées-sorties.
- Tous les paramètres vivent dans config/babel.toml ; aucun nombre magique dans le code.
- Chaque signal a un test sur le mini-corpus synthétique, avec une divergence injectée à la main et un classement attendu connu.
- Benchmarks criterion pour chrF, l’alignement et le scoring. Mesurer avant d’optimiser, et noter les chiffres.

### Intégrité scientifique

- Ne jamais ajuster un signal pour faire passer un test de santé ou améliorer l’AUC sur la partie test.
- Un résultat décevant est rapporté tel quel, avec son analyse.

## 14. Risques et hypothèses à confirmer

Le risque principal est scientifique, pas technique : que le score mesure autre chose que l’incertitude (la longueur, la mémoire du modèle, des traductions recopiées). Chaque risque a une parade prévue ailleurs dans ce document.

| Risque | Effet | Parade |
|---|---|---|
| Le modèle d’embedding a mémorisé la Bible | Désaccord sous-estimé dans S_sem | Test de sensibilité (§ 5), signaux sans modèle comme garde-fou |
| Traductions non indépendantes | Faux consensus | Pondération par groupes (§ 4.2) |
| Le score reflète surtout la longueur | Résultat trivial | Corrélations partielles et baselines (§ 5) |
| Décalages de versification | Faux pics d’incertitude | Détection à l’ingestion, liste d’exclusion (§ 3) |
| Poids OmniSONAR indisponibles ou licence restrictive | Couverture réduite | Repli SONAR ou LaBSE, pondération par qualité (§ 7) |
| Écritures sans espaces | S_align biaisé | Tokenisation en caractères ou exclusion documentée (§ 4.1) |
| Installation CUDA laborieuse sous Windows | Calcul bloqué | Repli DirectML puis CPU, procédure dans README.md |
| Licences NC-ND | Texte non publiable | Mode publication (§ 3) |

### Hypothèses par défaut

- GPU NVIDIA avec au moins 8 Go de VRAM.
- Canon : toutes les lignes de vref.txt, deutérocanoniques compris ; un filtre « canon protestant » existe dans le viewer.
- Traductions de référence affichées par défaut : Louis Segond 1910 et King James Version, toutes deux dans le domaine public, si elles figurent dans le corpus.
- Interface en français, noms de livres en français et en anglais.
- Projet local : pas de publication web avant la phase 4.

## Sources

- [eBible corpus (BibleNLP)](https://github.com/BibleNLP/ebible) : format, versification, plages, licences
- [STEPBible Data](https://github.com/STEPBible/STEPBible-Data) : TAGNT, TAHOT, licence CC BY 4.0
- [OmniSONAR, Meta FAIR, mars 2026](https://arxiv.org/html/2603.16606v1) : couverture, tailles de modèles, données d’entraînement
- [ort, ONNX Runtime pour Rust](https://github.com/pykeio/ort) : fournisseurs CUDA, TensorRT, DirectML
