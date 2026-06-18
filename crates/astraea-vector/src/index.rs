//! Thread-safe wrapper around [`HnswIndex`] implementing the [`VectorIndex`] trait.

use std::path::Path;

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
            inner: RwLock::new(HnswIndex::new(
                dimension,
                metric,
                DEFAULT_M,
                DEFAULT_EF_CONSTRUCTION,
            )),
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

    /// Create a new HNSW vector index with a fixed RNG seed for reproducible
    /// level sampling — useful for tests and benchmarks where the exact
    /// graph layout needs to be deterministic. Uses the same default
    /// parameters as [`Self::new`] otherwise. astraeadb-issues.md #18.
    pub fn with_seed(dimension: usize, metric: DistanceMetric, seed: u64) -> Self {
        Self {
            inner: RwLock::new(HnswIndex::with_seed(
                dimension,
                metric,
                DEFAULT_M,
                DEFAULT_EF_CONSTRUCTION,
                seed,
            )),
            ef_search: DEFAULT_EF_SEARCH,
        }
    }

    /// Persist the index to the given file path.
    ///
    /// Acquires a read lock on the inner index and writes the full
    /// HNSW state to a versioned binary file.
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let idx = self.inner.read();
        idx.save(path)
    }

    /// Load an index from the given file path.
    ///
    /// Reads and validates the binary file, then wraps the deserialized
    /// `HnswIndex` in a new `HnswVectorIndex` with the default `ef_search`.
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let idx = HnswIndex::load(path)?;
        Ok(Self {
            inner: RwLock::new(idx),
            ef_search: DEFAULT_EF_SEARCH,
        })
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

    fn node_ids(&self) -> Vec<NodeId> {
        let idx = self.inner.read();
        idx.node_ids()
    }

    fn save_to_path(&self, path: &Path) -> Result<()> {
        self.save_to_file(path)
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

    // --- Task 2 & 3 (issue-26 §11): node_ids and save_to_path through the trait ---

    /// node_ids on HnswVectorIndex reflects inserts/removes through the VectorIndex trait.
    #[test]
    fn test_node_ids_via_trait() {
        let idx: Box<dyn VectorIndex> =
            Box::new(HnswVectorIndex::new(2, DistanceMetric::Euclidean));

        assert!(idx.node_ids().is_empty());

        idx.insert(NodeId(10), &[1.0, 0.0]).unwrap();
        idx.insert(NodeId(20), &[0.0, 1.0]).unwrap();

        let mut ids = idx.node_ids();
        ids.sort();
        assert_eq!(ids, vec![NodeId(10), NodeId(20)]);

        idx.remove(NodeId(10)).unwrap();
        let ids = idx.node_ids();
        assert_eq!(ids, vec![NodeId(20)]);
    }

    /// save_to_path on HnswVectorIndex persists a file that load_from_file can read back.
    #[test]
    fn test_save_to_path_via_trait() {
        use astraea_core::traits::VectorIndex as VTrait;

        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_owned();
        // Close the NamedTempFile so save_to_file can create the file (it calls File::create).
        drop(tmp);

        let original: Box<dyn VTrait> =
            Box::new(HnswVectorIndex::new(3, DistanceMetric::Euclidean));
        original.insert(NodeId(1), &[1.0, 0.0, 0.0]).unwrap();
        original.insert(NodeId(2), &[0.0, 1.0, 0.0]).unwrap();

        // Save through the trait.
        original.save_to_path(&path).unwrap();

        // Load back directly (the Graph layer's load path).
        let loaded = HnswVectorIndex::load_from_file(&path).unwrap();
        let mut ids = loaded.node_ids();
        ids.sort();
        assert_eq!(ids, vec![NodeId(1), NodeId(2)]);
        assert_eq!(loaded.dimension(), 3);
    }
}
