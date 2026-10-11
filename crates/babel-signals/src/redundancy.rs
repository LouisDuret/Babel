//! Groupes de traductions redondantes (§ 4.2) : les traductions quasi
//! identiques d'une même langue forment un groupe qui pèse 1 au total.

/// Union-find sur les traductions.
#[derive(Debug, Clone)]
pub struct Groups {
    parent: Vec<usize>,
}

impl Groups {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    /// Réunit les groupes de `a` et de `b`. Le plus petit index devient le
    /// représentant, pour que le résultat ne dépende pas de l'ordre des
    /// appels.
    pub fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            let (low, high) = (ra.min(rb), ra.max(rb));
            self.parent[high] = low;
        }
    }

    /// Étiquette de groupe de chaque élément : l'index du représentant.
    pub fn labels(&mut self) -> Vec<u32> {
        (0..self.parent.len())
            .map(|i| self.find(i) as u32)
            .collect()
    }

    /// Poids w(t) = 1 / taille du groupe.
    pub fn weights(&mut self) -> Vec<f32> {
        let labels = self.labels();
        let mut size = vec![0_u32; labels.len()];
        for label in &labels {
            size[*label as usize] += 1;
        }
        labels
            .iter()
            .map(|label| 1.0 / size[*label as usize] as f32)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaque_groupe_pese_un() {
        let mut g = Groups::new(6);
        g.union(0, 1);
        g.union(1, 2);
        g.union(4, 5);
        assert_eq!(g.labels(), vec![0, 0, 0, 3, 4, 4]);
        let w = g.weights();
        assert_eq!(w, vec![1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0, 1.0, 0.5, 0.5]);
        // Le poids total est le nombre de groupes.
        assert!((w.iter().sum::<f32>() - 3.0).abs() < 1e-6);
    }

    #[test]
    fn independant_de_l_ordre_des_unions() {
        let mut a = Groups::new(5);
        a.union(3, 4);
        a.union(0, 3);
        let mut b = Groups::new(5);
        b.union(0, 3);
        b.union(4, 3);
        assert_eq!(a.labels(), b.labels());
    }
}
