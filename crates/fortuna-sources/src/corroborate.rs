//! Layer 2 corroboration (design §4.4): collapse syndication so it cannot
//! launder a single-source claim into fake consensus. "Ten outlets carrying
//! one wire story are ONE origin, not ten."
//!
//! This module does the DETERMINISTIC half of Layer 2: near-duplicate text
//! clustering across a batch of signals. Two items whose normalized content is
//! similar above a threshold are the SAME copy (syndication) and collapse into
//! one cluster. The per-signal annotation tells a downstream context assembler
//! whether an item is `single-source` or `syndicated across N sources` — and
//! syndication is explicitly NOT corroboration.
//!
//! What it deliberately does NOT do: decide that two DIFFERENTLY-worded items
//! are about the same event ("N independent origins corroborating event X").
//! That is semantic and belongs to the model / world-forward discovery, which
//! composes these syndication clusters with its event grouping. The annotation
//! here is computed deterministically and never self-reported by the model
//! (spec 5.11 data-not-instructions).
//!
//! Algorithm: token-set Jaccard similarity + union-find connected components.
//! O(n²) within a batch, which is bounded by the per-tick volume envelope.
//! Simhash/MinHash is a future refinement for larger batches (open question in
//! the design doc); the interface is stable across that change.

use std::collections::BTreeSet;

/// One signal entering corroboration. `text` is the content used for
/// similarity (e.g. title + summary), already extracted by the caller.
#[derive(Debug, Clone)]
pub struct CorroborationInput {
    pub signal_id: String,
    pub source: String,
    pub text: String,
    pub tier: u8,
}

/// The deterministic verdict for one signal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corroboration {
    pub signal_id: String,
    /// Stable cluster id (assigned by first appearance; same input → same ids).
    pub cluster_id: usize,
    /// Distinct sources whose content is near-identical to this one.
    pub distinct_source_count: usize,
    /// True when the SAME content appears from MORE THAN ONE source — fake
    /// consensus, to be treated as one origin, never as corroboration.
    pub syndicated: bool,
    /// Human-readable annotation for the context assembler.
    pub annotation: String,
}

/// Cluster a batch of signals by near-duplicate content and annotate each.
/// `threshold` is the minimum token-set Jaccard similarity (0.0..=1.0) for two
/// items to count as the same copy; 0.8 is a sensible default for "same wire
/// story." Output order matches input order.
pub fn corroborate(inputs: &[CorroborationInput], threshold: f64) -> Vec<Corroboration> {
    let n = inputs.len();
    let tokens: Vec<BTreeSet<String>> = inputs.iter().map(|i| tokenize(&i.text)).collect();

    // Union-find over indices; union any pair at or above the threshold.
    let mut uf = UnionFind::new(n);
    for i in 0..n {
        for j in (i + 1)..n {
            if jaccard(&tokens[i], &tokens[j]) >= threshold {
                uf.union(i, j);
            }
        }
    }

    // Assign stable cluster ids by first appearance of each root.
    let mut root_to_cluster: Vec<Option<usize>> = vec![None; n];
    let mut next_cluster = 0usize;
    let mut cluster_of = vec![0usize; n];
    for (idx, slot) in cluster_of.iter_mut().enumerate() {
        let root = uf.find(idx);
        let cid = match root_to_cluster[root] {
            Some(c) => c,
            None => {
                let c = next_cluster;
                root_to_cluster[root] = Some(c);
                next_cluster += 1;
                c
            }
        };
        *slot = cid;
    }

    // Per-cluster distinct sources.
    let mut cluster_sources: Vec<BTreeSet<String>> = vec![BTreeSet::new(); next_cluster];
    for (input, &cid) in inputs.iter().zip(cluster_of.iter()) {
        cluster_sources[cid].insert(input.source.clone());
    }

    inputs
        .iter()
        .enumerate()
        .map(|(idx, input)| {
            let cid = cluster_of[idx];
            let distinct = &cluster_sources[cid];
            let distinct_source_count = distinct.len();
            let syndicated = distinct_source_count > 1;
            let annotation = if syndicated {
                let mut srcs: Vec<&str> = distinct.iter().map(String::as_str).collect();
                srcs.sort_unstable();
                format!(
                    "syndicated: same content from {} sources [{}] — treat as ONE origin, not {}",
                    distinct_source_count,
                    srcs.join(", "),
                    distinct_source_count
                )
            } else {
                format!("single-source (tier {})", input.tier)
            };
            Corroboration {
                signal_id: input.signal_id.clone(),
                cluster_id: cid,
                distinct_source_count,
                syndicated,
                annotation,
            }
        })
        .collect()
}

