use std::collections::{HashMap, VecDeque};

const METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY: usize = 512;
const METAL_SOURCE_CACHE_BYTES: usize = 128 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::export) struct MetalSourceTileKey {
    pub(in crate::export) scene: usize,
    pub(in crate::export) series: usize,
    pub(in crate::export) level: u32,
    pub(in crate::export) z: u32,
    pub(in crate::export) c: u32,
    pub(in crate::export) t: u32,
    pub(in crate::export) col: i64,
    pub(in crate::export) row: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::export) struct MetalEncodedRowRunKey {
    pub(in crate::export) scene: usize,
    pub(in crate::export) series: usize,
    pub(in crate::export) level: u32,
    pub(in crate::export) z: u32,
    pub(in crate::export) c: u32,
    pub(in crate::export) t: u32,
    pub(in crate::export) row: u64,
    pub(in crate::export) start_col: u64,
    pub(in crate::export) tile_count: usize,
    pub(in crate::export) matrix_columns: u64,
    pub(in crate::export) matrix_rows: u64,
    pub(in crate::export) tile_size: u32,
}

pub(in crate::export) struct MetalSourceTileCache {
    capacity: usize,
    entries: HashMap<MetalSourceTileKey, MetalSourceTileCacheEntry>,
    order: VecDeque<(MetalSourceTileKey, u64)>,
    next_generation: u64,
    retained_bytes: usize,
    allocations: HashMap<usize, (usize, usize)>,
}

impl Default for MetalSourceTileCache {
    fn default() -> Self {
        Self {
            capacity: METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY,
            entries: HashMap::new(),
            order: VecDeque::new(),
            next_generation: 0,
            retained_bytes: 0,
            allocations: HashMap::new(),
        }
    }
}

struct MetalSourceTileCacheEntry {
    tile: wsi_rs::output::metal::MetalDeviceTile,
    generation: u64,
    allocation: usize,
}

impl MetalSourceTileCache {
    pub(in crate::export) fn get(
        &mut self,
        key: MetalSourceTileKey,
    ) -> Option<wsi_rs::output::metal::MetalDeviceTile> {
        let tile = self.entries.get(&key)?.tile.clone();
        self.touch(key);
        Some(tile)
    }

    pub(in crate::export) fn insert(
        &mut self,
        key: MetalSourceTileKey,
        tile: wsi_rs::output::metal::MetalDeviceTile,
    ) -> Result<(), crate::Error> {
        let image = crate::metal_interop::device_tile_image(&tile)?;
        let (allocation, bytes) = crate::metal_interop::resident_allocation_identity(image);
        if self.capacity == 0 || bytes > METAL_SOURCE_CACHE_BYTES {
            return Ok(());
        }
        self.remove(key);
        let retained = self.allocations.entry(allocation).or_insert((0, bytes));
        if retained.0 == 0 {
            self.retained_bytes += bytes;
        }
        retained.0 += 1;
        let generation = self.next_generation();
        self.entries.insert(
            key,
            MetalSourceTileCacheEntry {
                tile,
                generation,
                allocation,
            },
        );
        self.order.push_back((key, generation));
        while self.entries.len() > self.capacity || self.retained_bytes > METAL_SOURCE_CACHE_BYTES {
            let Some((oldest, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&oldest)
                .is_some_and(|entry| entry.generation == generation)
            {
                self.remove(oldest);
            }
        }
        self.compact_stale_order_entries_if_needed();
        Ok(())
    }

    fn remove(&mut self, key: MetalSourceTileKey) {
        if let Some(entry) = self.entries.remove(&key) {
            let retained = self
                .allocations
                .get_mut(&entry.allocation)
                .expect("cached tile retains its allocation");
            retained.0 -= 1;
            if retained.0 == 0 {
                self.retained_bytes -= retained.1;
                self.allocations.remove(&entry.allocation);
            }
        }
    }

    fn touch(&mut self, key: MetalSourceTileKey) {
        let generation = self.next_generation();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.generation = generation;
            self.order.push_back((key, generation));
            self.compact_stale_order_entries_if_needed();
        }
    }

    fn next_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        generation
    }

    fn compact_stale_order_entries_if_needed(&mut self) {
        let max_order_len = self.capacity.saturating_mul(4).max(1);
        if self.order.len() <= max_order_len {
            return;
        }
        self.order.retain(|(key, generation)| {
            self.entries
                .get(key)
                .is_some_and(|entry| entry.generation == *generation)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(col: i64) -> MetalSourceTileKey {
        MetalSourceTileKey {
            scene: 0,
            series: 0,
            level: 0,
            z: 0,
            c: 0,
            t: 0,
            col,
            row: 0,
        }
    }

    #[test]
    fn cache_bounds_retained_allocations_even_for_small_views() {
        let device = j2k_metal_support::system_default_device().unwrap();
        let mut cache = MetalSourceTileCache::default();
        let bytes = vec![7u8; 16 * 1024 * 1024];
        for col in 0..9 {
            let buffer =
                j2k_metal_support::checked_shared_buffer_with_slice(&device, &bytes).unwrap();
            let tile = crate::metal_interop::test_tile_from_completed_buffer(
                buffer,
                0,
                1,
                1,
                1,
                wsi_rs::PixelFormat::Gray8,
            );
            cache.insert(key(col), tile).unwrap();
        }
        assert!(
            cache.get(key(0)).is_none(),
            "old allocations must be evicted at 128 MiB"
        );
        assert!(cache.get(key(8)).is_some());
    }

    #[test]
    fn shared_views_are_charged_once_and_replacement_releases_the_allocation() {
        let device = j2k_metal_support::system_default_device().unwrap();
        let tile = crate::metal_interop::test_tile_from_shared_bytes(
            &device,
            &[1u8; 16],
            4,
            4,
            j2k_core::PixelFormat::Gray8,
        );
        let mut cache = MetalSourceTileCache {
            capacity: 2,
            ..Default::default()
        };
        cache.insert(key(0), tile.clone()).unwrap();
        cache.insert(key(1), tile.clone()).unwrap();
        assert_eq!(cache.retained_bytes, 16);
        cache.insert(key(0), tile).unwrap();
        let another = crate::metal_interop::test_tile_from_shared_bytes(
            &device,
            &[2u8; 8],
            4,
            2,
            j2k_core::PixelFormat::Gray8,
        );
        cache.insert(key(2), another.clone()).unwrap();
        assert!(cache.get(key(1)).is_none());
        assert_eq!(cache.retained_bytes, 24);
        cache.insert(key(0), another).unwrap();
        assert_eq!(cache.retained_bytes, 8);
    }
}
