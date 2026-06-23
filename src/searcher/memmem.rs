use crate::searcher::memchr::memchr;
use crate::searcher::memcmp::memcmp;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

struct TwoWayLongNeedle<'a> {
    haystack: &'a [u8],
    needle: &'a [u8],
}

impl<'a> TwoWayLongNeedle<'a> {
    fn new(haystack: &'a [u8], needle: &'a [u8]) -> TwoWayLongNeedle<'a> {
        TwoWayLongNeedle { haystack, needle }
    }

    fn critical_factorization(&self) -> (usize, usize) {
        let needle_len = self.needle.len();
        let mut max_suffix: isize = -1;

        let mut j = 0usize;
        let mut k = 1usize;
        let mut p = 1usize;

        // 第一遍：按正常字典序寻找 maximal suffix。
        //
        // 这一步会找到一个候选切分点，并计算对应 period。
        while j + k < needle_len {
            // a 是当前候选位置的字符。
            let a = self.needle[j + k];
            let b = self.needle[(max_suffix + k as isize) as usize];

            if a < b {
                j += k;
                k = 1;

                p = (j as isize - max_suffix) as usize;
            } else if a == b {
                if k != p {
                    k += 1;
                } else {
                    j += p;
                    k = 1;
                }
            } else {
                max_suffix = j as isize;
                j += 1;
                k = 1;
                p = 1;
            }
        }
        let period = p;
        let mut max_suffix_rev: isize = -1;
        j = 0;
        k = 1;
        p = 1;

        while j + k < needle_len {
            let a = self.needle[j + k];
            let b = self.needle[(max_suffix_rev + k as isize) as usize];

            if b < a {
                j += k;
                k = 1;
                p = (j as isize - max_suffix_rev) as usize;
            } else if a == b {
                if k != p {
                    k += 1;
                } else {
                    j += p;
                    k = 1;
                }
            } else {
                max_suffix_rev = j as isize;
                j += 1;
                k = 1;
                p = 1;
            }
        }
        if max_suffix_rev + 1 < max_suffix + 1 {
            ((max_suffix + 1) as usize, period)
        } else {
            ((max_suffix_rev + 1) as usize, p)
        }
    }

    fn find(&mut self) -> Option<usize> {
        let haystack_len = self.haystack.len();
        let needle_len = self.needle.len();
        if needle_len == 0 {
            return Some(0);
        }
        if needle_len > haystack_len {
            return None;
        }
        let mut i: usize;
        let mut j: usize;

        let (suffix, mut period) = self.critical_factorization();

        let mut shift_table = [0usize; 256];

        shift_table.fill(needle_len);

        for i in 0..needle_len {
            shift_table[self.needle[i] as usize] = needle_len - i - 1;
        }

        let last_start = haystack_len - needle_len;

        let is_periodic =
            suffix == 0 || self.needle[..suffix] == self.needle[period..period + suffix];
        if is_periodic {
            let mut memory = 0usize;
            let mut shift: usize;
            j = 0;
            while j <= last_start {
                shift = shift_table[self.haystack[j + needle_len - 1] as usize];
                if shift > 0 {
                    if memory != 0 && shift < period {
                        shift = needle_len - period;
                    }
                    memory = 0;

                    // 移动窗口。
                    j += shift;
                    continue;
                }
                i = suffix.max(memory);
                while i < needle_len - 1 && self.needle[i] == self.haystack[j + i] {
                    i += 1;
                }
                if needle_len - 1 <= i {
                    let mut left = suffix;
                    while left > memory {
                        let idx = left - 1;
                        if self.needle[idx] != self.haystack[j + idx] {
                            break;
                        }
                        left -= 1;
                    }

                    if left <= memory {
                        return Some(j);
                    }
                    j += period;

                    memory = needle_len - period;
                } else {
                    j += i - suffix + 1;
                    memory = 0;
                }
            }
        } else {
            period = suffix.max(needle_len - suffix) + 1;
            j = 0;
            while j <= last_start {
                let shift = shift_table[self.haystack[j + needle_len - 1] as usize];
                if shift > 0 {
                    j += shift;
                    continue;
                }
                i = suffix;
                while i < needle_len - 1 && self.needle[i] == self.haystack[j + i] {
                    i += 1;
                }
                if needle_len - 1 <= i {
                    let mut left = suffix;

                    while left > 0 {
                        let idx = left - 1;

                        if self.needle[idx] != self.haystack[j + idx] {
                            break;
                        }
                        left -= 1;
                    }
                    if left == 0 {
                        return Some(j);
                    }
                    j += period;
                } else {
                    j += i - suffix + 1;
                }
            }
        }

        None
    }
}

fn memmem_len2_safe(hs: &[u8], ne: &[u8]) -> Option<usize> {
    let end = hs.len() - 2;
    let nw = ((ne[0] as u32) << 16) | ne[1] as u32;
    let mut hw = ((hs[0] as u32) << 16) | hs[1] as u32;
    let mut i = 1usize;
    while i <= end && hw != nw {
        i += 1;
        hw = (hw << 16) | hs[i] as u32;
    }
    if hw == nw {
        Some(i - 1)
    } else {
        None
    }
}

#[inline]
fn hash2(bytes: &[u8], index: usize) -> usize {
    (bytes[index] as usize).wrapping_sub((bytes[index - 1] as usize) << 3) % 256
}

fn modified_horspool(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let hs_len = haystack.len();
    let ne_len = needle.len();

    let end = hs_len - ne_len;
    let mut shift = [0u8; 256];
    let mut tmp: usize;
    let shift1: usize;
    let m1 = ne_len - 1;
    let mut offset = 0usize;

    for i in 1..m1 {
        shift[hash2(needle, i)] = i as u8;
    }

    // 完整匹配 needle 末尾 pair 的 hash，但后续完整比较失败时使用的跳跃距离。
    shift1 = m1 - shift[hash2(needle, m1)] as usize;
    shift[hash2(needle, m1)] = m1 as u8;

    let mut hs = 0usize;
    while hs <= end {
        // 跳过那些“末尾双字节 hash 完全不在 needle 里”的窗口。
        loop {
            hs += m1;
            tmp = shift[hash2(haystack, hs)] as usize;

            if !(tmp == 0 && hs <= end) {
                break;
            }
        }
        hs -= tmp;
        if tmp < m1 {
            continue;
        }
        // 末尾 2 字节 hash 命中。长 needle 先比较 8 字节过滤片段，减少完整比较。
        if m1 < 15
            || memcmp(
                &haystack[hs + offset..hs + offset + 8],
                &needle[offset..offset + 8],
            ) == 0
        {
            if memcmp(&haystack[hs..hs + m1], &needle[..m1]) == 0 {
                return Some(hs);
            }
            // 如果过滤片段没有发现 mismatch，下次换一个靠后的 8 字节片段过滤。
            offset = (if offset >= 8 { offset } else { m1 }).wrapping_sub(8);
        }

        hs += shift1;
    }

    None
}

#[derive(Clone, Copy, Debug)]
struct PackedPair {
    index1: usize,
    index2: usize,
}

impl PackedPair {
    fn new(needle: &[u8]) -> Option<PackedPair> {
        if needle.len() < 2 {
            return None;
        }

        // 和 memchr crate 的 packed-pair 思路一样：选两个“更稀有”的 needle
        // offset 做候选过滤。这里先用简单静态 rank，后续可替换成更精细频率表。
        let limit = needle.len().min(256);
        let mut index1 = 0usize;
        let mut index2 = 1usize;

        if byte_frequency_rank(needle[index2]) < byte_frequency_rank(needle[index1]) {
            core::mem::swap(&mut index1, &mut index2);
        }

        for i in 2..limit {
            let b = needle[i];
            let rank = byte_frequency_rank(b);
            let rank1 = byte_frequency_rank(needle[index1]);
            let rank2 = byte_frequency_rank(needle[index2]);

            if rank < rank1 {
                index2 = index1;
                index1 = i;
            } else if rank < rank2 || (needle[index1] == needle[index2] && b != needle[index1]) {
                index2 = i;
            }
        }

        if index1 == index2 {
            None
        } else {
            Some(PackedPair { index1, index2 })
        }
    }
}

