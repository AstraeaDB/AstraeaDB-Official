//! Persistence layer for HNSW indices.
//!
//! Provides save/load functionality using a versioned binary file format.
//! The format uses a fixed header for quick validation followed by bincode-
//! serialized index data for compact storage of float arrays and adjacency lists.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::DistanceMetric;

use crate::hnsw::HnswIndex;

/// Magic bytes identifying an HNSW index file: ASCII "HNSW".
const MAGIC: u32 = 0x48_4E_53_57;

/// Current file format version.
const FORMAT_VERSION: u32 = 1;

/// Fixed-size header written at the start of every HNSW index file.
///
/// This allows quick validation and metadata inspection without
/// deserializing the full index.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct HnswFileHeader {
    /// Magic bytes for file identification.
    magic: u32,
    /// Format version for forward compatibility.
    version: u32,
    /// Vector dimensionality.
    dimension: u32,
    /// Distance metric: 0=Cosine, 1=Euclidean, 2=DotProduct.
    metric: u8,
    /// Max connections per node per layer (except layer 0).
    m: u32,
    /// Max connections at layer 0.
    m_max0: u32,
    /// Beam width during construction.
    ef_construction: u32,
    /// Number of vectors stored.
    num_vectors: u64,
    /// Number of layers in the graph.
    num_layers: u32,
}

/// Encode a `DistanceMetric` as a single byte for the file header.
fn metric_to_byte(metric: DistanceMetric) -> u8 {
    match metric {
        DistanceMetric::Cosine => 0,
        DistanceMetric::Euclidean => 1,
        DistanceMetric::DotProduct => 2,
    }
}

/// Decode a single byte from the file header into a `DistanceMetric`.
fn byte_to_metric(b: u8) -> Result<DistanceMetric> {
    match b {
        0 => Ok(DistanceMetric::Cosine),
        1 => Ok(DistanceMetric::Euclidean),
        2 => Ok(DistanceMetric::DotProduct),
        _ => Err(AstraeaError::Deserialization(format!(
            "unknown distance metric byte: {b}"
        ))),
    }
}

/// Write the fixed header to the given writer.
fn write_header<W: Write>(writer: &mut W, header: &HnswFileHeader) -> Result<()> {
    writer.write_all(&header.magic.to_le_bytes())?;
    writer.write_all(&header.version.to_le_bytes())?;
    writer.write_all(&header.dimension.to_le_bytes())?;
    writer.write_all(&[header.metric])?;
    writer.write_all(&header.m.to_le_bytes())?;
    writer.write_all(&header.m_max0.to_le_bytes())?;
    writer.write_all(&header.ef_construction.to_le_bytes())?;
    writer.write_all(&header.num_vectors.to_le_bytes())?;
    writer.write_all(&header.num_layers.to_le_bytes())?;
    Ok(())
}

/// Read the fixed header from the given reader and validate magic/version.
fn read_header<R: Read>(reader: &mut R) -> Result<HnswFileHeader> {
    let mut buf4 = [0u8; 4];
    let mut buf8 = [0u8; 8];
    let mut buf1 = [0u8; 1];

    // magic
    reader.read_exact(&mut buf4)?;
    let magic = u32::from_le_bytes(buf4);
    if magic != MAGIC {
        return Err(AstraeaError::Deserialization(format!(
            "invalid HNSW file magic: expected 0x{MAGIC:08X}, got 0x{magic:08X}"
        )));
    }

    // version
    reader.read_exact(&mut buf4)?;
    let version = u32::from_le_bytes(buf4);
    if version != FORMAT_VERSION {
        return Err(AstraeaError::Deserialization(format!(
            "unsupported HNSW file version: expected {FORMAT_VERSION}, got {version}"
        )));
    }

    // dimension
    reader.read_exact(&mut buf4)?;
    let dimension = u32::from_le_bytes(buf4);

    // metric
    reader.read_exact(&mut buf1)?;
    let metric = buf1[0];

    // m
    reader.read_exact(&mut buf4)?;
    let m = u32::from_le_bytes(buf4);

    // m_max0
    reader.read_exact(&mut buf4)?;
    let m_max0 = u32::from_le_bytes(buf4);

    // ef_construction
    reader.read_exact(&mut buf4)?;
    let ef_construction = u32::from_le_bytes(buf4);

    // num_vectors
    reader.read_exact(&mut buf8)?;
    let num_vectors = u64::from_le_bytes(buf8);

    // num_layers
    reader.read_exact(&mut buf4)?;
    let num_layers = u32::from_le_bytes(buf4);

    Ok(HnswFileHeader {
        magic,
        version,
        dimension,
        metric,
        m,
        m_max0,
        ef_construction,
        num_vectors,
        num_layers,
    })
}

