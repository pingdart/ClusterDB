use crate::storage::page::{PageId, SlottedPage, PAGE_SIZE};
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug)]
pub struct DiskManager {
    file: Mutex<File>,
    next_page_id: AtomicU32,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl DiskManager {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&db_path)
            .with_context(|| format!("Failed to open DB storage file at {:?}", db_path))?;

        let file_len = file.metadata()?.len();
        let existing_pages = (file_len / PAGE_SIZE as u64) as u32;

        Ok(Self {
            file: Mutex::new(file),
            next_page_id: AtomicU32::new(existing_pages),
            db_path,
        })
    }

    pub fn allocate_page(&self) -> PageId {
        self.next_page_id.fetch_add(1, Ordering::SeqCst)
    }

    pub fn read_page(&self, page_id: PageId) -> Result<[u8; PAGE_SIZE]> {
        let mut file = self.file.lock().unwrap();
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        file.seek(SeekFrom::Start(offset))?;

        let mut buf = [0u8; PAGE_SIZE];
        let bytes_read = file.read(&mut buf)?;
        if bytes_read == 0 {
            // New blank page
            return Ok([0u8; PAGE_SIZE]);
        }
        Ok(buf)
    }

    pub fn write_page(&self, page_id: PageId, data: &[u8; PAGE_SIZE]) -> Result<()> {
        let mut file = self.file.lock().unwrap();
        let offset = (page_id as u64) * (PAGE_SIZE as u64);
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        file.sync_data()?;
        Ok(())
    }
}

pub struct BufferFrame {
    pub page_id: Option<PageId>,
    pub page: SlottedPage,
    pub pin_count: u32,
    pub is_dirty: bool,
    pub ref_bit: bool,
}

impl BufferFrame {
    pub fn new() -> Self {
        Self {
            page_id: None,
            page: SlottedPage::new(0),
            pin_count: 0,
            is_dirty: false,
            ref_bit: false,
        }
    }
}

pub struct BufferPoolManager {
    disk_manager: Arc<DiskManager>,
    frames: Vec<Arc<RwLock<BufferFrame>>>,
    page_table: RwLock<HashMap<PageId, usize>>, // PageId -> FrameId
    clock_hand: AtomicUsize,
    pool_size: usize,
}

impl BufferPoolManager {
    pub fn new(disk_manager: Arc<DiskManager>, pool_size: usize) -> Arc<Self> {
        let mut frames = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            frames.push(Arc::new(RwLock::new(BufferFrame::new())));
        }