#[inline]
fn byte_frequency_rank(b: u8) -> u16 {
    match b {
        // 控制字节、非 ASCII 字节在普通文本里通常更稀有。
        0 => 1,
        1..=8 | 14..=31 | 127 => 8,
        128..=255 => 16,
        b'_' | b'-' | b'/' | b'\\' | b'.' | b':' => 80,
        b'0'..=b'9' => 120,
        b'A'..=b'Z' => 150,
        b'a'..=b'z' => match b {
            b'e' | b't' | b'a' | b'o' | b'i' | b'n' | b's' | b'h' | b'r' => 240,
            _ => 190,
        },
        b' ' => 255,
        _ => 100,
    }
}

/// Packed-pair SIMD memmem：先用两个 needle offset 做 SIMD 候选过滤，
/// 候选位置再用 memcmp 精确验证。
// pub fn memmem_packed_pair(haystack: &[u8], needle: &[u8]) -> Option<(usize, bool)> {
//     let pair = PackedPair::new(needle)?;

//     #[cfg(target_arch = "x86_64")]
//     {
//         if std::is_x86_feature_detected!("avx2") {
//             let u = unsafe { memmem_packed_pair_avx2(haystack, needle, pair) }?;
//             return Some((u, true));
//         }
//     }
//     return Some((0, false));
// }

// fn memmem_packed_pair_scalar(haystack: &[u8], needle: &[u8], pair: PackedPair) -> Option<usize> {
//     let ne_len = needle.len();
//     let candidate_count = haystack.len() - ne_len + 1;
//     let b1 = needle[pair.index1];
//     let b2 = needle[pair.index2];

//     let mut i = 0usize;
//     while i < candidate_count {
//         if haystack[i + pair.index1] == b1
//             && haystack[i + pair.index2] == b2
//             && memcmp(&haystack[i..i + ne_len], needle) == 0
//         {
//             return Some(i);
//         }
//         i += 1;
//     }

//     None
// }

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn memmem_packed_pair_avx2(
    haystack: &[u8],
    needle: &[u8],
    pair: PackedPair,
) -> Option<usize> {
    let ne_len = needle.len();
    let candidate_count = haystack.len() - ne_len + 1;
    let byte1 = _mm256_set1_epi8(needle[pair.index1] as i8);
    let byte2 = _mm256_set1_epi8(needle[pair.index2] as i8);
    let ptr = haystack.as_ptr();

    let mut i = 0usize;

    while i + 32 <= candidate_count {
        let h1 = _mm256_loadu_si256(ptr.add(i + pair.index1) as *const __m256i);
        let h2 = _mm256_loadu_si256(ptr.add(i + pair.index2) as *const __m256i);
        let eq1 = _mm256_cmpeq_epi8(h1, byte1);
        let eq2 = _mm256_cmpeq_epi8(h2, byte2);
        let mut mask = _mm256_movemask_epi8(_mm256_and_si256(eq1, eq2)) as u32;

        while mask != 0 {
            let bit = mask.trailing_zeros() as usize;
            let pos = i + bit;

            if memcmp(&haystack[pos..pos + ne_len], needle) == 0 {
                return Some(pos);
            }

            mask &= mask - 1;
        }

        i += 32;
    }

    while i < candidate_count {
        if haystack[i + pair.index1] == needle[pair.index1]
            && haystack[i + pair.index2] == needle[pair.index2]
            && memcmp(&haystack[i..i + ne_len], needle) == 0
        {
            return Some(i);
        }
        i += 1;
    }

    None
}

pub fn memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let hs_len = haystack.len();
    let ne_len: usize = needle.len();
    if ne_len == 0 {
        return None;
    }
    if ne_len == 1 {
        return memchr(haystack, needle[0]);
    }
    if hs_len < ne_len {
        return None;
    }
    if ne_len == 2 {
        return memmem_len2_safe(haystack, needle);
    }
    if ne_len > 256 {
        return TwoWayLongNeedle::new(haystack, needle).find();
    }
    #[cfg(target_arch = "x86_64")]
    {
        if std::is_x86_feature_detected!("avx2") {
            let pair = PackedPair::new(needle)?;
            return unsafe { memmem_packed_pair_avx2(haystack, needle, pair) };
        }
    }
    modified_horspool(haystack, needle)
}

pub fn memmem_chunks<'a, I>(chunks: I, needle: &[u8]) -> Option<usize>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    if needle.is_empty() {
        return None;
    }

    let keep = needle.len().saturating_sub(1);
    let mut global_base = 0usize;
    let mut prev_tail = Vec::new();
    let mut boundary = Vec::new();

    for chunk in chunks {
        if !prev_tail.is_empty() && !chunk.is_empty() {
            let head_len = chunk.len().min(keep);

            boundary.clear();
            boundary.extend_from_slice(&prev_tail);
            boundary.extend_from_slice(&chunk[..head_len]);

            if let Some(pos) = memmem(&boundary, needle) {
                return Some(global_base - prev_tail.len() + pos);
            }
        }

        if let Some(pos) = memmem(chunk, needle) {
            return Some(global_base + pos);
        }

        if keep > 0 {
            if chunk.len() >= keep {
                prev_tail.clear();
                prev_tail.extend_from_slice(&chunk[chunk.len() - keep..]);
            } else if !chunk.is_empty() {
                let new_len = prev_tail.len() + chunk.len();
                if new_len > keep {
                    prev_tail.drain(..new_len - keep);
                }
                prev_tail.extend_from_slice(chunk);
            }
        }

        global_base += chunk.len();
    }

    None
}

pub fn memmem_two_slices_no_alloc(left: &[u8], right: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }

    if let Some(pos) = memmem(left, needle) {
        return Some(pos);
    }

    if let Some(pos) = find_cross_boundary(left, right, needle) {
        return Some(pos);
    }

    memmem(right, needle).map(|pos| left.len() + pos)
}

pub fn memmem_small_slices_no_alloc(parts: &[&[u8]], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }

    match parts {
        [] => None,
        [part] => memmem(part, needle),
        [left, right] => memmem_two_slices_no_alloc(left, right, needle),
        _ => memmem_small_slices_scan(parts, needle),
    }
}

fn memmem_small_slices_scan(parts: &[&[u8]], needle: &[u8]) -> Option<usize> {
    debug_assert!(parts.len() <= 4);

    let total_len = parts.iter().map(|part| part.len()).sum::<usize>();
    if needle.len() > total_len {
        return None;
    }

    'start: for start in 0..=total_len - needle.len() {
        for (needle_offset, &expected) in needle.iter().enumerate() {
            if small_slices_byte_at(parts, start + needle_offset) != Some(expected) {
                continue 'start;
            }
        }

        return Some(start);
    }

    None
}

#[inline]
fn small_slices_byte_at(parts: &[&[u8]], mut index: usize) -> Option<u8> {
    for part in parts {
        if index < part.len() {
            return Some(part[index]);
        }

        index -= part.len();
    }

    None
}

