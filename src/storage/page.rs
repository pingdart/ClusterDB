use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

pub const PAGE_SIZE: usize = 4096;
pub const PAGE_HEADER_SIZE: usize = 32;

pub type PageId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub offset: u16,
    pub length: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageHeader {
    pub page_id: PageId,
    pub lsn: u64,
    pub slot_count: u16,
    pub free_space_offset: u16,
}

#[derive(Debug, Clone)]
pub struct SlottedPage {
    pub header: PageHeader,
    pub slots: Vec<Slot>,
    pub data: [u8; PAGE_SIZE],
}

impl SlottedPage {
    pub fn new(page_id: PageId) -> Self {
        Self {
            header: PageHeader {
                page_id,
                lsn: 0,
                slot_count: 0,
                free_space_offset: PAGE_SIZE as u16,
            },
            slots: Vec::new(),
            data: [0u8; PAGE_SIZE],
        }
    }

    /// Calculates remaining free contiguous space available for slot + payload
    pub fn free_space(&self) -> usize {
        let slots_end = PAGE_HEADER_SIZE + (self.slots.len() * 4);
        if (self.header.free_space_offset as usize) < slots_end {
            0
        } else {
            (self.header.free_space_offset as usize) - slots_end
        }
    }

    /// Inserts a tuple into the slotted page.
    /// Grows slots array downwards and tuple data upwards from end of page.
    pub fn insert_tuple(&mut self, tuple_data: &[u8]) -> Option<u16> {
        let needed_space = tuple_data.len() + 4; // 4 bytes for new Slot entry
        if self.free_space() < needed_space {
            return None;
        }

        let new_offset = self.header.free_space_offset - (tuple_data.len() as u16);
        self.data[(new_offset as usize)..(new_offset as usize + tuple_data.len())]
            .copy_from_slice(tuple_data);

        let slot_id = self.slots.len() as u16;
        self.slots.push(Slot {
            offset: new_offset,
            length: tuple_data.len() as u16,
        });

        self.header.free_space_offset = new_offset;
        self.header.slot_count = self.slots.len() as u16;

        Some(slot_id)
    }

    /// Reads a tuple by slot ID
    pub fn get_tuple(&self, slot_id: u16) -> Option<&[u8]> {
        let slot = self.slots.get(slot_id as usize)?;
        if slot.length == 0 {
            return None; // Tombstone / deleted
        }
        let start = slot.offset as usize;
        let end = start + slot.length as usize;
        Some(&self.data[start..end])
    }

    /// Marks a tuple as deleted (tombstone)
    pub fn delete_tuple(&mut self, slot_id: u16) -> bool {
        if let Some(slot) = self.slots.get_mut(slot_id as usize) {
            if slot.length > 0 {
                slot.length = 0;
                return true;
            }
        }
        false
    }

    /// Serializes the slotted page to a fixed 4096-byte array for disk I/O
    pub fn to_bytes(&self) -> [u8; PAGE_SIZE] {
        let mut raw = self.data;

        // Serialize header into first 32 bytes
        raw[0..4].copy_from_slice(&self.header.page_id.to_le_bytes());
        raw[4..12].copy_from_slice(&self.header.lsn.to_le_bytes());
        raw[12..14].copy_from_slice(&self.header.slot_count.to_le_bytes());
        raw[14..16].copy_from_slice(&self.header.free_space_offset.to_le_bytes());

        // Serialize slots starting at byte 32
        let mut pos = PAGE_HEADER_SIZE;
        for s in &self.slots {
            raw[pos..pos + 2].copy_from_slice(&s.offset.to_le_bytes());
            raw[pos + 2..pos + 4].copy_from_slice(&s.length.to_le_bytes());
            pos += 4;
        }

        raw
    }

    /// Deserializes a fixed 4096-byte disk frame into a SlottedPage
    pub fn from_bytes(raw: &[u8; PAGE_SIZE]) -> Result<Self> {
        let page_id = u32::from_le_bytes(raw[0..4].try_into()?);
        let lsn = u64::from_le_bytes(raw[4..12].try_into()?);
        let slot_count = u16::from_le_bytes(raw[12..14].try_into()?);
        let free_space_offset = u16::from_le_bytes(raw[14..16].try_into()?);

        let mut slots = Vec::with_capacity(slot_count as usize);
        let mut pos = PAGE_HEADER_SIZE;

        for _ in 0..slot_count {
            if pos + 4 > PAGE_SIZE {
                bail!("Corrupt page slots table: overflow");
            }
            let offset = u16::from_le_bytes(raw[pos..pos + 2].try_into()?);
            let length = u16::from_le_bytes(raw[pos + 2..pos + 4].try_into()?);
            slots.push(Slot { offset, length });
            pos += 4;
        }

        Ok(Self {
            header: PageHeader {
                page_id,
                lsn,
                slot_count,
                free_space_offset,
            },
            slots,
            data: *raw,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slotted_page_insert_read_and_delete() {
        let mut page = SlottedPage::new(1);

        let t1 = b"Patient: Alice, Age: 30";
        let t2 = b"Patient: Bob, Age: 45";

        let slot1 = page.insert_tuple(t1).unwrap();
        let slot2 = page.insert_tuple(t2).unwrap();

        assert_eq!(page.get_tuple(slot1), Some(&t1[..]));
        assert_eq!(page.get_tuple(slot2), Some(&t2[..]));

        // Roundtrip to disk bytes
        let raw_bytes = page.to_bytes();
        let restored = SlottedPage::from_bytes(&raw_bytes).unwrap();
        assert_eq!(restored.get_tuple(slot1), Some(&t1[..]));
        assert_eq!(restored.get_tuple(slot2), Some(&t2[..]));

        // Delete t1
        page.delete_tuple(slot1);
        assert_eq!(page.get_tuple(slot1), None);
        assert_eq!(page.get_tuple(slot2), Some(&t2[..]));
    }
}
