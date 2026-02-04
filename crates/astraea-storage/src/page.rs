//! Page format and layout for the AstraeaDB storage engine.
//!
//! Each page is a fixed-size 8 KiB block. Pages are the fundamental unit of
//! storage and I/O. Node and edge records are serialized into pages using a
//! slotted-page design with fixed-size record headers.

use astraea_core::error::{AstraeaError, Result};
use astraea_core::types::PageId;
use serde::{Deserialize, Serialize};

/// Page size in bytes (8 KiB).
pub const PAGE_SIZE: usize = 8192;

/// Size of the serialized page header in bytes.
/// Layout: page_id(8) + page_type(1) + record_count(2) + free_space_offset(2) + checksum(4) = 17
pub const PAGE_HEADER_SIZE: usize = 17;

/// Size of a node record header within a page.
/// Layout: node_id(8) + data_len(4) + adjacency_offset(4) = 16
pub const NODE_RECORD_HEADER_SIZE: usize = 16;

/// Type of data stored in a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum PageType {
    NodePage = 0,
    EdgePage = 1,
    OverflowPage = 2,
    FreelistPage = 3,
    MetadataPage = 4,
}

impl PageType {
    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(PageType::NodePage),
            1 => Ok(PageType::EdgePage),
            2 => Ok(PageType::OverflowPage),
            3 => Ok(PageType::FreelistPage),
            4 => Ok(PageType::MetadataPage),
            _ => Err(AstraeaError::Deserialization(format!(
                "invalid page type: {}",
                v
            ))),
        }
    }
}

/// Header at the beginning of every page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageHeader {
    /// Unique page identifier.
    pub page_id: PageId,
    /// Type of data stored in this page.
    pub page_type: PageType,
    /// Number of records currently stored in this page.
    pub record_count: u16,
    /// Byte offset of the next free space within the page (relative to page start).
    pub free_space_offset: u16,
    /// CRC32 checksum of the page data (excluding the checksum field itself).
    pub checksum: u32,
}

impl PageHeader {
    /// Create a new page header for an empty page of the given type.
    pub fn new(page_id: PageId, page_type: PageType) -> Self {
        Self {
            page_id,
            page_type,
            record_count: 0,
            free_space_offset: PAGE_HEADER_SIZE as u16,
            checksum: 0,
        }
    }

    /// Serialize this header into the first bytes of the given buffer.
    pub fn write_to(&self, buf: &mut [u8; PAGE_SIZE]) {
        buf[0..8].copy_from_slice(&self.page_id.0.to_le_bytes());
        buf[8] = self.page_type as u8;
        buf[9..11].copy_from_slice(&self.record_count.to_le_bytes());
        buf[11..13].copy_from_slice(&self.free_space_offset.to_le_bytes());
        buf[13..17].copy_from_slice(&self.checksum.to_le_bytes());
    }

    /// Deserialize a header from the first bytes of the given buffer.
    pub fn read_from(buf: &[u8; PAGE_SIZE]) -> Result<Self> {
        let page_id = PageId(u64::from_le_bytes(
            buf[0..8].try_into().expect("slice len == 8"),
        ));
        let page_type = PageType::from_u8(buf[8])?;
        let record_count = u16::from_le_bytes(buf[9..11].try_into().expect("slice len == 2"));
        let free_space_offset = u16::from_le_bytes(buf[11..13].try_into().expect("slice len == 2"));
        let checksum = u32::from_le_bytes(buf[13..17].try_into().expect("slice len == 4"));

        Ok(Self {
            page_id,
            page_type,
            record_count,
            free_space_offset,
            checksum,
        })
    }

    /// Remaining free space in the page after the current records.
    pub fn free_space(&self) -> usize {
        PAGE_SIZE.saturating_sub(self.free_space_offset as usize)
    }
}

/// Header for a node record stored within a NodePage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeRecordHeader {
    /// The node's unique identifier.
    pub node_id: u64,
    /// Length of the serialized data (JSON properties + embedding) following this header.
    pub data_len: u32,
    /// Byte offset within the page (or overflow page reference) where adjacency
    /// (outgoing edge IDs) are stored. 0 means no adjacency data.
    pub adjacency_offset: u32,
}

