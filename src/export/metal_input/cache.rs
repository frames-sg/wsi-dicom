use super::*;

const METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY: usize = 512;

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
}

impl Default for MetalSourceTileCache {
    fn default() -> Self {
        Self {
            capacity: METAL_WHOLE_LEVEL_SOURCE_TILE_CACHE_CAPACITY,
            entries: HashMap::new(),
            order: VecDeque::new(),
            next_generation: 0,
        }
    }
}

struct MetalSourceTileCacheEntry {
    tile: wsi_rs::output::metal::MetalDeviceTile,
    generation: u64,
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
    ) {
        if self.capacity == 0 {
            return;
        }
        let generation = self.next_generation();
        self.entries
            .insert(key, MetalSourceTileCacheEntry { tile, generation });
        self.order.push_back((key, generation));
        while self.entries.len() > self.capacity {
            let Some((oldest, generation)) = self.order.pop_front() else {
                break;
            };
            if self
                .entries
                .get(&oldest)
                .is_some_and(|entry| entry.generation == generation)
            {
                self.entries.remove(&oldest);
            }
        }
        self.compact_stale_order_entries_if_needed();
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
