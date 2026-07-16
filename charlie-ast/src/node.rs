// Concern: the AST IDENTITY layer — `NodeId`, a per-node handle that keys off-tree side-channels (spans, located refusals, resolved types) yet is DELIBERATELY invisible to structural equality and hashing, so a synthesized node compares equal to a parsed one (ast-standards PART 3, "identity is not meaning") | Non-concern: what a node MEANS (`expr::Expr` owns that) and the contents of the side-channels a `NodeId` keys (later phases) | IO: none — a value type
//! Identity layer: [`NodeId`].

use std::hash::{Hash, Hasher};

/// A per-node identity handle.
///
/// A `NodeId` is *only* a key: it names a node so off-tree side-channels (spans, refusals,
/// resolved types) can attach data to it. It is **excluded from equality and hashing** — the
/// two `impl`s below are constant — so that embedding a `NodeId` in a node struct that derives
/// `PartialEq`/`Eq`/`Hash` leaves structural comparison untouched. This is Option A of
/// `ast-standards.md` PART 3: a synthesized node equals a parsed one, which is what unlocks
/// common-subexpression elimination, dedup, and `emit == parse` round-trip tests.
///
/// Because the `Hash` impl is constant, a `NodeId` must **not** itself be used as a hash-map key
/// for a side-channel — key the side-channel on the raw [`NodeId::index`] instead.
///
/// **W0 deferral — how identity attaches to a node is not decided yet.** [`crate::Expr`] carries no
/// `NodeId` field today, so the id-blind-equality payoff is exercised only in isolation (see the
/// tests below), not on the real node type. Two shapes are on the table (ast-standards PART 2/3):
/// embed the `NodeId` *in* the node (constant-`Eq` keeps structural comparison clean — proven by
/// `embedding_a_nodeid_does_not_perturb_structural_equality`), or key a parallel arena/`NodeIdx`
/// beside the tree. Pick one when the parser lands; until then this type only fixes the *key*
/// contract (blind `Eq`/`Hash`, raw `index`), not the attachment mechanism.
#[derive(Clone, Copy, Debug)]
pub struct NodeId(u32);

impl NodeId {
    /// Mint a `NodeId` from its monotonic index.
    pub const fn new(index: u32) -> Self {
        NodeId(index)
    }

    /// The raw index — the key to use for `NodeId`-keyed side-channels (not the `Hash` impl).
    pub const fn index(self) -> u32 {
        self.0
    }
}

// Constant-`Eq`: the id is invisible to equality, so `synthesized == parsed` (ast-standards PART 3).
impl PartialEq for NodeId {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for NodeId {}

// Constant-`Hash`: mirrors the constant `Eq` so an enclosing node's derived `Hash` ignores the id.
impl Hash for NodeId {
    fn hash<H: Hasher>(&self, _: &mut H) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids_compare_equal() {
        // Inequality-blindness: the id never participates in equality.
        assert_eq!(NodeId::new(1), NodeId::new(2));
        assert_eq!(NodeId::new(0), NodeId::new(u32::MAX));
    }

    #[test]
    fn raw_index_is_preserved_even_though_eq_is_blind() {
        // The value is still there for side-channel keying — only `Eq`/`Hash` ignore it.
        assert_eq!(NodeId::new(7).index(), 7);
        assert_ne!(NodeId::new(7).index(), NodeId::new(8).index());
    }

    #[test]
    fn embedding_a_nodeid_does_not_perturb_structural_equality() {
        // The payoff, demonstrated: a struct that derives `PartialEq`/`Eq` and carries a `NodeId`
        // compares by MEANING only — "synthesized == parsed" even with different ids.
        #[derive(PartialEq, Eq, Hash, Debug)]
        struct Demo {
            id: NodeId,
            meaning: u32,
        }
        assert_eq!(
            Demo {
                id: NodeId::new(1),
                meaning: 42
            },
            Demo {
                id: NodeId::new(999),
                meaning: 42
            },
        );
        assert_ne!(
            Demo {
                id: NodeId::new(1),
                meaning: 42
            },
            Demo {
                id: NodeId::new(1),
                meaning: 43
            },
        );
    }
}
