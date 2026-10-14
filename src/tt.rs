// Fixed size, Each bucket is a 64-byte cache line
// each holding 4 entries
// ---------------------------------------------------------------------------

use crate::{search::MATE_THRESHOLD, types::Move};

const BUCKET_SIZE: usize = 4;
const BOUND_MASK: u8 = 0b11; // flags bits 0-1: bound (0 means empty slot)
const GEN_SHIFT: u8 = 2; // flags bits 2-7: search generation
const GEN_MASK: u8 = 0x3F;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    Exact,
    Lower,
    Upper,
}

impl Bound {
    #[inline]
    fn to_bits(self) -> u8 {
        match self {
            Bound::Exact => 1,
            Bound::Lower => 2,
            Bound::Upper => 3,
        }
    }

    #[inline]
    fn from_bits(bits: u8) -> Bound {
        match bits {
            1 => Bound::Exact,
            2 => Bound::Lower,
            _ => Bound::Upper,
        }
    }
}

// 16 bytes: 8 + 4 + 2 + 1 + 1
#[derive(Clone, Copy)]
#[repr(C)]
pub struct TTEntry {
    pub key: u64, // full zobrist hash, used to verify the slot really is this position
    pub value: i32,
    pub best_move: Move,
    pub depth: i8,
    pub flags: u8, // bound | generation << 2
}

impl TTEntry {
    const EMPTY: TTEntry = TTEntry {
        key: 0,
        value: 0,
        best_move: Move::NULL,
        depth: 0,
        flags: 0,
    };

    #[inline]
    fn is_empty(&self) -> bool {
        self.flags & BOUND_MASK == 0
    }

    #[inline]
    pub fn bound(&self) -> Bound {
        Bound::from_bits(self.flags & BOUND_MASK)
    }

    #[inline]
    fn generation(&self) -> u8 {
        self.flags >> GEN_SHIFT
    }
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct Bucket {
    entries: [TTEntry; BUCKET_SIZE],
}

impl Bucket {
    const EMPTY: Bucket = Bucket {
        entries: [TTEntry::EMPTY; BUCKET_SIZE],
    };
}

pub struct TranspositionTable {
    buckets: Vec<Bucket>,
    mask: usize,
    generation: u8,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let bytes = size_mb * 1024 * 1024;
        let wanted = (bytes / std::mem::size_of::<Bucket>()).max(1);
        // power of two bucket count so the index is just `hash & mask`
        let count = if wanted.is_power_of_two() {
            wanted
        } else {
            wanted.next_power_of_two() / 2
        };

        Self {
            buckets: vec![Bucket::EMPTY; count],
            mask: count - 1,
            generation: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buckets.fill(Bucket::EMPTY);
        self.generation = 0;
    }

    // call once at the start of every search so old entries can be told apart
    pub fn new_search(&mut self) {
        self.generation = (self.generation + 1) & GEN_MASK;
    }

    #[inline]
    pub fn index(&self, hash: u64) -> usize {
        (hash as usize) & self.mask
    }

    // hint to CPU to start fetching hash bucket into cache
    // called as soon as child hash is known
    #[inline]
    pub fn prefetch(&self, hash: u64) {
        #[cfg(target_arch = "x86_64")]
        {
            use core::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
            let ptr = &self.buckets[self.index(hash)] as *const Bucket as *const i8;
            unsafe { _mm_prefetch::<_MM_HINT_T0>(ptr) };
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = hash;
    }
    #[inline]
    pub fn probe(&self, hash: u64) -> Option<TTEntry> {
        self.buckets[self.index(hash)]
            .entries
            .iter()
            .find(|e| !e.is_empty() && e.key == hash)
            .copied()
    }

    pub fn store(&mut self, hash: u64, depth: i32, value: i32, bound: Bound, best_move: Move) {
        let cur_gen = self.generation;
        let idx = self.index(hash);
        let bucket = &mut self.buckets[idx];
        let depth8 = depth.clamp(i8::MIN as i32, i8::MAX as i32) as i8;

        // pick a slot: same position > empty > least valuable
        // (value = depth, minus a penalty for every search it has sat unused)
        let mut victim = 0;
        let mut victim_worth = i32::MAX;
        for (i, e) in bucket.entries.iter().enumerate() {
            if !e.is_empty() && e.key == hash {
                // same position: keep the old entry if it's deeper and from this search
                if depth8 < e.depth && e.generation() == cur_gen {
                    return;
                }
                victim = i;
                break;
            }
            if e.is_empty() {
                victim = i;
                victim_worth = i32::MIN;
                continue;
            }
            let age = (cur_gen.wrapping_sub(e.generation()) & GEN_MASK) as i32;
            let worth = e.depth as i32 - 8 * age;
            if worth < victim_worth {
                victim_worth = worth;
                victim = i;
            }
        }

        bucket.entries[victim] = TTEntry {
            key: hash,
            value,
            best_move,
            depth: depth8,
            flags: (cur_gen << GEN_SHIFT) | bound.to_bits(),
        };
    }

    // UCI hashfull print
    pub fn hashfull(&self) -> usize {
        let sample = self.buckets.len().min(250);
        let mut used = 0;
        for bucket in &self.buckets[..sample] {
            for e in &bucket.entries {
                if !e.is_empty() && e.generation() == self.generation {
                    used += 1;
                }
            }
        }
        used * 1000 / (sample * BUCKET_SIZE)
    }
}

#[inline]
pub fn score_to_tt(value: i32, ply: i32) -> i32 {
    if value >= MATE_THRESHOLD {
        value + ply
    } else if value <= -MATE_THRESHOLD {
        value - ply
    } else {
        value
    }
}

#[inline]
pub fn score_from_tt(value: i32, ply: i32) -> i32 {
    if value >= MATE_THRESHOLD {
        value - ply
    } else if value <= -MATE_THRESHOLD {
        value + ply
    } else {
        value
    }
}