        Arc::new(Self {
            disk_manager,
            frames,
            page_table: RwLock::new(HashMap::new()),
            clock_hand: AtomicUsize::new(0),
            pool_size,
        })
    }

    /// Fetches a page from buffer pool, reading from disk if not currently cached
    pub fn fetch_page(&self, page_id: PageId) -> Result<Arc<RwLock<BufferFrame>>> {
        // Fast path: already in buffer pool
        {
            let table = self.page_table.read().unwrap();
            if let Some(&frame_id) = table.get(&page_id) {
                let frame_arc = self.frames[frame_id].clone();
                let mut frame = frame_arc.write().unwrap();
                frame.pin_count += 1;
                frame.ref_bit = true;
                return Ok(frame_arc.clone());
            }
        }

        // Cache miss: find victim frame using Clock Eviction
        let (victim_frame_id, frame_arc) = self.find_victim_frame()?;

        // Evict old page if needed
        {
            let mut frame = frame_arc.write().unwrap();
            if let Some(old_page_id) = frame.page_id {
                if frame.is_dirty {
                    let bytes = frame.page.to_bytes();
                    self.disk_manager.write_page(old_page_id, &bytes)?;
                    frame.is_dirty = false;
                }
                let mut table = self.page_table.write().unwrap();
                table.remove(&old_page_id);
            }

            // Read new page from disk into frame
            let raw_bytes = self.disk_manager.read_page(page_id)?;
            let loaded_page = SlottedPage::from_bytes(&raw_bytes).unwrap_or_else(|_| SlottedPage::new(page_id));

            frame.page_id = Some(page_id);
            frame.page = loaded_page;
            frame.pin_count = 1;
            frame.is_dirty = false;
            frame.ref_bit = true;

            let mut table = self.page_table.write().unwrap();
            table.insert(page_id, victim_frame_id);
        }

        Ok(frame_arc)
    }

    /// Allocates and initializes a new page in the buffer pool
    pub fn new_page(&self) -> Result<(PageId, Arc<RwLock<BufferFrame>>)> {
        let new_page_id = self.disk_manager.allocate_page();
        let (victim_frame_id, frame_arc) = self.find_victim_frame()?;

        {
            let mut frame = frame_arc.write().unwrap();
            if let Some(old_page_id) = frame.page_id {
                if frame.is_dirty {
                    let bytes = frame.page.to_bytes();
                    self.disk_manager.write_page(old_page_id, &bytes)?;
                    frame.is_dirty = false;
                }
                let mut table = self.page_table.write().unwrap();
                table.remove(&old_page_id);
            }

            frame.page_id = Some(new_page_id);
            frame.page = SlottedPage::new(new_page_id);
            frame.pin_count = 1;
            frame.is_dirty = true;
            frame.ref_bit = true;

            let mut table = self.page_table.write().unwrap();
            table.insert(new_page_id, victim_frame_id);
        }

        Ok((new_page_id, frame_arc))
    }

    /// Unpins a page after a query/transaction finishes using it
    pub fn unpin_page(&self, page_id: PageId, is_dirty: bool) -> Result<()> {
        let table = self.page_table.read().unwrap();
        if let Some(&frame_id) = table.get(&page_id) {
            let mut frame = self.frames[frame_id].write().unwrap();
            if frame.pin_count > 0 {
                frame.pin_count -= 1;
            }
            if is_dirty {
                frame.is_dirty = true;
            }
            Ok(())
        } else {
            bail!("Page {} not found in page table", page_id);
        }
    }

    /// Flushes a specific page to disk
    #[allow(dead_code)]
    pub fn flush_page(&self, page_id: PageId) -> Result<()> {
        let table = self.page_table.read().unwrap();
        if let Some(&frame_id) = table.get(&page_id) {
            let mut frame = self.frames[frame_id].write().unwrap();
            if frame.is_dirty {
                let bytes = frame.page.to_bytes();
                self.disk_manager.write_page(page_id, &bytes)?;
                frame.is_dirty = false;
            }
            Ok(())
        } else {
            Ok(()) // Page already evicted and durable
        }
    }

    /// Flushes all dirty buffer frames to disk
    pub fn flush_all(&self) -> Result<()> {
        for frame_arc in &self.frames {
            let mut frame = frame_arc.write().unwrap();
            if frame.is_dirty {
                if let Some(pid) = frame.page_id {
                    let bytes = frame.page.to_bytes();
                    self.disk_manager.write_page(pid, &bytes)?;
                    frame.is_dirty = false;
                }
            }
        }
        Ok(())
    }

    /// Clock replacement algorithm (Second Chance) to choose an unpinned frame for eviction
    fn find_victim_frame(&self) -> Result<(usize, Arc<RwLock<BufferFrame>>)> {
        for _ in 0..(self.pool_size * 2) {
            let frame_id = self.clock_hand.fetch_add(1, Ordering::SeqCst) % self.pool_size;
            let frame_arc = self.frames[frame_id].clone();
            let mut frame = frame_arc.write().unwrap();

            if frame.pin_count == 0 {
                if frame.ref_bit {
                    frame.ref_bit = false;
                } else {
                    return Ok((frame_id, frame_arc.clone()));
                }
            }
        }

        bail!("Buffer pool full: all frames are currently pinned!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_pool_page_pin_flush_and_eviction() {
        let temp_dir = std::env::temp_dir().join(format!("pingdart_bp_test_{}", rand::random::<u64>()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let db_file = temp_dir.join("test.db");

        let disk_mgr = Arc::new(DiskManager::new(&db_file).unwrap());
        // Small buffer pool of 2 frames to test eviction
        let bpm = BufferPoolManager::new(disk_mgr.clone(), 2);

        // Allocate Page 0
        let (p0_id, p0_frame) = bpm.new_page().unwrap();
        {
            let mut f0 = p0_frame.write().unwrap();
            f0.page.insert_tuple(b"Record in Page 0");
        }
        bpm.unpin_page(p0_id, true).unwrap();

        // Allocate Page 1
        let (p1_id, p1_frame) = bpm.new_page().unwrap();
        {
            let mut f1 = p1_frame.write().unwrap();
            f1.page.insert_tuple(b"Record in Page 1");
        }
        bpm.unpin_page(p1_id, true).unwrap();

        // Allocate Page 2 -> forces Clock Eviction of an unpinned frame
        let (p2_id, p2_frame) = bpm.new_page().unwrap();
        {
            let mut f2 = p2_frame.write().unwrap();
            f2.page.insert_tuple(b"Record in Page 2");
        }
        bpm.unpin_page(p2_id, true).unwrap();

        // Flush all frames to disk
        bpm.flush_all().unwrap();

        // Fetch evicted Page 0 back into buffer pool from disk
        let p0_reloaded = bpm.fetch_page(p0_id).unwrap();
        {
            let f0 = p0_reloaded.read().unwrap();
            assert_eq!(f0.page.get_tuple(0), Some(&b"Record in Page 0"[..]));
        }
        bpm.unpin_page(p0_id, false).unwrap();

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