/// Save an `HnswIndex` to the file at `path`.
///
/// The file format is:
/// 1. Fixed header (magic, version, metadata)
/// 2. Bincode-serialized index body (vectors, layers, entry_point, etc.)
pub fn save_to_file(index: &HnswIndex, path: &Path) -> Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    let header = HnswFileHeader {
        magic: MAGIC,
        version: FORMAT_VERSION,
        dimension: index.dimension() as u32,
        metric: metric_to_byte(index.metric()),
        m: index.m() as u32,
        m_max0: index.m_max0() as u32,
        ef_construction: index.ef_construction() as u32,
        num_vectors: index.len() as u64,
        num_layers: index.num_layers() as u32,
    };

    write_header(&mut writer, &header)?;

    // Serialize the full index via bincode.
    bincode::serialize_into(&mut writer, index).map_err(|e| {
        AstraeaError::Serialization(format!("bincode serialization failed: {e}"))
    })?;

    writer.flush()?;
    Ok(())
}

/// Load an `HnswIndex` from the file at `path`.
///
/// Validates the file header (magic bytes and format version) before
/// deserializing the index body.
pub fn load_from_file(path: &Path) -> Result<HnswIndex> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    // Read and validate the header.
    let header = read_header(&mut reader)?;

    // Validate the metric byte is known.
    let _metric = byte_to_metric(header.metric)?;

    // Deserialize the index body.
    let index: HnswIndex = bincode::deserialize_from(&mut reader).map_err(|e| {
        AstraeaError::Deserialization(format!("bincode deserialization failed: {e}"))
    })?;

    // Cross-check header against deserialized data.
    if index.dimension() != header.dimension as usize {
        return Err(AstraeaError::Deserialization(format!(
            "header/body dimension mismatch: header says {}, body has {}",
            header.dimension,
            index.dimension()
        )));
    }

    Ok(index)
}

// --- Convenience methods on HnswIndex ---

impl HnswIndex {
    /// Persist this index to the given file path.
    pub fn save(&self, path: &Path) -> Result<()> {
        save_to_file(self, path)
    }

    /// Load an index from the given file path.
    pub fn load(path: &Path) -> Result<Self> {
        load_from_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use astraea_core::types::NodeId;
    use rand::Rng;
    use tempfile::NamedTempFile;

    /// Helper: create a small index, insert some vectors, return it.
    fn build_test_index(dim: usize, n: usize) -> HnswIndex {
        let mut idx = HnswIndex::new(dim, DistanceMetric::Euclidean, 16, 200);
        let mut rng = rand::thread_rng();
        for i in 0..n {
            let v: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();
            idx.insert(NodeId(i as u64), &v).unwrap();
        }
        idx
    }

    #[test]
    fn test_round_trip_100_vectors() {
        let dim = 32;
        let n = 100;
        let original = build_test_index(dim, n);

        // Save to a temp file.
        let tmp = NamedTempFile::new().unwrap();
        original.save(tmp.path()).unwrap();

        // Load it back.
        let loaded = HnswIndex::load(tmp.path()).unwrap();

        // Verify metadata matches.
        assert_eq!(loaded.dimension(), original.dimension());
        assert_eq!(loaded.metric(), original.metric());
        assert_eq!(loaded.m(), original.m());
        assert_eq!(loaded.m_max0(), original.m_max0());
        assert_eq!(loaded.ef_construction(), original.ef_construction());
        assert_eq!(loaded.len(), original.len());

        // Verify search results match.
        let mut rng = rand::thread_rng();
        let query: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();
        let k = 5;
        let ef_search = 100;

        let orig_results = original.search(&query, k, ef_search).unwrap();
        let loaded_results = loaded.search(&query, k, ef_search).unwrap();

        assert_eq!(orig_results.len(), loaded_results.len());
        // The top result should be the same node with the same distance.
        assert_eq!(orig_results[0].0, loaded_results[0].0);
        assert!((orig_results[0].1 - loaded_results[0].1).abs() < 1e-6);
    }

    #[test]
    fn test_round_trip_empty_index() {
        let dim = 8;
        let original = HnswIndex::new(dim, DistanceMetric::Cosine, 16, 200);
        assert!(original.is_empty());

        let tmp = NamedTempFile::new().unwrap();
        original.save(tmp.path()).unwrap();

        let loaded = HnswIndex::load(tmp.path()).unwrap();

        assert_eq!(loaded.dimension(), dim);
        assert_eq!(loaded.metric(), DistanceMetric::Cosine);
        assert!(loaded.is_empty());
        assert_eq!(loaded.len(), 0);

        // Search on empty loaded index should return empty results.
        let results = loaded.search(&vec![0.0; dim], 5, 50).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_invalid_magic_bytes() {
        let dim = 4;
        let original = build_test_index(dim, 5);

        let tmp = NamedTempFile::new().unwrap();
        original.save(tmp.path()).unwrap();

        // Corrupt the first 4 bytes (magic).
        let mut data = std::fs::read(tmp.path()).unwrap();
        data[0] = 0xFF;
        data[1] = 0xFF;
        data[2] = 0xFF;
        data[3] = 0xFF;
        std::fs::write(tmp.path(), &data).unwrap();

        let result = HnswIndex::load(tmp.path());
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("invalid HNSW file magic"),
            "expected magic error, got: {err_msg}"
        );
    }

    #[test]
    fn test_invalid_version() {
        let dim = 4;
        let original = build_test_index(dim, 5);

        let tmp = NamedTempFile::new().unwrap();
        original.save(tmp.path()).unwrap();

        // Corrupt the version field (bytes 4..8) to version 99.
        let mut data = std::fs::read(tmp.path()).unwrap();
        let bad_version: u32 = 99;
        data[4..8].copy_from_slice(&bad_version.to_le_bytes());
        std::fs::write(tmp.path(), &data).unwrap();

        let result = HnswIndex::load(tmp.path());
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("unsupported HNSW file version"),
            "expected version error, got: {err_msg}"
        );
    }

