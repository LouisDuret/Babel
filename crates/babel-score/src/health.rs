//! Tests de santé du § 5 : des versets connus pour leurs variantes doivent
//! se classer en tête du Nouveau Testament.
//!
//! Ces résultats sont attendus mais pas garantis. Un échec déclenche une
//! enquête (bug d'ingestion, versification), jamais un réglage du score.

use babel_core::config::Health;
use babel_core::{SignalKind, Testament, VerseId, Versification};
use std::fmt::Write as _;

/// Marge d'arrondi sur la frontière « rang ≤ part × population ».
const RANK_TOLERANCE: f32 = 1e-3;

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Le verset est dans la part supérieure attendue.
    Passed,
    Failed,
    /// Le verset est absent de la versification ou n'a pas de valeur.
    NotEvaluated(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HealthCheck {
    pub signal: SignalKind,
    /// Référence au format de vref.txt : `ACT 8:37`.
    pub reference: String,
    pub value: Option<f32>,
    /// Rang dans le NT, 1 = le plus incertain. Les ex æquo partagent le
    /// meilleur rang.
    pub rank: Option<usize>,
    /// Nombre de versets du NT qui ont une valeur.
    pub population: usize,
    pub outcome: Outcome,
}

impl HealthCheck {
    /// Part supérieure du NT où se situe le verset (rang / population).
    pub fn share(&self) -> Option<f32> {
        self.rank.map(|r| r as f32 / self.population.max(1) as f32)
    }
}

fn parse_reference(versification: &Versification, reference: &str) -> Option<VerseId> {
    let (code, rest) = reference.split_once(' ')?;
    let (chapter, verse) = rest.split_once(':')?;
    versification.find_usfm(
        code,
        chapter.trim().parse().ok()?,
        verse.trim().parse().ok()?,
    )
}

/// Classe un verset parmi ceux du Nouveau Testament.
pub fn check(
    versification: &Versification,
    signal: SignalKind,
    values: &[f32],
    reference: &str,
    top_share: f32,
) -> HealthCheck {
    let nt: Vec<f32> = values
        .iter()
        .enumerate()
        .filter(|(v, x)| versification.testament_of(*v) == Testament::New && x.is_finite())
        .map(|(_, x)| *x)
        .collect();
    let mut result = HealthCheck {
        signal,
        reference: reference.to_string(),
        value: None,
        rank: None,
        population: nt.len(),
        outcome: Outcome::Failed,
    };
    let Some(id) = parse_reference(versification, reference) else {
        result.outcome = Outcome::NotEvaluated("verset absent de la versification".into());
        return result;
    };
    let value = values.get(id.index()).copied().unwrap_or(f32::NAN);
    if !value.is_finite() {
        result.outcome = Outcome::NotEvaluated("pas de valeur pour ce verset".into());
        return result;
    }
    let rank = 1 + nt.iter().filter(|x| **x > value).count();
    result.value = Some(value);
    result.rank = Some(rank);
    result.outcome = if rank as f32 <= top_share * nt.len() as f32 + RANK_TOLERANCE {
        Outcome::Passed
    } else {
        Outcome::Failed
    };
    result
}

/// Exécute tous les tests de santé configurés.
pub fn run(
    versification: &Versification,
    omit: &[f32],
    len: &[f32],
    config: &Health,
) -> Vec<HealthCheck> {
    let omit_checks = config
        .omit_verses
        .iter()
        .map(|r| check(versification, SignalKind::Omit, omit, r, config.top_share));
    let len_checks = config
        .len_verses
        .iter()
        .map(|r| check(versification, SignalKind::Len, len, r, config.top_share));
    omit_checks.chain(len_checks).collect()
}

/// Rapport Markdown des tests de santé.
pub fn render_markdown(checks: &[HealthCheck], config: &Health) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Tests de santé (§ 5)\n");
    let _ = writeln!(
        out,
        "Chaque verset doit se classer dans le top {:.1} % du Nouveau Testament pour son signal. \
         Ces résultats sont attendus mais pas garantis : un échec déclenche une enquête, jamais un \
         réglage du score.\n",
        config.top_share * 100.0
    );
    let _ = writeln!(
        out,
        "| Signal | Verset | Valeur | Rang dans le NT | Part supérieure | Résultat |"
    );
    let _ = writeln!(out, "|---|---|---:|---:|---:|---|");
    for c in checks {
        let value = c.value.map_or("—".to_string(), |v| format!("{v:.4}"));
        let rank = c
            .rank
            .map_or("—".to_string(), |r| format!("{r} / {}", c.population));
        let share = c
            .share()
            .map_or("—".to_string(), |s| format!("{:.2} %", s * 100.0));
        let outcome = match &c.outcome {
            Outcome::Passed => "réussi".to_string(),
            Outcome::Failed => "**échec**".to_string(),
            Outcome::NotEvaluated(why) => format!("non évalué ({why})"),
        };
        let _ = writeln!(
            out,
            "| S_{} | {} | {} | {} | {} | {} |",
            c.signal.code(),
            c.reference,
            value,
            rank,
            share,
            outcome
        );
    }
    let passed = checks
        .iter()
        .filter(|c| c.outcome == Outcome::Passed)
        .count();
    let _ = writeln!(out, "\n{passed} test(s) réussi(s) sur {}.", checks.len());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versification() -> Versification {
        // 5 versets d'AT puis 200 de NT.
        let mut vref: String = (1..=5).map(|i| format!("GEN 1:{i}\n")).collect();
        vref.extend((1..=200).map(|i| format!("ACT 8:{i}\n")));
        Versification::parse(&vref).expect("vref valide")
    }

    #[test]
    fn rang_dans_le_nt_seulement() {
        let v = versification();
        let mut values = vec![0.0_f32; 205];
        // Des valeurs élevées dans l'AT ne comptent pas.
        values[..5].fill(9.0);
        values[5 + 36] = 0.5; // ACT 8:37
        values[5 + 100] = 0.7;
        let c = check(&v, SignalKind::Omit, &values, "ACT 8:37", 0.01);
        assert_eq!(c.population, 200);
        assert_eq!(c.rank, Some(2));
        assert_eq!(c.outcome, Outcome::Passed);
        assert_eq!(c.share(), Some(0.01));

        // Troisième sur 200 : hors du top 1 %.
        values[5 + 101] = 0.6;
        let c = check(&v, SignalKind::Omit, &values, "ACT 8:37", 0.01);
        assert_eq!(c.rank, Some(3));
        assert_eq!(c.outcome, Outcome::Failed);
    }

    #[test]
    fn ex_aequo_au_meilleur_rang() {
        let v = versification();
        let values = vec![0.0_f32; 205];
        let c = check(&v, SignalKind::Omit, &values, "ACT 8:37", 0.01);
        assert_eq!(c.rank, Some(1));
    }

    #[test]
    fn verset_absent_ou_sans_valeur() {
        let v = versification();
        let mut values = vec![0.0_f32; 205];
        let c = check(&v, SignalKind::Len, &values, "1JN 5:7", 0.01);
        assert!(matches!(c.outcome, Outcome::NotEvaluated(_)));
        values[5 + 36] = f32::NAN;
        let c = check(&v, SignalKind::Omit, &values, "ACT 8:37", 0.01);
        assert!(matches!(c.outcome, Outcome::NotEvaluated(_)));
        assert_eq!(c.population, 199);
    }

    #[test]
    fn rapport() {
        let v = versification();
        let mut values = vec![0.0_f32; 205];
        values[5 + 36] = 0.5;
        let config = Health {
            top_share: 0.01,
            omit_verses: vec!["ACT 8:37".into()],
            len_verses: vec!["1JN 5:7".into()],
        };
        let checks = run(&v, &values, &values, &config);
        let md = render_markdown(&checks, &config);
        assert!(md.contains("| S_omit | ACT 8:37 | 0.5000 | 1 / 200 | 0.50 % | réussi |"));
        assert!(md.contains("| S_len | 1JN 5:7 | — | — | — | non évalué"));
        assert!(md.contains("1 test(s) réussi(s) sur 2."));
    }
}
