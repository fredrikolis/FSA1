// Concern: declares NodeId, the equality-blind node-identity key | Non-concern: attaching an id to a node, the side-channels it keys | IO: (u32) -> NodeId

use std::hash::{Hash, Hasher};

/// A node-identity key for off-tree side-channels. `Eq`/`Hash` are constant, so a `NodeId` never
/// perturbs an enclosing node's derived structural equality — and never works as a map key; key a
/// side-channel on [`NodeId::index`] instead.
#[derive(Clone, Copy, Debug)]
pub struct NodeId(u32);

impl NodeId {
    pub const fn new(index: u32) -> Self {
        NodeId(index)
    }

    pub const fn index(self) -> u32 {
        self.0
    }
}

impl PartialEq for NodeId {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for NodeId {}

impl Hash for NodeId {
    fn hash<H: Hasher>(&self, _: &mut H) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids_compare_equal() {
        assert_eq!(NodeId::new(1), NodeId::new(2));
        assert_eq!(NodeId::new(0), NodeId::new(u32::MAX));
    }

    #[test]
    fn raw_index_is_preserved_even_though_eq_is_blind() {
        assert_eq!(NodeId::new(7).index(), 7);
        assert_ne!(NodeId::new(7).index(), NodeId::new(8).index());
    }

    #[test]
    fn embedding_a_nodeid_does_not_perturb_structural_equality() {
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