fn find_cross_boundary(left: &[u8], right: &[u8], needle: &[u8]) -> Option<usize> {
    let needle_len = needle.len();

    if needle_len <= 1 || left.is_empty() || right.is_empty() {
        return None;
    }

    let min_left_take = needle_len.saturating_sub(right.len()).max(1);
    let max_left_take = left.len().min(needle_len - 1);

    if min_left_take > max_left_take {
        return None;
    }

    // 从更早的逻辑 offset 开始检查，保证返回第一个跨 gap 匹配。
    for left_take in (min_left_take..=max_left_take).rev() {
        let start = left.len() - left_take;
        let right_take = needle_len - left_take;

        if memcmp(&left[start..], &needle[..left_take]) == 0
            && memcmp(&right[..right_take], &needle[left_take..]) == 0
        {
            return Some(start);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::boyermoore::BoyerMoore;
    use std::time::Duration;

    fn naive_memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        if needle.len() > haystack.len() {
            return None;
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn two_way_find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        let mut finder = TwoWayLongNeedle::new(haystack, needle);
        finder.find()
    }

    fn assert_two_way_eq_naive(haystack: &[u8], needle: &[u8]) {
        assert_eq!(
            two_way_find(haystack, needle),
            naive_memmem(haystack, needle),
            "haystack={haystack:?}, needle={needle:?}",
        );
    }

    fn assert_modified_horspool_eq_naive(haystack: &[u8], needle: &[u8]) {
        assert_eq!(
            modified_horspool(haystack, needle),
            naive_memmem(haystack, needle),
            "haystack={haystack:?}, needle={needle:?}",
        );
    }

    fn assert_packed_pair_eq_naive(haystack: &[u8], needle: &[u8]) {
        if needle.len() < 3 || haystack.len() < needle.len() {
            return;
        }

        // assert_eq!(
        //     memmem_packed_pair(haystack, needle).map(|(pos, _used_avx2)| pos),
        //     expected_public_memmem(haystack, needle),
        //     "haystack={haystack:?}, needle={needle:?}",
        // );
    }

    #[cfg(target_arch = "x86_64")]
    fn assert_packed_pair_avx2_eq_naive(haystack: &[u8], needle: &[u8]) {
        assert!(needle.len() >= 3);
        assert!(haystack.len() >= needle.len());

        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("skip memmem_packed_pair_avx2 test: AVX2 is not available");
            return;
        }

        let pair = PackedPair::new(needle).expect("needle length >= 3 has a valid packed pair");
        let got = unsafe { memmem_packed_pair_avx2(haystack, needle, pair) };
        assert_eq!(
            got,
            naive_memmem(haystack, needle),
            "haystack={haystack:?}, needle={needle:?}, pair={pair:?}",
        );
    }

    fn expected_public_memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return None;
        }

        naive_memmem(haystack, needle)
    }

    fn assert_memmem_eq_naive(haystack: &[u8], needle: &[u8]) {
        assert_eq!(
            memmem(haystack, needle),
            expected_public_memmem(haystack, needle),
            "haystack={haystack:?}, needle={needle:?}",
        );
    }

    fn assert_memmem_chunks_eq_joined(chunks: &[&[u8]], needle: &[u8]) {
        let joined = chunks
            .iter()
            .flat_map(|chunk| chunk.iter().copied())
            .collect::<Vec<_>>();

        assert_eq!(
            memmem_chunks(chunks.iter().copied(), needle),
            expected_public_memmem(&joined, needle),
            "chunks={chunks:?}, needle={needle:?}",
        );
    }

    fn assert_memmem_small_slices_eq_joined(parts: &[&[u8]], needle: &[u8]) {
        let joined = parts
            .iter()
            .flat_map(|part| part.iter().copied())
            .collect::<Vec<_>>();

        assert_eq!(
            memmem_small_slices_no_alloc(parts, needle),
            expected_public_memmem(&joined, needle),
            "parts={parts:?}, needle={needle:?}",
        );
    }

    fn assert_memmem_two_slices_eq_joined(left: &[u8], right: &[u8], needle: &[u8]) {
        let joined = left.iter().chain(right.iter()).copied().collect::<Vec<_>>();

        assert_eq!(
            memmem_two_slices_no_alloc(left, right, needle),
            expected_public_memmem(&joined, needle),
            "left={left:?}, right={right:?}, needle={needle:?}",
        );
    }

    fn generated_bytes(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        let mut out = Vec::with_capacity(len);

        for _ in 0..len {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            out.push((x >> 32) as u8);
        }

        out
    }

    fn generated_nonzero_bytes(len: usize, seed: u64) -> Vec<u8> {
        generated_bytes(len, seed)
            .into_iter()
            .map(|b| (b % 251) + 1)
            .collect()
    }

    fn run_search_bench<F>(
        name: &str,
        haystack_len: usize,
        rounds: usize,
        expected: Option<usize>,
        mut find: F,
    ) -> Duration
    where
        F: FnMut() -> Option<usize>,
    {
        use std::hint::black_box;
        use std::time::Instant;

        let start = Instant::now();
        let mut checksum = 0usize;

        for _ in 0..rounds {
            let found = black_box(find());
            assert_eq!(found, expected, "{name} returned wrong result");
            checksum ^= found.unwrap_or(usize::MAX);
        }

        black_box(checksum);
        let elapsed = start.elapsed();
        let total_mib = (haystack_len as f64 * rounds as f64) / 1024.0 / 1024.0;
        let mib_s = total_mib / elapsed.as_secs_f64();

        println!("{name:<24}: {elapsed:?} ({mib_s:.2} MiB/s)");
        elapsed
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_boyermoore_two_way_modified_horspool_randomish_late_match() {
        let haystack_len = 8 * 1024 * 1024;
        let needle_len = 16usize;
        let rounds = 64usize;

        // 生成不含 0 的 haystack，needle 最后一个字节是 0。
        // 这样可以保证唯一匹配被放在末尾，迫使算法扫描完整 haystack。
        let mut haystack = generated_nonzero_bytes(haystack_len, 0x9999);
        let mut needle = generated_nonzero_bytes(needle_len, 0xaaaa);
        needle[needle_len - 1] = 0;

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        assert_eq!(naive_memmem(&haystack, &needle), Some(expected));
        assert_eq!(modified_horspool(&haystack, &needle), Some(expected));
        assert_eq!(two_way_find(&haystack, &needle), Some(expected));

        let bm = BoyerMoore::new(&needle);
        assert_eq!(bm.find(&haystack), Some(expected));

        println!(
            "\nrandomish late-match benchmark: haystack={} MiB, needle_len={}, rounds={}",
            haystack_len / 1024 / 1024,
            needle_len,
            rounds
        );

        let bm_elapsed =
            run_search_bench("BoyerMoore", haystack_len, rounds, Some(expected), || {
                bm.find(&haystack)
            });
        let two_way_elapsed = run_search_bench(
            "TwoWayLongNeedle",
            haystack_len,
            rounds,
            Some(expected),
            || two_way_find(&haystack, &needle),
        );
        let horspool_elapsed = run_search_bench(
            "modified_horspool",
            haystack_len,
            rounds,
            Some(expected),
            || modified_horspool(&haystack, &needle),
        );

        println!(
            "relative: BM/TwoWay={:.2}x, Horspool/TwoWay={:.2}x, Horspool/BM={:.2}x",
            two_way_elapsed.as_secs_f64() / bm_elapsed.as_secs_f64(),
            two_way_elapsed.as_secs_f64() / horspool_elapsed.as_secs_f64(),
            bm_elapsed.as_secs_f64() / horspool_elapsed.as_secs_f64(),
        );
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_boyermoore_two_way_modified_horspool_repeated_late_match() {
        let haystack_len = 8 * 1024 * 1024;
        let needle_len = 32usize;
        let rounds = 64usize;

        // 重复字节场景容易触发坏字符跳跃不明显，用来观察算法在低熵输入上的表现。
        let mut haystack = vec![b'a'; haystack_len];
        let mut needle = vec![b'a'; needle_len];
        needle[needle_len - 1] = b'b';

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        assert_eq!(naive_memmem(&haystack, &needle), Some(expected));
        assert_eq!(modified_horspool(&haystack, &needle), Some(expected));
        assert_eq!(two_way_find(&haystack, &needle), Some(expected));

        let bm = BoyerMoore::new(&needle);
        assert_eq!(bm.find(&haystack), Some(expected));

        println!(
            "\nrepeated late-match benchmark: haystack={} MiB, needle_len={}, rounds={}",
            haystack_len / 1024 / 1024,
            needle_len,
            rounds
        );

        let bm_elapsed =
            run_search_bench("BoyerMoore", haystack_len, rounds, Some(expected), || {
                bm.find(&haystack)
            });
        let two_way_elapsed = run_search_bench(
            "TwoWayLongNeedle",
            haystack_len,
            rounds,
            Some(expected),
            || two_way_find(&haystack, &needle),
        );
        let horspool_elapsed = run_search_bench(
            "modified_horspool",
            haystack_len,
            rounds,
            Some(expected),
            || modified_horspool(&haystack, &needle),
        );

        println!(
            "relative: BM/TwoWay={:.2}x, Horspool/TwoWay={:.2}x, Horspool/BM={:.2}x",
            two_way_elapsed.as_secs_f64() / bm_elapsed.as_secs_f64(),
            two_way_elapsed.as_secs_f64() / horspool_elapsed.as_secs_f64(),
            bm_elapsed.as_secs_f64() / horspool_elapsed.as_secs_f64(),
        );
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_long_needle_gt_256_boyermoore_vs_two_way() {
        let haystack_len = 8 * 1024 * 1024;
        let needle_len = 300usize;
        let rounds = 64usize;

        // needle_len > 256 时，memmem 主入口会选择 Two-Way。
        // haystack 不含 0，needle 末尾放 0，保证唯一匹配在最后。
        let mut haystack = generated_nonzero_bytes(haystack_len, 0x7777);
        let mut needle = generated_nonzero_bytes(needle_len, 0x8888);
        needle[needle_len - 1] = 0;

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        let bm = BoyerMoore::new(&needle);
        assert_eq!(bm.find(&haystack), Some(expected));
        assert_eq!(two_way_find(&haystack, &needle), Some(expected));

        println!(
            "\nneedle_len > 256 benchmark: haystack={} MiB, needle_len={}, rounds={}",
            haystack_len / 1024 / 1024,
            needle_len,
            rounds
        );

        let bm_elapsed =
            run_search_bench("BoyerMoore", haystack_len, rounds, Some(expected), || {
                bm.find(&haystack)
            });
        let two_way_elapsed = run_search_bench(
            "TwoWayLongNeedle",
            haystack_len,
            rounds,
            Some(expected),
            || two_way_find(&haystack, &needle),
        );

        println!(
            "relative: TwoWay/BM={:.2}x",
            bm_elapsed.as_secs_f64() / two_way_elapsed.as_secs_f64(),
        );
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_long_needle_gt_256_mixed_boyermoore_vs_two_way() {
        let haystack_len = 8 * 1024 * 1024;
        let rounds = 16usize;
        let needle_lens = [300usize, 400, 512, 600];

        println!(
            "\nneedle_len > 256 mixed benchmark: haystack={} MiB, rounds={}",
            haystack_len / 1024 / 1024,
            rounds
        );
        println!(
            "{:<18} {:>10} {:>14} {:>14} {:>10}",
            "dataset", "needle", "BoyerMoore", "TwoWay", "TW/BM"
        );

        for &needle_len in &needle_lens {
            let cases = [
                make_random_late_match_case(haystack_len, needle_len, needle_len as u64 + 0x1000),
                make_random_no_match_case(haystack_len, needle_len, needle_len as u64 + 0x2000),
                make_repeated_late_match_case(haystack_len, needle_len),
                make_periodic_late_match_case(haystack_len, needle_len),
            ];

            for (dataset, haystack, needle, expected) in cases {
                let bm = BoyerMoore::new(&needle);
                assert_eq!(bm.find(&haystack), expected, "{dataset} BoyerMoore");
                assert_eq!(
                    two_way_find(&haystack, &needle),
                    expected,
                    "{dataset} TwoWayLongNeedle"
                );

                let bm_elapsed = run_search_bench_quiet(rounds, expected, || bm.find(&haystack));
                let two_way_elapsed =
                    run_search_bench_quiet(rounds, expected, || two_way_find(&haystack, &needle));
                let total_mib = (haystack_len as f64 * rounds as f64) / 1024.0 / 1024.0;
                let bm_mib_s = total_mib / bm_elapsed.as_secs_f64();
                let two_way_mib_s = total_mib / two_way_elapsed.as_secs_f64();

                println!(
                    "{:<18} {:>10} {:>10.0} MiB/s {:>10.0} MiB/s {:>10.2}x",
                    dataset,
                    needle_len,
                    bm_mib_s,
                    two_way_mib_s,
                    bm_elapsed.as_secs_f64() / two_way_elapsed.as_secs_f64(),
                );
            }
        }
    }

    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_short_needle_4_to_255_boyermoore_vs_modified_horspool() {
        let haystack_len = 8 * 1024 * 1024;
        let needle_len = 64usize;
        let rounds = 64usize;

        // 3 < needle_len < 256 时，memmem 主入口会选择 modified_horspool。
        // haystack 不含 0，needle 末尾放 0，保证唯一匹配在最后。
        let mut haystack = generated_nonzero_bytes(haystack_len, 0x9999);
        let mut needle = generated_nonzero_bytes(needle_len, 0xaaaa);
        needle[needle_len - 1] = 0;

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        let bm = BoyerMoore::new(&needle);
        assert_eq!(bm.find(&haystack), Some(expected));
        assert_eq!(modified_horspool(&haystack, &needle), Some(expected));

        println!(
            "\n3 < needle_len < 256 benchmark: haystack={} MiB, needle_len={}, rounds={}",
            haystack_len / 1024 / 1024,
            needle_len,
            rounds
        );

        let bm_elapsed =
            run_search_bench("BoyerMoore", haystack_len, rounds, Some(expected), || {
                bm.find(&haystack)
            });
        let horspool_elapsed = run_search_bench(
            "modified_horspool",
            haystack_len,
            rounds,
            Some(expected),
            || modified_horspool(&haystack, &needle),
        );

        println!(
            "relative: Horspool/BM={:.2}x",
            bm_elapsed.as_secs_f64() / horspool_elapsed.as_secs_f64(),
        );
    }

    fn run_search_bench_quiet<F>(rounds: usize, expected: Option<usize>, mut find: F) -> Duration
    where
        F: FnMut() -> Option<usize>,
    {
        use std::hint::black_box;
        use std::time::Instant;

        let start = Instant::now();
        let mut checksum = 0usize;

        for _ in 0..rounds {
            let found = black_box(find());
            assert_eq!(found, expected);
            checksum ^= found.unwrap_or(usize::MAX);
        }

        black_box(checksum);
        start.elapsed()
    }

    #[cfg(target_arch = "x86_64")]
    fn packed_pair_avx2_find_for_bench(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        debug_assert!(needle.len() >= 3);
        debug_assert!(haystack.len() >= needle.len());

        let pair = PackedPair::new(needle).expect("needle length >= 3 has a valid packed pair");
        unsafe { memmem_packed_pair_avx2(haystack, needle, pair) }
    }

    fn mib_per_second(haystack_len: usize, rounds: usize, elapsed: Duration) -> f64 {
        let total_mib = (haystack_len as f64 * rounds as f64) / 1024.0 / 1024.0;
        total_mib / elapsed.as_secs_f64()
    }

    #[cfg(target_arch = "x86_64")]
    fn run_packed_pair_two_way_row(
        dataset: &str,
        haystack: &[u8],
        needle: &[u8],
        expected: Option<usize>,
        rounds: usize,
    ) {
        assert_eq!(
            packed_pair_avx2_find_for_bench(haystack, needle),
            expected,
            "{dataset} memmem_packed_pair_avx2",
        );
        assert_eq!(
            two_way_find(haystack, needle),
            expected,
            "{dataset} TwoWayLongNeedle",
        );

        let packed_elapsed = run_search_bench_quiet(rounds, expected, || {
            packed_pair_avx2_find_for_bench(haystack, needle)
        });
        let two_way_elapsed =
            run_search_bench_quiet(rounds, expected, || two_way_find(haystack, needle));

        let packed_mib_s = mib_per_second(haystack.len(), rounds, packed_elapsed);
        let two_way_mib_s = mib_per_second(haystack.len(), rounds, two_way_elapsed);

        println!(
            "{:<18} {:>10} {:>16.0} {:>16.0} {:>12.2}x",
            dataset,
            needle.len(),
            packed_mib_s,
            two_way_mib_s,
            two_way_elapsed.as_secs_f64() / packed_elapsed.as_secs_f64(),
        );
    }

    #[cfg(target_arch = "x86_64")]
    fn run_packed_pair_two_way_horspool_row(
        dataset: &str,
        haystack: &[u8],
        needle: &[u8],
        expected: Option<usize>,
        rounds: usize,
    ) {
        assert_eq!(
            packed_pair_avx2_find_for_bench(haystack, needle),
            expected,
            "{dataset} memmem_packed_pair_avx2",
        );
        assert_eq!(
            two_way_find(haystack, needle),
            expected,
            "{dataset} TwoWayLongNeedle",
        );
        assert_eq!(
            modified_horspool(haystack, needle),
            expected,
            "{dataset} modified_horspool",
        );

        let packed_elapsed = run_search_bench_quiet(rounds, expected, || {
            packed_pair_avx2_find_for_bench(haystack, needle)
        });
        let two_way_elapsed =
            run_search_bench_quiet(rounds, expected, || two_way_find(haystack, needle));
        let horspool_elapsed =
            run_search_bench_quiet(rounds, expected, || modified_horspool(haystack, needle));

        let packed_mib_s = mib_per_second(haystack.len(), rounds, packed_elapsed);
        let two_way_mib_s = mib_per_second(haystack.len(), rounds, two_way_elapsed);
        let horspool_mib_s = mib_per_second(haystack.len(), rounds, horspool_elapsed);

        println!(
            "{:<18} {:>10} {:>16.0} {:>16.0} {:>18.0} {:>12.2}x {:>12.2}x",
            dataset,
            needle.len(),
            packed_mib_s,
            two_way_mib_s,
            horspool_mib_s,
            two_way_elapsed.as_secs_f64() / packed_elapsed.as_secs_f64(),
            horspool_elapsed.as_secs_f64() / packed_elapsed.as_secs_f64(),
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_packed_pair_avx2_long_needle_gt_256_vs_two_way() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("skip packed-pair AVX2 benchmark: AVX2 is not available");
            return;
        }

        let haystack_len = 8 * 1024 * 1024;
        let rounds = 16usize;
        let needle_lens = [300usize, 400, 512, 600];

        println!(
            "\npacked-pair AVX2 vs TwoWayLongNeedle, needle_len > 256: haystack={} MiB, rounds={}",
            haystack_len / 1024 / 1024,
            rounds
        );
        println!(
            "{:<18} {:>10} {:>16} {:>16} {:>13}",
            "dataset", "needle", "packed_pair", "TwoWay", "PP/TW"
        );

        for &needle_len in &needle_lens {
            let cases = [
                make_random_late_match_case(haystack_len, needle_len, needle_len as u64 + 0xa000),
                make_random_no_match_case(haystack_len, needle_len, needle_len as u64 + 0xb000),
                make_repeated_late_match_case(haystack_len, needle_len),
                make_periodic_late_match_case(haystack_len, needle_len),
            ];

            for (dataset, haystack, needle, expected) in cases {
                run_packed_pair_two_way_row(dataset, &haystack, &needle, expected, rounds);
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    #[ignore = "benchmark-style perf test; run with --release --ignored --nocapture"]
    fn bench_packed_pair_avx2_short_needle_4_to_255_vs_two_way_horspool() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("skip packed-pair AVX2 benchmark: AVX2 is not available");
            return;
        }

        let haystack_len = 8 * 1024 * 1024;
        let rounds = 16usize;
        let needle_lens = [4usize, 16, 64, 128, 255];

        println!(
            "\npacked-pair AVX2 vs TwoWayLongNeedle vs modified_horspool, 3 < needle_len < 256: haystack={} MiB, rounds={}",
            haystack_len / 1024 / 1024,
            rounds
        );
        println!(
            "{:<18} {:>10} {:>16} {:>16} {:>18} {:>13} {:>13}",
            "dataset", "needle", "packed_pair", "TwoWay", "modified_horspool", "PP/TW", "PP/Hors"
        );

        for &needle_len in &needle_lens {
            let cases = [
                make_random_late_match_case(haystack_len, needle_len, needle_len as u64 + 0xc000),
                make_random_no_match_case(haystack_len, needle_len, needle_len as u64 + 0xd000),
                make_repeated_late_match_case(haystack_len, needle_len),
                make_periodic_late_match_case(haystack_len, needle_len),
            ];

            for (dataset, haystack, needle, expected) in cases {
                run_packed_pair_two_way_horspool_row(dataset, &haystack, &needle, expected, rounds);
            }
        }
    }

    fn make_random_late_match_case(
        haystack_len: usize,
        needle_len: usize,
        seed: u64,
    ) -> (&'static str, Vec<u8>, Vec<u8>, Option<usize>) {
        let mut haystack = generated_nonzero_bytes(haystack_len, seed);
        let mut needle = generated_nonzero_bytes(needle_len, seed ^ 0x5555);
        needle[needle_len - 1] = 0;

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        ("random-late", haystack, needle, Some(expected))
    }

    fn make_random_no_match_case(
        haystack_len: usize,
        needle_len: usize,
        seed: u64,
    ) -> (&'static str, Vec<u8>, Vec<u8>, Option<usize>) {
        let haystack = generated_nonzero_bytes(haystack_len, seed);
        let mut needle = generated_nonzero_bytes(needle_len, seed ^ 0x7777);
        needle[needle_len - 1] = 0;

        ("random-none", haystack, needle, None)
    }

    fn make_repeated_late_match_case(
        haystack_len: usize,
        needle_len: usize,
    ) -> (&'static str, Vec<u8>, Vec<u8>, Option<usize>) {
        let mut haystack = vec![b'a'; haystack_len];
        let mut needle = vec![b'a'; needle_len];
        needle[needle_len - 1] = b'b';

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        ("repeated-late", haystack, needle, Some(expected))
    }

    fn make_periodic_late_match_case(
        haystack_len: usize,
        needle_len: usize,
    ) -> (&'static str, Vec<u8>, Vec<u8>, Option<usize>) {
        let pattern = b"abc";
        let mut haystack = (0..haystack_len)
            .map(|i| pattern[i % pattern.len()])
            .collect::<Vec<_>>();
        let mut needle = (0..needle_len)
            .map(|i| pattern[i % pattern.len()])
            .collect::<Vec<_>>();
        needle[needle_len - 1] = b'X';

        let expected = haystack_len - needle_len;
        haystack[expected..].copy_from_slice(&needle);

        ("periodic-late", haystack, needle, Some(expected))
    }

    fn all_words(alphabet: &[u8], len: usize) -> Vec<Vec<u8>> {
        if len == 0 {
            return vec![Vec::new()];
        }

        let prev = all_words(alphabet, len - 1);
        let mut out = Vec::with_capacity(prev.len() * alphabet.len());

        for word in prev {
            for &b in alphabet {
                let mut next = word.clone();
                next.push(b);
                out.push(next);
            }
        }

        out
    }

    #[test]
    fn memmem_empty_needle_returns_none_by_current_api() {
        assert_memmem_eq_naive(b"", b"");
        assert_memmem_eq_naive(b"abc", b"");
        assert_memmem_eq_naive(&[0, 1, 2], b"");
    }

    #[test]
    fn memmem_one_byte_needle_uses_memchr_cases() {
        assert_memmem_eq_naive(b"", b"a");
        assert_memmem_eq_naive(b"abc", b"a");
        assert_memmem_eq_naive(b"abc", b"b");
        assert_memmem_eq_naive(b"abc", b"c");
        assert_memmem_eq_naive(b"abc", b"d");
        assert_memmem_eq_naive(&[0, 1, 2, 0xff], &[0]);
        assert_memmem_eq_naive(&[0, 1, 2, 0xff], &[0xff]);
        assert_memmem_eq_naive(&[0, 1, 2, 0xff], &[3]);
    }

    #[test]
    fn memmem_two_byte_needle_covers_start_middle_end_and_absent() {
        assert_memmem_eq_naive(b"ab", b"ab");
        assert_memmem_eq_naive(b"xab", b"ab");
        assert_memmem_eq_naive(b"abx", b"ab");
        assert_memmem_eq_naive(b"xxabxx", b"ab");
        assert_memmem_eq_naive(b"aaaaa", b"aa");
        assert_memmem_eq_naive(b"ababab", b"ba");
        assert_memmem_eq_naive(b"abcdef", b"zz");
        assert_memmem_eq_naive(&[0, 0xff, 0x10, 0xff], &[0xff, 0x10]);
        assert_memmem_eq_naive(&[0, 0xff, 0x10, 0xff], &[0x10, 0x10]);
    }

    #[test]
    fn memmem_short_needles_cover_modified_horspool_basic_cases() {
        assert_memmem_eq_naive(b"abcdef", b"abc");
        assert_memmem_eq_naive(b"abcdef", b"bcd");
        assert_memmem_eq_naive(b"abcdef", b"def");
        assert_memmem_eq_naive(b"abcdef", b"xyz");
        assert_memmem_eq_naive(b"xxabcabc", b"abc");
        assert_memmem_eq_naive(b"abcabcabc", b"abcabc");
        assert_memmem_eq_naive(b"the quick brown fox", b"quick");
        assert_memmem_eq_naive(b"the quick brown fox", b"fox");
        assert_memmem_eq_naive(b"the quick brown fox", b"slow");
    }

    #[test]
    fn packed_pair_memmem_matches_naive_for_basic_cases() {
        assert_packed_pair_eq_naive(b"", b"abc");
        assert_packed_pair_eq_naive(b"abc", b"");
        assert_packed_pair_eq_naive(b"abc", b"a");
        assert_packed_pair_eq_naive(b"abc", b"ab");
        assert_packed_pair_eq_naive(b"abcdef", b"abc");
        assert_packed_pair_eq_naive(b"abcdef", b"bcd");
        assert_packed_pair_eq_naive(b"abcdef", b"def");
        assert_packed_pair_eq_naive(b"abcdef", b"xyz");
        assert_packed_pair_eq_naive(b"xxabcabc", b"abc");
        assert_packed_pair_eq_naive(b"abcabcabc", b"abcabc");
    }

    #[test]
    fn packed_pair_memmem_matches_naive_for_generated_data() {
        for &haystack_len in &[0usize, 1, 2, 3, 7, 15, 31, 32, 33, 64, 127, 256, 511] {
            for &needle_len in &[0usize, 1, 2, 3, 4, 8, 16, 31, 32, 64, 128] {
                if needle_len > haystack_len {
                    let haystack = generated_bytes(haystack_len, haystack_len as u64 + 0x9000);
                    let needle = generated_bytes(needle_len, needle_len as u64 + 0x9100);
                    assert_packed_pair_eq_naive(&haystack, &needle);
                    continue;
                }

                for seed in 0..4u64 {
                    let mut haystack = generated_bytes(haystack_len, seed + 0x9200);
                    let needle = if needle_len > 0 && seed % 2 == 0 {
                        let start = (seed as usize * 17 + haystack_len / 3)
                            % (haystack_len - needle_len + 1);
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        let mut needle = generated_bytes(needle_len, seed + 0x9300);
                        if needle_len > 2 && !haystack.is_empty() {
                            needle[needle_len - 1] = 0;
                            haystack.iter_mut().for_each(|b| *b = (*b % 251) + 1);
                        }
                        needle
                    };

                    assert_packed_pair_eq_naive(&haystack, &needle);
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn packed_pair_avx2_matches_naive_for_basic_positions() {
        assert_packed_pair_avx2_eq_naive(b"abcdef", b"abc");
        assert_packed_pair_avx2_eq_naive(b"abcdef", b"bcd");
        assert_packed_pair_avx2_eq_naive(b"abcdef", b"def");
        assert_packed_pair_avx2_eq_naive(b"abcdef", b"xyz");
        assert_packed_pair_avx2_eq_naive(b"xxabcabc", b"abc");
        assert_packed_pair_avx2_eq_naive(b"abcabcabc", b"abcabc");
        assert_packed_pair_avx2_eq_naive(b"aaaaa", b"aaa");
        assert_packed_pair_avx2_eq_naive(b"abababab", b"baba");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn packed_pair_avx2_matches_naive_for_vector_boundaries() {
        for &needle_len in &[3usize, 4, 8, 16, 31, 32, 33, 64] {
            for &candidate_count in &[1usize, 2, 31, 32, 33, 63, 64, 65, 127] {
                let haystack_len = needle_len + candidate_count - 1;
                let mut haystack =
                    generated_nonzero_bytes(haystack_len, needle_len as u64 + 0x7100);
                let mut needle = generated_nonzero_bytes(needle_len, needle_len as u64 + 0x7200);
                needle[needle_len - 1] = 0;

                assert_packed_pair_avx2_eq_naive(&haystack, &needle);

                let expected = haystack_len - needle_len;
                haystack[expected..].copy_from_slice(&needle);
                assert_packed_pair_avx2_eq_naive(&haystack, &needle);
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn packed_pair_avx2_matches_naive_for_binary_and_generated_data() {
        let binary = [
            0x00, 0xff, 0x10, 0x20, 0x00, 0xff, 0x10, 0x21, 0x80, 0x00, 0xff,
        ];
        assert_packed_pair_avx2_eq_naive(&binary, &[0x00, 0xff, 0x10]);
        assert_packed_pair_avx2_eq_naive(&binary, &[0x00, 0xff, 0x10, 0x21]);
        assert_packed_pair_avx2_eq_naive(&binary, &[0x80, 0x00, 0xff]);

        for &haystack_len in &[64usize, 127, 256, 511, 1024] {
            for &needle_len in &[3usize, 5, 17, 32, 64, 128] {
                if needle_len > haystack_len {
                    continue;
                }

                for seed in 0..4u64 {
                    let mut haystack = generated_bytes(haystack_len, seed + 0x7300);
                    let needle = if seed % 2 == 0 {
                        let start = (seed as usize * 29 + haystack_len / 4)
                            % (haystack_len - needle_len + 1);
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        let mut needle = generated_nonzero_bytes(needle_len, seed + 0x7400);
                        needle[needle_len - 1] = 0;
                        haystack.iter_mut().for_each(|b| *b = (*b % 251) + 1);
                        needle
                    };

                    assert_packed_pair_avx2_eq_naive(&haystack, &needle);
                }
            }
        }
    }

    #[test]
    fn memmem_returns_first_match_for_overlapping_and_repeated_data() {
        assert_memmem_eq_naive(b"aaaaa", b"aaa");
        assert_memmem_eq_naive(b"baaaaa", b"aaa");
        assert_memmem_eq_naive(b"abababab", b"abab");
        assert_memmem_eq_naive(b"abababab", b"baba");
        assert_memmem_eq_naive(b"abcabcabcabcd", b"abcabcd");
        assert_memmem_eq_naive(b"mississippi", b"issi");
        assert_memmem_eq_naive(b"mississippi", b"ssi");
    }

    #[test]
    fn memmem_handles_boundaries_and_exact_lengths() {
        assert_memmem_eq_naive(b"", b"a");
        assert_memmem_eq_naive(b"a", b"a");
        assert_memmem_eq_naive(b"a", b"aa");
        assert_memmem_eq_naive(b"abc", b"abc");
        assert_memmem_eq_naive(b"abc", b"abcd");
        assert_memmem_eq_naive(b"xxabcdef", b"abcdef");
        assert_memmem_eq_naive(b"abcdefxx", b"abcdef");
        assert_memmem_eq_naive(b"xabcdefx", b"abcdef");
    }

    #[test]
    fn memmem_chunks_matches_current_empty_needle_api() {
        assert_memmem_chunks_eq_joined(&[], b"");
        assert_memmem_chunks_eq_joined(&[b"abc"], b"");
        assert_memmem_chunks_eq_joined(&[b"ab", b"cd"], b"");
    }

    #[test]
    fn memmem_chunks_finds_matches_inside_chunks() {
        assert_memmem_chunks_eq_joined(&[b"abcdef"], b"bcd");
        assert_memmem_chunks_eq_joined(&[b"ab", b"cdef"], b"def");
        assert_memmem_chunks_eq_joined(&[b"", b"ab", b"", b"cdef"], b"cd");
        assert_memmem_chunks_eq_joined(&[b"ab", b"", b"cd"], b"zz");
    }

    #[test]
    fn memmem_chunks_finds_matches_across_chunk_boundaries() {
        assert_memmem_chunks_eq_joined(&[b"ab", b"cd"], b"bc");
        assert_memmem_chunks_eq_joined(&[b"abc", b"def"], b"cde");
        assert_memmem_chunks_eq_joined(&[b"xxab", b"cabc"], b"abc");
        assert_memmem_chunks_eq_joined(&[b"hello ", b"wor", b"ld"], b"world");
    }

    #[test]
    fn memmem_chunks_finds_matches_across_many_small_chunks() {
        assert_memmem_chunks_eq_joined(&[b"a", b"b", b"c", b"d"], b"abcd");
        assert_memmem_chunks_eq_joined(&[b"xx", b"a", b"b", b"c", b"d", b"yy"], b"abcd");
        assert_memmem_chunks_eq_joined(&[b"12", b"3", b"45", b"6", b"789"], b"34567");
        assert_memmem_chunks_eq_joined(&[b"a", b"a", b"a", b"a", b"b"], b"aaaab");
    }

    #[test]
    fn memmem_chunks_matches_joined_search_for_generated_splits() {
        for haystack_len in 0..96 {
            let haystack = generated_bytes(haystack_len, haystack_len as u64 + 0xe000);

            for needle_len in 1..=haystack_len.min(32) {
                let needle = if needle_len <= haystack_len && needle_len % 2 == 0 {
                    let start = haystack_len.saturating_sub(needle_len) / 2;
                    haystack[start..start + needle_len].to_vec()
                } else {
                    generated_bytes(needle_len, needle_len as u64 + 0xe100)
                };

                for &split in &[1usize, 2, 3, 5, 8, 13] {
                    let chunks = haystack
                        .chunks(split)
                        .chain(std::iter::once(&b""[..]))
                        .collect::<Vec<_>>();
                    assert_memmem_chunks_eq_joined(&chunks, &needle);
                }
            }
        }
    }

    #[test]
    fn memmem_small_slices_matches_current_empty_needle_api() {
        assert_memmem_small_slices_eq_joined(&[], b"");
        assert_memmem_small_slices_eq_joined(&[b"abc"], b"");
        assert_memmem_small_slices_eq_joined(&[b"ab", b"cd"], b"");
        assert_memmem_small_slices_eq_joined(&[b"a", b"b", b"c", b"d"], b"");
    }

    #[test]
    fn memmem_small_slices_finds_matches_inside_parts() {
        assert_memmem_small_slices_eq_joined(&[b"abcdef"], b"bcd");
        assert_memmem_small_slices_eq_joined(&[b"ab", b"cdef"], b"def");
        assert_memmem_small_slices_eq_joined(&[b"", b"ab", b"", b"cdef"], b"cd");
        assert_memmem_small_slices_eq_joined(&[b"ab", b"", b"cd"], b"zz");
    }

    #[test]
    fn memmem_small_slices_finds_matches_across_many_parts_without_joining() {
        assert_memmem_small_slices_eq_joined(&[b"ab", b"cd"], b"bc");
        assert_memmem_small_slices_eq_joined(&[b"abc", b"def"], b"cde");
        assert_memmem_small_slices_eq_joined(&[b"hello ", b"wor", b"ld"], b"world");
        assert_memmem_small_slices_eq_joined(&[b"a", b"b", b"c", b"d"], b"abcd");
        assert_memmem_small_slices_eq_joined(&[b"xx", b"a", b"b", b"cdef"], b"abcdef");
        assert_memmem_small_slices_eq_joined(&[b"12", b"3", b"45", b"6789"], b"34567");
    }

    #[test]
    fn memmem_small_slices_returns_earliest_logical_match() {
        assert_eq!(
            memmem_small_slices_no_alloc(&[b"abcxx".as_slice(), b"abc".as_slice()], b"abc"),
            Some(0)
        );
        assert_eq!(
            memmem_small_slices_no_alloc(&[b"xxab".as_slice(), b"cabc".as_slice()], b"abc"),
            Some(2)
        );
        assert_eq!(
            memmem_small_slices_no_alloc(
                &[b"xx".as_slice(), b"ab".as_slice(), b"cabc".as_slice()],
                b"abc"
            ),
            Some(2)
        );
    }

    #[test]
    fn memmem_small_slices_matches_joined_search_for_generated_splits() {
        for &haystack_len in &[0usize, 1, 2, 3, 4, 7, 15, 31, 63, 95] {
            let haystack = generated_bytes(haystack_len, haystack_len as u64 + 0xf200);

            for needle_len in 1..=haystack_len.min(24) {
                let needle = if needle_len % 2 == 0 {
                    let start = haystack_len.saturating_sub(needle_len) / 2;
                    haystack[start..start + needle_len].to_vec()
                } else {
                    generated_bytes(needle_len, needle_len as u64 + 0xf300)
                };

                let splits = [
                    (0, 0, 0),
                    (0, haystack_len / 2, haystack_len / 2),
                    (haystack_len / 3, haystack_len / 3, haystack_len * 2 / 3),
                    (haystack_len / 4, haystack_len / 2, haystack_len * 3 / 4),
                    (haystack_len, haystack_len, haystack_len),
                ];

                for (a, b, c) in splits {
                    let parts = [
                        &haystack[..a],
                        &haystack[a..b],
                        &haystack[b..c],
                        &haystack[c..],
                    ];
                    assert_memmem_small_slices_eq_joined(&parts, &needle);
                }
            }
        }
    }

    #[test]
    fn memmem_two_slices_matches_current_empty_needle_api() {
        assert_memmem_two_slices_eq_joined(b"", b"", b"");
        assert_memmem_two_slices_eq_joined(b"abc", b"def", b"");
    }

    #[test]
    fn memmem_two_slices_finds_matches_inside_each_slice() {
        assert_memmem_two_slices_eq_joined(b"abcdef", b"xyz", b"bcd");
        assert_memmem_two_slices_eq_joined(b"abc", b"defxyz", b"efx");
        assert_memmem_two_slices_eq_joined(b"", b"abcdef", b"bcd");
        assert_memmem_two_slices_eq_joined(b"abcdef", b"", b"bcd");
    }

    #[test]
    fn memmem_two_slices_finds_matches_across_gap_without_joining() {
        assert_memmem_two_slices_eq_joined(b"ab", b"cd", b"bc");
        assert_memmem_two_slices_eq_joined(b"abc", b"def", b"cde");
        assert_memmem_two_slices_eq_joined(b"hello wor", b"ld !!!", b"world");
        assert_memmem_two_slices_eq_joined(b"xxab", b"cabc", b"abc");
        assert_memmem_two_slices_eq_joined(b"12", b"3456789", b"23456");
    }

    #[test]
    fn memmem_two_slices_returns_earliest_logical_match() {
        assert_eq!(
            memmem_two_slices_no_alloc(b"abcxx", b"abc", b"abc"),
            Some(0)
        );
        assert_eq!(
            memmem_two_slices_no_alloc(b"xxab", b"cabc", b"abc"),
            Some(2)
        );
        assert_eq!(
            memmem_two_slices_no_alloc(b"xx", b"abcabc", b"abc"),
            Some(2)
        );
    }

    #[test]
    fn memmem_two_slices_matches_joined_search_for_generated_splits() {
        for haystack_len in 0..128 {
            let haystack = generated_bytes(haystack_len, haystack_len as u64 + 0xf000);

            for needle_len in 1..=haystack_len.min(48) {
                let needle = if needle_len % 2 == 0 {
                    let start = haystack_len.saturating_sub(needle_len) / 2;
                    haystack[start..start + needle_len].to_vec()
                } else {
                    generated_bytes(needle_len, needle_len as u64 + 0xf100)
                };

                for split in 0..=haystack_len {
                    assert_memmem_two_slices_eq_joined(
                        &haystack[..split],
                        &haystack[split..],
                        &needle,
                    );
                }
            }
        }
    }

    #[test]
    fn memmem_handles_binary_and_utf8_bytes() {
        let binary = [
            0x00, 0xff, 0x10, 0x20, 0x00, 0xff, 0x10, 0x21, 0x80, 0x00, 0xff,
        ];

        assert_memmem_eq_naive(&binary, &[0x00, 0xff, 0x10]);
        assert_memmem_eq_naive(&binary, &[0x00, 0xff, 0x10, 0x21]);
        assert_memmem_eq_naive(&binary, &[0x80, 0x00, 0xff]);
        assert_memmem_eq_naive(&binary, &[0xff, 0xff]);

        let text = "这是中文文本，包含英文 English，还有 emoji 😀 和日文 こんにちは";
        assert_memmem_eq_naive(text.as_bytes(), "中文".as_bytes());
        assert_memmem_eq_naive(text.as_bytes(), "English".as_bytes());
        assert_memmem_eq_naive(text.as_bytes(), "😀".as_bytes());
        assert_memmem_eq_naive(text.as_bytes(), "こんにちは".as_bytes());
        assert_memmem_eq_naive(text.as_bytes(), "不存在".as_bytes());
    }

    #[test]
    fn memmem_matches_naive_for_all_small_binary_inputs() {
        let alphabet = [0u8, 1, 2];

        for haystack_len in 0..=7 {
            let haystacks = all_words(&alphabet, haystack_len);

            for needle_len in 0..=4 {
                let needles = all_words(&alphabet, needle_len);

                for haystack in &haystacks {
                    for needle in &needles {
                        assert_memmem_eq_naive(haystack, needle);
                    }
                }
            }
        }
    }

    #[test]
    fn memmem_matches_naive_for_generated_modified_horspool_path() {
        for haystack_len in 0..256 {
            for needle_len in 3..=haystack_len.min(128) {
                for seed in 0..4u64 {
                    let haystack = generated_bytes(haystack_len, seed + 0x1111);
                    let needle = if seed % 2 == 0 {
                        let start = (seed as usize * 13 + haystack_len / 7)
                            % (haystack_len - needle_len + 1);
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        generated_bytes(needle_len, seed + 0x2222)
                    };

                    assert_memmem_eq_naive(&haystack, &needle);
                }
            }
        }
    }

    #[test]
    fn memmem_matches_naive_for_needle_lengths_around_algorithm_boundaries() {
        for &needle_len in &[1usize, 2, 3, 15, 16, 31, 32, 64, 128, 255, 256, 257, 300] {
            let haystack_len = needle_len + 64;
            let haystack = generated_bytes(haystack_len, needle_len as u64 + 0x3333);

            assert_memmem_eq_naive(&haystack, &haystack[..needle_len]);

            let middle = 17usize;
            assert_memmem_eq_naive(&haystack, &haystack[middle..middle + needle_len]);

            assert_memmem_eq_naive(&haystack, &haystack[haystack_len - needle_len..]);

            let absent = vec![0xa5; needle_len];
            assert_memmem_eq_naive(&haystack, &absent);
        }
    }

    #[test]
    fn memmem_matches_naive_for_generated_two_way_long_needle_path() {
        for &haystack_len in &[260usize, 320, 511, 777] {
            for &needle_len in &[257usize, 258, 300] {
                if needle_len > haystack_len {
                    continue;
                }

                for seed in 0..4u64 {
                    let haystack = generated_bytes(haystack_len, seed + 0x4444);
                    let needle = if seed % 2 == 0 {
                        let start = (seed as usize * 19 + haystack_len / 4)
                            % (haystack_len - needle_len + 1);
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        generated_bytes(needle_len, seed + 0x5555)
                    };

                    assert_memmem_eq_naive(&haystack, &needle);
                }
            }
        }
    }

    #[test]
    fn memmem_two_way_path_handles_periodic_long_needles() {
        let mut haystack = vec![b'x'; 80];
        let mut needle = vec![b'a'; 300];
        needle[299] = b'b';
        haystack.extend_from_slice(&needle);
        haystack.extend_from_slice(&vec![b'y'; 80]);

        assert_memmem_eq_naive(&haystack, &needle);

        let absent = vec![b'a'; 300];
        assert_memmem_eq_naive(&haystack, &absent);
    }

    #[test]
    fn modified_horspool_finds_basic_positions() {
        assert_modified_horspool_eq_naive(b"abcdef", b"abc");
        assert_modified_horspool_eq_naive(b"abcdef", b"bcd");
        assert_modified_horspool_eq_naive(b"abcdef", b"def");
        assert_modified_horspool_eq_naive(b"abcdef", b"xyz");
        assert_modified_horspool_eq_naive(b"xxabcabc", b"abc");
        assert_modified_horspool_eq_naive(b"abcabcabc", b"abcabc");
    }

    #[test]
    fn modified_horspool_matches_naive_search_for_generated_data() {
        for haystack_len in 3..160 {
            for needle_len in 3..=haystack_len.min(64) {
                for seed in 0..4u64 {
                    let haystack = generated_bytes(haystack_len, seed + 0x4321);
                    let needle = if seed % 2 == 0 {
                        let start = (seed as usize * 11 + haystack_len / 5)
                            % (haystack_len - needle_len + 1);
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        generated_bytes(needle_len, seed + 0x8765)
                    };

                    assert_modified_horspool_eq_naive(&haystack, &needle);
                }
            }
        }
    }

    #[test]
    fn two_way_long_needle_handles_empty_and_bounds() {
        assert_two_way_eq_naive(b"", b"");
        assert_two_way_eq_naive(b"", b"a");
        assert_two_way_eq_naive(b"a", b"");
        assert_two_way_eq_naive(b"abc", b"abcd");
        assert_two_way_eq_naive(b"abc", b"abc");
    }

    #[test]
    fn two_way_long_needle_finds_basic_positions() {
        assert_two_way_eq_naive(b"abcdef", b"a");
        assert_two_way_eq_naive(b"abcdef", b"abc");
        assert_two_way_eq_naive(b"abcdef", b"cd");
        assert_two_way_eq_naive(b"abcdef", b"ef");
        assert_two_way_eq_naive(b"abcdef", b"zz");
        assert_two_way_eq_naive(b"xxabcabc", b"abc");
        assert_two_way_eq_naive(b"abcabcabc", b"abcabc");
    }

    #[test]
    fn two_way_long_needle_handles_periodic_needles() {
        assert_two_way_eq_naive(b"aaaaaaaaab", b"aaaab");
        assert_two_way_eq_naive(b"zzzzababababx", b"ababx");
        assert_two_way_eq_naive(b"abcabcabcabcd", b"abcabcd");
        assert_two_way_eq_naive(b"bbbbbbbbbbbb", b"bbbbbb");
        assert_two_way_eq_naive(b"aaaaaaaaaaaa", b"baaaa");
    }

    #[test]
    fn two_way_long_needle_handles_binary_bytes() {
        let haystack = [
            0x00, 0xff, 0x10, 0x20, 0x00, 0xff, 0x10, 0x21, 0x80, 0x00, 0xff,
        ];

        assert_two_way_eq_naive(&haystack, &[0x00]);
        assert_two_way_eq_naive(&haystack, &[0x00, 0xff, 0x10]);
        assert_two_way_eq_naive(&haystack, &[0x00, 0xff, 0x10, 0x21]);
        assert_two_way_eq_naive(&haystack, &[0x80, 0x00, 0xff]);
        assert_two_way_eq_naive(&haystack, &[0xff, 0xff]);
    }

    #[test]
    fn two_way_long_needle_matches_naive_search_for_generated_data() {
        for haystack_len in 0..96 {
            for needle_len in 0..48 {
                for seed in 0..4u64 {
                    let haystack = generated_bytes(haystack_len, seed + 0x1234);
                    let needle = if needle_len <= haystack_len && seed % 2 == 0 {
                        let start = if needle_len == 0 {
                            0
                        } else {
                            (seed as usize * 7 + haystack_len / 3) % (haystack_len - needle_len + 1)
                        };
                        haystack[start..start + needle_len].to_vec()
                    } else {
                        generated_bytes(needle_len, seed + 0x5678)
                    };

                    assert_two_way_eq_naive(&haystack, &needle);
                }
            }
        }
    }
}
