# Journal des décisions techniques

Format ADR : contexte, options, choix, conséquences, mesures à l'appui. Les
décisions verrouillées par le cahier des charges (SPEC.md, § 13) n'y figurent
pas ; celles-ci sont les décisions déléguées ou imposées par le terrain.

Machine des mesures : Windows 10, Core i7-10700K (8 cœurs, 16 threads), 48 Go
de RAM, GeForce RTX 3070 (8 Go), écran 2560 × 1440 à 74 Hz. Mesures du
11 octobre 2026.

---

## ADR 1 — Périmètre du prototype : phases 0 et 1

**Contexte.** Le cahier des charges demande d'avancer phase par phase et
précise que la phase 1 donne déjà une carte de chaleur utilisable sans GPU
pour le calcul.

**Choix.** Le prototype couvre les phases 0 et 1 en entier. Quelques éléments
de la phase 3 sont venus presque gratuitement avec l'architecture du rendu
(positions par vue dans le buffer d'instances, composite recalculé sur CPU) :
vue Hilbert, transition animée, poids en direct, inspecteur, état persistant.

**Conséquences.** `babel align`, `babel embed` et `babel validate` existent
dans la CLI mais répondent qu'ils appartiennent à la phase 2. Aucune vérité
terrain n'est encore lue : rien dans le prototype ne dit si le score prédit
les variantes manuscrites. Les tests de santé ne remplacent pas la validation.

## ADR 2 — Sous-corpus « multi » au lieu du corpus complet

**Contexte.** Le dépôt eBible contient 1 079 fichiers de traduction (2,27 Go
décompressés). Le disque système n'avait que 11,8 Go libres au départ, et la
compilation en consomme près de 5.

**Options.** (a) Corpus complet sur `C:` ; (b) corpus complet sur `D:`, ce qui
demande l'accord de l'utilisateur pour écrire hors du projet ; (c) un
sous-corpus.

**Choix.** (c) : `fetch.selection = "multi"` ne télécharge que les langues qui
ont au moins deux traductions, plus les traductions de référence. Ce sont les
seules langues qui nourrissent S_intra. Le clone git est partiel
(`--filter=blob:none`, profondeur 1) et clairsemé.

**Mesures.** 278 fichiers, 917 Mo sur disque, téléchargés en 2 min 12 s.
43 fichiers sont vides dans le dépôt et écartés ; restent 235 traductions,
74 langues, 4 033 894 textes.

**Conséquences.** S_omit et S_len reposent sur 235 traductions au lieu d'un
millier, et les langues à traduction unique manquent. Les livres
deutérocanoniques ont rarement cinq traductions éligibles : 4 330 versets
sur 41 899 n'ont aucun score. Passer au corpus complet : `selection = "all"`
dans `config/babel.toml`, puis `babel fetch --yes` et `babel run`.

## ADR 3 — Environnement de compilation C propre à la machine

**Contexte.** `zstd-sys` compile du C. MSVC est installé dans `C:\VS` sans
entrée de registre : `cc` trouve `cl.exe` mais pas les en-têtes.

**Choix.** Un `.cargo/config.toml` local, non suivi par git, fixe `INCLUDE` et
`LIB` avec `force = false` (un environnement déjà configuré garde la main).

**Conséquences.** Sur une autre machine avec une installation Visual Studio
normale, ce fichier est inutile.

## ADR 4 — Les textes sources ne sont pas des traductions

**Contexte.** Le corpus contient le grec (`grc-*` : Texte reçu, byzantin,
Septante de Brenton…) et l'hébreu (`hbo-*`).

**Choix.** `config/exclusions.toml` les exclut des signaux par code de langue.
Ils restent dans `corpus.parquet` : la phase 2 en aura besoin pour le contrôle
croisé de la vérité terrain.

**Mesures.** 9 traductions exclues sur 235.

## ADR 5 — S_omit n'est pas normalisé par traduction

**Contexte.** Le § 4.2 demande de centrer-réduire chaque mesure par
traduction (médiane, MAD). Pour l'omission, la mesure est binaire et vaut 0
presque partout : le MAD est nul et la normalisation n'a pas de sens.

**Choix.** S_omit(v) est la part pondérée par w(t) des traductions éligibles
où le verset est vide ou fusionné, comme l'écrit le tableau du § 4.1.

## ADR 6 — S_len : MAD pondéré des résidus normalisés

**Contexte.** Le § 4.1 définit S_len comme le MAD des résidus du median
polish ; le § 4.2 impose une normalisation robuste par traduction avant toute
agrégation.

**Choix.** Les deux à la fois : le résidu ε(t, v) est centré-réduit par
traduction, puis S_len(v) est le MAD, pondéré par w(t), des z(t, v). Le median
polish lui-même n'est pas pondéré.

**Conséquences à surveiller.** Un MAD ignore par construction toute minorité
de moins de la moitié du poids : si une variante longue n'est suivie que par
30 % des traductions, le MAD ne la voit pas. Sur ce sous-corpus le test de
santé passe largement (1 Jn 5:7 est premier du NT), parce que les traductions
se partagent à peu près à égalité sur ce verset. Sur un corpus où le texte
critique domine, le résultat peut changer ; il ne faudra pas régler le signal
pour autant, mais le signaler et, si l'utilisateur le décide, comparer avec
une moyenne tronquée des |z|.

## ADR 7 — S_intra : chrF symétrique, par langue et par écriture

**Contexte.** chrF distingue hypothèse et référence (β = 2 favorise le
rappel) ; entre deux traductions, aucune n'est la référence.

**Choix.**
- chrF symétrique : moyenne des deux sens. Espaces retirés, casse conservée,
  comme sacreBLEU.
- Les traductions sont comparées à l'intérieur d'un groupe (langue,
  écriture) : deux écritures d'une même langue n'ont aucun n-gramme commun.
- Distance d'un verset dans une langue : moyenne des `1 − chrF` par paires,
  pondérée par le produit des poids w.
- Normalisation robuste par langue, sur les versets.
- Moyenne sur les langues pondérée par (nombre de traductions indépendantes
  présentes − 1), plafonnée à `signals.intra_language_weight_cap` (8) pour que
  l'anglais et ses 47 fichiers ne dictent pas le signal.

**Conséquences.** Une langue dont toutes les traductions sont redondantes
(une seule voix) ne contribue pas. Les langues sans espaces n'ont pas besoin
de tokenisation ici : chrF travaille déjà en caractères. La question se
posera pour S_align, en phase 2.

## ADR 8 — Groupes de redondance

**Choix.** Union-find par langue. Deux traductions sont réunies quand leur
chrF moyen dépasse `redundancy_chrf` (0,80), mesuré sur un verset tous les 16
pour ne pas doubler le coût de S_intra. Entre deux écritures différentes
d'une même langue (translittérations, comme le sanskrit publié dans vingt
écritures), le critère est la corrélation de leurs résidus de longueur
(> 0,90). Chaque groupe pèse 1.