/// Normalize text to a token set: lowercase, split on non-alphanumeric, drop
/// empties. Deterministic and dependency-light.
fn tokenize(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

/// Jaccard similarity of two token sets. Two empty sets share no CONTENT to
/// corroborate, so their similarity is 0 (they never cluster on emptiness).
fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// Minimal union-find with path compression + union by size (deterministic).
struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> UnionFind {
        UnionFind {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        // Attach smaller under larger; ties broken by lower index for
        // deterministic structure.
        let (big, small) = if self.size[ra] >= self.size[rb] {
            (ra, rb)
        } else {
            (rb, ra)
        };
        self.parent[small] = big;
        self.size[big] += self.size[small];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str, source: &str, text: &str, tier: u8) -> CorroborationInput {
        CorroborationInput {
            signal_id: id.into(),
            source: source.into(),
            text: text.into(),
            tier,
        }
    }

    const WIRE: &str = "Federal Reserve holds interest rates steady amid inflation concerns";

    #[test]
    fn same_wire_text_from_many_outlets_is_one_origin() {
        // The fake-consensus case: 3 outlets, identical wire copy.
        let inputs = vec![
            input("s1", "outlet_a", WIRE, 5),
            input("s2", "outlet_b", WIRE, 5),
            input("s3", "outlet_c", WIRE, 4),
        ];
        let out = corroborate(&inputs, 0.8);
        // All in one cluster.
        assert_eq!(out[0].cluster_id, out[1].cluster_id);
        assert_eq!(out[1].cluster_id, out[2].cluster_id);
        // Flagged syndicated, 3 distinct sources, NOT corroboration.
        for c in &out {
            assert!(c.syndicated);
            assert_eq!(c.distinct_source_count, 3);
            assert!(c.annotation.contains("ONE origin, not 3"));
        }
    }

    #[test]
    fn genuinely_different_stories_are_separate_single_source() {
        let inputs = vec![
            input(
                "s1",
                "outlet_a",
                "Hurricane makes landfall in Florida overnight",
                6,
            ),
            input(
                "s2",
                "outlet_b",
                "Stock market rallies on strong jobs report",
                6,
            ),
        ];
        let out = corroborate(&inputs, 0.8);
        assert_ne!(out[0].cluster_id, out[1].cluster_id);
        assert!(!out[0].syndicated && !out[1].syndicated);
        assert!(out[0].annotation.contains("single-source (tier 6)"));
    }

    #[test]
    fn same_source_repeating_is_not_syndication() {
        // One source posting the same text twice: a repeat, distinct_source=1.
        let inputs = vec![
            input("s1", "outlet_a", WIRE, 5),
            input("s2", "outlet_a", WIRE, 5),
        ];
        let out = corroborate(&inputs, 0.8);
        assert_eq!(out[0].cluster_id, out[1].cluster_id);
        assert!(
            !out[0].syndicated,
            "same source twice is a repeat, not syndication"
        );
        assert_eq!(out[0].distinct_source_count, 1);
    }

    #[test]
    fn threshold_separates_near_from_far() {
        let a = "the quick brown fox jumps over the lazy dog today";
        // Heavy overlap (one word changed) -> near-dup at 0.8.
        let near = "the quick brown fox jumps over the lazy dog now";
        // Low overlap -> not clustered.
        let far = "completely unrelated sentence about weather forecasts tomorrow";
        let inputs = vec![
            input("s1", "a", a, 5),
            input("s2", "b", near, 5),
            input("s3", "c", far, 5),
        ];
        let out = corroborate(&inputs, 0.8);
        assert_eq!(out[0].cluster_id, out[1].cluster_id, "near-dup clusters");
        assert_ne!(
            out[0].cluster_id, out[2].cluster_id,
            "far is its own cluster"
        );
    }

    #[test]
    fn empty_text_items_do_not_cluster_on_emptiness() {
        let inputs = vec![input("s1", "a", "", 5), input("s2", "b", "", 5)];
        let out = corroborate(&inputs, 0.8);
        assert_ne!(out[0].cluster_id, out[1].cluster_id);
        assert!(!out[0].syndicated);
    }

    #[test]
    fn deterministic_stable_cluster_ids() {
        let inputs = vec![
            input("s1", "a", WIRE, 5),
            input("s2", "b", "something else entirely different here", 5),
            input("s3", "c", WIRE, 5),
        ];
        let a = corroborate(&inputs, 0.8);
        let b = corroborate(&inputs, 0.8);
        assert_eq!(a, b, "same input must produce identical output");
        // Cluster ids assigned by first appearance: s1 -> 0, s2 -> 1, s3 joins 0.
        assert_eq!(a[0].cluster_id, 0);
        assert_eq!(a[1].cluster_id, 1);
        assert_eq!(a[2].cluster_id, 0);
    }

    #[test]
    fn empty_batch_is_empty() {
        assert!(corroborate(&[], 0.8).is_empty());
    }
}