    #[test]
    fn test_round_trip_cosine_metric() {
        let dim = 16;
        let n = 50;
        let mut idx = HnswIndex::new(dim, DistanceMetric::Cosine, 8, 100);
        let mut rng = rand::thread_rng();
        for i in 0..n {
            let v: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>() + 0.01).collect();
            idx.insert(NodeId(i as u64), &v).unwrap();
        }

        let tmp = NamedTempFile::new().unwrap();
        idx.save(tmp.path()).unwrap();

        let loaded = HnswIndex::load(tmp.path()).unwrap();
        assert_eq!(loaded.metric(), DistanceMetric::Cosine);
        assert_eq!(loaded.len(), n);
        assert_eq!(loaded.m(), 8);
        assert_eq!(loaded.ef_construction(), 100);
    }

    #[test]
    fn test_round_trip_dot_product_metric() {
        let dim = 8;
        let n = 20;
        let mut idx = HnswIndex::new(dim, DistanceMetric::DotProduct, 12, 150);
        let mut rng = rand::thread_rng();
        for i in 0..n {
            let v: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();
            idx.insert(NodeId(i as u64), &v).unwrap();
        }

        let tmp = NamedTempFile::new().unwrap();
        idx.save(tmp.path()).unwrap();

        let loaded = HnswIndex::load(tmp.path()).unwrap();
        assert_eq!(loaded.metric(), DistanceMetric::DotProduct);
        assert_eq!(loaded.len(), n);
    }

    #[test]
    fn test_search_consistency_after_load() {
        // Verify that multiple queries all produce identical results
        // on the original and loaded indices.
        let dim = 16;
        let n = 80;
        let original = build_test_index(dim, n);

        let tmp = NamedTempFile::new().unwrap();
        original.save(tmp.path()).unwrap();
        let loaded = HnswIndex::load(tmp.path()).unwrap();

        let mut rng = rand::thread_rng();
        for _ in 0..10 {
            let query: Vec<f32> = (0..dim).map(|_| rng.r#gen::<f32>()).collect();
            let orig_results = original.search(&query, 3, 100).unwrap();
            let loaded_results = loaded.search(&query, 3, 100).unwrap();

            assert_eq!(orig_results.len(), loaded_results.len());
            for (o, l) in orig_results.iter().zip(loaded_results.iter()) {
                assert_eq!(o.0, l.0, "node IDs should match");
                assert!(
                    (o.1 - l.1).abs() < 1e-6,
                    "distances should match"
                );
            }
        }
    }
}