**Mesures.** 226 traductions non exclues forment 177 groupes indépendants.

## ADR 9 — Rang percentile : part des valeurs strictement inférieures

**Contexte.** U(v) combine des rangs percentiles. Environ 80 % des versets
ont une omission nulle. Avec le rang moyen des ex æquo, ils recevaient tous
le rang 0,4 et apparaissaient en violet moyen sur la carte, comme s'ils
étaient modérément incertains.

**Choix.** rang(v) = part des versets de valeur strictement inférieure, dans
[0, 1[. Un verset que personne n'omet a le rang 0.

**Conséquences.** Décision prise sur un critère de lecture de la carte, avant
toute confrontation à une vérité terrain. Le rang amplifie les petites
valeurs : dès qu'une seule traduction sur cent omet un passage, ces versets
passent au-dessus de 0,8 en S_omit. La vue « Omission » le montre dans le
début de la Genèse ; c'est un effet du rang, pas un bug.

## ADR 10 — Détection des décalages de versification

**Contexte.** Le § 3 demande de détecter les versets qui ressemblent plus au
consensus de v±1 qu'à celui de v. Sans embeddings en phase 0, il faut un
critère indépendant de la langue.

**Choix.** Le profil des log-longueurs. Pour chaque (traduction, chapitre)
d'au moins 12 versets, on compare la corrélation avec le consensus aligné et
avec le consensus décalé d'un verset ; le chapitre est signalé si le décalage
gagne plus de 0,25.

**Mesures.** 1 048 chapitres signalés dans 54 traductions. Les mêmes
chapitres reviennent partout : Gn 32, Ex 22, Nb 30, Dt 13, 1 S 24 et
plusieurs Psaumes. Ce sont les différences connues entre la numérotation
hébraïque et la numérotation anglaise. Les deux textes hébreux eux-mêmes sont
signalés : le consensus du sous-corpus suit la numérotation anglaise.

**Conséquences.** eBible ne ramène donc pas toutes les traductions à une
versification commune, et ces chapitres portent de faux pics dans S_len et
S_intra. Le rapport les liste et `exclusions.suggested.toml` propose
d'exclure les traductions les plus touchées ; rien n'est exclu d'office.
Exclure une traduction entière pour soixante chapitres est grossier. Une
parade plus fine serait de rendre inéligibles les seuls couples (traduction,
chapitre) signalés. Elle n'est pas faite : elle change le calcul des signaux
et mérite l'accord de l'utilisateur.

## ADR 11 — Atlas en rkyv validé, sans memmap

**Contexte.** Le cahier des charges recommande rkyv et memmap2 pour un
chargement zéro-copie. `memmap2::Mmap::map` est `unsafe`, que le § 13
interdit hors FFI.

**Choix.** `atlas.bin` = en-tête de 16 octets (signature, version) puis
charge utile rkyv, relue en entier et validée (`bytecheck`), puis
désérialisée.

**Mesures.** 2,1 Mo ; la lecture ne se voit pas au démarrage. Le budget du
cahier des charges est de 50 Mo.

**Conséquences.** Si l'atlas grossit (z par traduction pour les filtres de la
phase 3), il faudra revoir ce point, par exemple avec `rkyv::access` sur un
tampon aligné, qui reste sûr.

## ADR 12 — Disposition de la vue Mur : colonnes de journal

**Contexte.** La disposition exacte est déléguée. Premier essai : un bloc par
livre, posé sur une étagère. Les petits livres (Abdias, 2 Jean) occupaient
une colonne entière : 528 × 374 cellules pour 41 899 versets, soit 21 % de
remplissage.

**Choix.** Dans une section du canon, les livres s'empilent dans des colonnes
de hauteur fixe, comme un texte de journal ; un livre long continue dans la
colonne suivante ; un chapitre n'est jamais coupé ; chaque livre a trois
rangées d'en-tête pour son nom. La hauteur d'étagère et le nombre d'étagères
sont choisis par recherche exhaustive pour maximiser la taille de cellule
dans un cadre de rapport 1,55.

**Mesures.** 409 × 271 cellules (38 % de remplissage). En plein écran
2560 × 1440 avec le panneau Couches ouvert, une cellule ferait environ 4,6 px
(calculé ; 3,0 px mesurés dans une fenêtre de 1600 × 960).

**Écart avec le cahier des charges.** Le § 8 annonce 7 à 9 px. Ce n'est pas
atteignable avec une rangée par chapitre et un retour à la ligne à 40 : un
chapitre compte 26 versets en moyenne, donc un tiers de chaque rangée reste
vide. Leviers possibles : un retour à la ligne plus court, des marges plus
serrées, ou masquer les deutérocanoniques (filtre « canon protestant »).

## ADR 13 — Texture LUT en demi-flottants

**Contexte.** Le § 9 demande un test : la luminosité OKLab doit augmenter sur
les 256 texels. Sur 8 bits par canal, le premier segment de Braise
(#262B52 → #4B2C73) n'a pas 51 couleurs distinctes : deux texels voisins
étaient identiques.

**Choix.** La LUT est une texture `Rgba16Float` de 256 × 1. Le test mesure la
luminosité sur le contenu exact de la texture. Les valeurs sont stockées en
sRGB encodé, parce que la cible de rendu d'egui n'est pas en sRGB.

## ADR 14 — Texte du canevas par egui, polices par défaut

**Contexte.** Le § 11 prévoit glyphon et cosmic-text pour le canevas, Inter
et JetBrains Mono pour l'interface, Gentium et Ezra SIL pour le grec et
l'hébreu.

**Choix du prototype.** Les libellés du canevas sont dessinés par le peintre
d'egui, par-dessus le rendu wgpu, avec les polices par défaut d'egui. En
phase 1, le viewer n'affiche que du texte latin (noms de livres, Segond, KJV).

**Conséquences.** Aucune police n'est livrée dans `assets/fonts/`. Le rapport
qualité liste 20 écritures du sous-corpus sans police. L'hébreu de droite à
gauche et le façonnage complexe restent à faire avec le texte source, en
phases 2 et 3.

## ADR 15 — Performance du viewer et outil de mesure

**Contexte.** Objectif : ≥ 144 images/s en zoom et panoramique, jamais sous
120. L'écran est à 74 Hz, donc la VSync masque tout.

**Choix.** Une mesure intégrée plutôt qu'un profileur externe :
`babel view --bench N` coupe la VSync, fait zoomer et tourner la caméra sur
toute la carte (du cadrage complet à 48 px par cellule, deux allers-retours)
et affiche moyenne, 99e centile et pire image. puffin ou tracy pourront
s'ajouter quand il y aura un point chaud à localiser.

**Mesures.** Canevas de 2280 × 1324 px, vue Mur, 3 000 images, deux passes :

| | Passe 1 | Passe 2 |
|---|---:|---:|
| Moyenne | 1,03 ms (972 img/s) | 1,04 ms (958 img/s) |
| 99e centile | 4,66 ms (215 img/s) | 4,11 ms (243 img/s) |
| Pire image | 6,30 ms (159 img/s) | 5,80 ms (172 img/s) |
| Images sous 120 img/s | 0 | 0 |
| CPU par image | 0,34 ms | 0,34 ms |

**Ce que la mesure a appris.** Avant correction, la pire image durait 64 ms :
la taille des étiquettes suivait le zoom en continu, donc chaque image
demandait une taille de police inédite et de nouveaux glyphes à tramer.
Arrondir les tailles au pixel a divisé le temps CPU par 3,5.

**Limites.** La fenêtre de mesure faisait 2560 × 1400, pas le plein écran. Le
temps GPU seul n'est pas isolé (pas encore de requêtes d'horodatage wgpu). La
mesure ne couvre ni la vue Hilbert ni les cartes de verset (Z3).

## ADR 16 — Cache par empreinte, avec version de calcul

**Choix.** Chaque étape écrit dans `data/cache/` une empreinte blake3 de sa
configuration, du contenu de ses entrées et de l'empreinte de l'étape
précédente. `babel run` saute les étapes à jour.

**Leçon.** L'empreinte ne voit pas le code. Après le changement de la
définition du rang (ADR 9), le cache a servi l'ancien résultat. Chaque étape
porte donc une version (`Stage::version()`), à incrémenter quand son calcul
change.

## ADR 17 — Chiffres de référence du calcul

Pipeline complet sur le sous-corpus, sans cache : 52 s, dont ingestion 36 s,
signaux 15 s, score 0,1 s, atlas 0,5 s. Le budget du § 7 est de 30 minutes
pour le corpus complet, quatre à cinq fois plus gros.

`cargo bench` (Criterion) :

| Mesure | Temps |
|---|---:|
| chrF, profil d'un verset (ordres 1 à 6) | 7,1 µs |
| chrF symétrique, une paire de profils | 3,7 µs |
| Median polish, 200 traductions × 5 000 versets | 10,3 ms |
| Rangs percentiles d'un signal (41 899 versets) | 1,5 ms |
| Composite à poids variables (41 899 versets) | 0,18 ms |
| Rangs et composite complets | 6,0 ms |

Le recalcul du composite après un curseur de poids tient donc largement dans
les 16 ms demandés.

## ADR 18 — Tests de santé : résultats

Sur le sous-corpus, rang dans le Nouveau Testament (7 957 versets) :

| Signal | Verset | Valeur | Rang | Résultat |
|---|---|---:|---:|---|
| S_omit | Actes 8:37 | 0,224 | 5 | réussi |
| S_omit | Luc 17:36 | 0,223 | 6 | réussi |
| S_omit | Matthieu 18:11 | 0,114 | 43 | réussi |
| S_omit | Matthieu 17:21 | 0,088 | 67 | réussi |
| S_len | 1 Jean 5:7 | 3,155 | 1 | réussi |

Aucun paramètre n'a été modifié après avoir vu ces résultats. Ils valent pour
235 traductions et restent à refaire sur le corpus complet.

Observation à creuser : la base textuelle inférée (§ 4.4) classe 184
traductions en « Texte reçu / byzantin », 11 en « critique », 5 en « mixte »
et 35 en « inconnue ». La proportion de « reçu » paraît élevée ; il est
possible que beaucoup de traductions impriment les versets diagnostiques dans
le corps du texte, entre crochets ou non. À vérifier avant de s'appuyer sur
cette classification en phase 2.

## ADR 19 — Sélection d'une portée et bilan de son contenu

**Contexte.** Demande de l'utilisateur, hors cahier des charges : pouvoir
sélectionner un livre ou une catégorie de livres et en obtenir un bilan, comme
pour un verset. Le viewer ne savait sélectionner qu'un verset.

**Options.** (a) Un mode « bilan » à part, avec sa propre liste de livres ;
(b) précalculer les agrégats dans l'atlas ; (c) généraliser la sélection et
calculer le bilan dans le viewer.

**Choix.** (c). La sélection devient une portée : verset, chapitre, livre ou
section du canon. Le chapitre n'était pas demandé ; il vient sans coût, parce
que la recherche « Jean 3 » et le fil d'Ariane le désignaient déjà. Toute
portée est une plage contiguë de versets, ce qui tient en deux uniformes : le
shader estompe les cellules hors de la plage, sans renvoyer de géométrie.

Le bilan est calculé à la sélection à partir du composite courant
(`crates/babel-view/src/scope.rs`, fonctions pures) et gardé en cache tant
que ni la sélection ni les poids ne changent. Le précalcul (b) est écarté :
U dépend des poids, réglables en direct.

Contenu du bilan : U moyen et médian des versets notés ; rang parmi les pairs
(livres entre eux, sections entre elles, chapitres entre eux) par U moyen
décroissant ; part de versets « très incertains » (rang dans la Bible ≥
`view.hot_rank`, soit le top 10 %) ; rang percentile moyen de chaque signal ;
histogramme de U avec la Bible entière en repère ; détail du contenu ;
versets les plus incertains.

**Sélection sur la carte.** Les noms de livres et de sections dessinés
deviennent des zones cliquables, relevées à chaque image et testées à la
suivante. Ils passent avant les cellules.

**Conséquences.**
- Un livre sans verset noté (plusieurs deutérocanoniques sur le sous-corpus)
  affiche « aucun verset noté » et n'a pas de rang.
- U moyen d'un livre est une moyenne de rangs percentiles : les écarts entre
  livres sont faibles (0,36 à 0,58 dans le Pentateuque et les Évangiles), et
  la grille des chapitres est peu contrastée. Le classement et la part de
  versets très incertains distinguent mieux les livres que la couleur.
- Ces agrégats héritent des défauts connus des signaux : les décalages de
  versification (ADR 10) gonflent le bilan des chapitres concernés.
- Quand l'inspecteur s'ouvre, le canevas rétrécit : si la carte entière était
  affichée elle est recadrée, sinon le contenu garde sa place à l'écran, et
  la caméra ne se déplace que si la sélection est sortie de la vue.

**Vérification.** Dix tests unitaires sur un atlas synthétique (plages, noms,
rangs, histogramme, sélection sans données, voisins). Clic sur un nom de
section et de livre vérifié en vue Mur et en vue Hilbert par clic simulé
(`--pointer X,Y --click`). Non essayé à la main : le double-clic sur un nom
et la navigation au clavier entre livres.