impl NodeRecordHeader {
    /// Write this record header into the buffer at the given offset.
    pub fn write_to(&self, buf: &mut [u8; PAGE_SIZE], offset: usize) {
        buf[offset..offset + 8].copy_from_slice(&self.node_id.to_le_bytes());
        buf[offset + 8..offset + 12].copy_from_slice(&self.data_len.to_le_bytes());
        buf[offset + 12..offset + 16].copy_from_slice(&self.adjacency_offset.to_le_bytes());
    }

    /// Read a record header from the buffer at the given offset.
    pub fn read_from(buf: &[u8; PAGE_SIZE], offset: usize) -> Self {
        let node_id = u64::from_le_bytes(
            buf[offset..offset + 8]
                .try_into()
                .expect("slice len == 8"),
        );
        let data_len = u32::from_le_bytes(
            buf[offset + 8..offset + 12]
                .try_into()
                .expect("slice len == 4"),
        );
        let adjacency_offset = u32::from_le_bytes(
            buf[offset + 12..offset + 16]
                .try_into()
                .expect("slice len == 4"),
        );
        Self {
            node_id,
            data_len,
            adjacency_offset,
        }
    }
}

/// Compute a CRC32 checksum of the page data, excluding the checksum field itself
/// (bytes 13..17).
pub fn compute_page_checksum(buf: &[u8; PAGE_SIZE]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&buf[0..13]);
    hasher.update(&buf[17..]);
    hasher.finalize()
}

/// Initialize a fresh page buffer with the given header. Zeros out all data.
pub fn init_page(page_id: PageId, page_type: PageType) -> [u8; PAGE_SIZE] {
    let mut buf = [0u8; PAGE_SIZE];
    let header = PageHeader::new(page_id, page_type);
    header.write_to(&mut buf);
    // Write checksum
    let checksum = compute_page_checksum(&buf);
    buf[13..17].copy_from_slice(&checksum.to_le_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_header_roundtrip() {
        let header = PageHeader {
            page_id: PageId(42),
            page_type: PageType::NodePage,
            record_count: 5,
            free_space_offset: 128,
            checksum: 0xDEAD_BEEF,
        };

        let mut buf = [0u8; PAGE_SIZE];
        header.write_to(&mut buf);

        let restored = PageHeader::read_from(&buf).unwrap();
        assert_eq!(header, restored);
    }

    #[test]
    fn test_page_type_from_u8() {
        assert_eq!(PageType::from_u8(0).unwrap(), PageType::NodePage);
        assert_eq!(PageType::from_u8(1).unwrap(), PageType::EdgePage);
        assert_eq!(PageType::from_u8(2).unwrap(), PageType::OverflowPage);
        assert_eq!(PageType::from_u8(3).unwrap(), PageType::FreelistPage);
        assert_eq!(PageType::from_u8(4).unwrap(), PageType::MetadataPage);
        assert!(PageType::from_u8(5).is_err());
    }

    #[test]
    fn test_node_record_header_roundtrip() {
        let rec = NodeRecordHeader {
            node_id: 999,
            data_len: 512,
            adjacency_offset: 1024,
        };
        let mut buf = [0u8; PAGE_SIZE];
        let offset = PAGE_HEADER_SIZE;
        rec.write_to(&mut buf, offset);

        let restored = NodeRecordHeader::read_from(&buf, offset);
        assert_eq!(rec, restored);
    }

    #[test]
    fn test_init_page() {
        let buf = init_page(PageId(7), PageType::EdgePage);
        let header = PageHeader::read_from(&buf).unwrap();
        assert_eq!(header.page_id, PageId(7));
        assert_eq!(header.page_type, PageType::EdgePage);
        assert_eq!(header.record_count, 0);
        assert_eq!(header.free_space_offset, PAGE_HEADER_SIZE as u16);
        // Verify checksum
        let expected = compute_page_checksum(&buf);
        assert_eq!(header.checksum, expected);
    }

    #[test]
    fn test_free_space() {
        let header = PageHeader::new(PageId(0), PageType::NodePage);
        assert_eq!(header.free_space(), PAGE_SIZE - PAGE_HEADER_SIZE);
    }
}
