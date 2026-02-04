//! Thread-safe wrapper around [`HnswIndex`] implementing the [`VectorIndex`] trait.

use parking_lot::RwLock;

use astraea_core::error::Result;
use astraea_core::traits::VectorIndex;
use astraea_core::types::{DistanceMetric, NodeId, SimilarityResult};

use crate::hnsw::HnswIndex;

/// Default number of connections per node per layer.
const DEFAULT_M: usize = 16;
/// Default beam width during construction.
const DEFAULT_EF_CONSTRUCTION: usize = 200;
/// Default beam width during search.
const DEFAULT_EF_SEARCH: usize = 50;

/// A thread-safe HNSW-based vector index.
///
/// Wraps [`HnswIndex`] with a `parking_lot::RwLock` so that multiple readers
/// can search concurrently, while writes (insert/remove) acquire exclusive access.
pub struct HnswVectorIndex {
    inner: RwLock<HnswIndex>,
    ef_search: usize,
}

impl HnswVectorIndex {
    /// Create a new HNSW vector index with default parameters.
    ///
    /// - `m = 16`
    /// - `ef_construction = 200`
    /// - `ef_search = 50`
    pub fn new(dimension: usize, metric: DistanceMetric) -> Self {
        Self {
            inner: RwLock::new(HnswIndex::new(dimension, metric, DEFAULT_M, DEFAULT_EF_CONSTRUCTION)),
            ef_search: DEFAULT_EF_SEARCH,
        }
    }

    /// Create a new HNSW vector index with custom parameters.
    pub fn with_params(
        dimension: usize,
        metric: DistanceMetric,
        m: usize,
        ef_construction: usize,
        ef_search: usize,
    ) -> Self {
        Self {
            inner: RwLock::new(HnswIndex::new(dimension, metric, m, ef_construction)),
            ef_search,
        }
    }
}

impl VectorIndex for HnswVectorIndex {
    fn insert(&self, node_id: NodeId, embedding: &[f32]) -> Result<()> {
        let mut idx = self.inner.write();
        idx.insert(node_id, embedding)
    }

    fn remove(&self, node_id: NodeId) -> Result<bool> {
        let mut idx = self.inner.write();
        idx.remove(node_id)
    }

    fn search(&self, query: &[f32], k: usize) -> Result<Vec<SimilarityResult>> {
        let idx = self.inner.read();
        let raw_results = idx.search(query, k, self.ef_search)?;
        Ok(raw_results
            .into_iter()
            .map(|(node_id, distance)| SimilarityResult { node_id, distance })
            .collect())
    }

    fn dimension(&self) -> usize {
        let idx = self.inner.read();
        idx.dimension()
    }

    fn metric(&self) -> DistanceMetric {
        let idx = self.inner.read();
        idx.metric()
    }

    fn len(&self) -> usize {
        let idx = self.inner.read();
        idx.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_index_trait_basic() {
        let idx = HnswVectorIndex::new(3, DistanceMetric::Euclidean);
        assert_eq!(idx.dimension(), 3);
        assert_eq!(idx.metric(), DistanceMetric::Euclidean);
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);

        idx.insert(NodeId(1), &[1.0, 0.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[0.0, 1.0, 0.0]).unwrap();
        idx.insert(NodeId(3), &[0.0, 0.0, 1.0]).unwrap();

        assert_eq!(idx.len(), 3);
        assert!(!idx.is_empty());

        let results = idx.search(&[1.0, 0.0, 0.0], 2).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].node_id, NodeId(1));
        assert!(results[0].distance < 1e-6);
    }

    #[test]
    fn test_vector_index_trait_remove() {
        let idx = HnswVectorIndex::new(2, DistanceMetric::Cosine);
        idx.insert(NodeId(1), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(2), &[0.0, 1.0]).unwrap();

        assert!(idx.remove(NodeId(1)).unwrap());
        assert_eq!(idx.len(), 1);
        assert!(!idx.remove(NodeId(99)).unwrap());
    }

    #[test]
    fn test_vector_index_custom_params() {
        let idx = HnswVectorIndex::with_params(4, DistanceMetric::DotProduct, 8, 100, 30);
        assert_eq!(idx.dimension(), 4);
        assert_eq!(idx.metric(), DistanceMetric::DotProduct);
    }
}
