#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

const OP_T_THRES: usize = 16;

/// 逐字节比较两个等长或等公共长度的 slice。
///
/// 找到第一个不同字节后，返回：
///   a_byte - b_byte
///
/// 这和 C memcmp 的返回方向一致。
fn cmp_bytes(a: &[u8], b: &[u8]) -> Option<i32> {
    debug_assert_eq!(a.len(), b.len());
    for (&x, &y) in a.iter().zip(b.iter()) {
        if x != y {
            return Some(x as i32 - y as i32);
        }
    }
    None
}

/// 比较两个等长 slice。
///
/// 这是最接近 C memcmp(s1, s2, len) 的核心逻辑。
fn _memcmp(a: &[u8], b: &[u8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());

    let len = a.len();

    if len < OP_T_THRES {
        return cmp_bytes(a, b).unwrap_or(0);
    }

    let word_size = core::mem::size_of::<usize>();
    let word_bytes_len = len / word_size * word_size;

    let (a_words, a_tail) = a.split_at(word_bytes_len);
    let (b_words, b_tail) = b.split_at(word_bytes_len);

    // 按 usize 大小分块比较。
    //
    // 注意：
    // 这里只用 usize 判断“这一整块是否完全相同”。
    // 如果 word 不同，不能直接用 word 的大小决定 memcmp 结果，
    // 因为小端机器上整数大小不等于内存字典序。
    //
    // 所以 word 不同后，必须回到字节级别找第一个不同字节。
    for (a_chunk, b_chunk) in a_words
        .chunks_exact(word_size)
        .zip(b_words.chunks_exact(word_size))
    {
        let aw = usize::from_ne_bytes(
            a_chunk
                .try_into()
                .expect("chunks_exact guarantees usize-sized chunks"),
        );

        let bw = usize::from_ne_bytes(
            b_chunk
                .try_into()
                .expect("chunks_exact guarantees usize-sized chunks"),
        );

        if aw != bw {
            return cmp_bytes(a_chunk, b_chunk)
                .expect("different words must contain a different byte");
        }
    }

    // 比较尾部不足一个 usize 的字节。
    cmp_bytes(a_tail, b_tail).unwrap_or(0)
}

#[target_feature(enable = "sse2")]
unsafe fn cmp_vec16_at(pa: *const u8, pb: *const u8, offset: usize) -> Option<i32> {
    let va = _mm_loadu_si128(pa.add(offset) as *const __m128i);
    let vb = _mm_loadu_si128(pb.add(offset) as *const __m128i);

    let eq = _mm_cmpeq_epi8(va, vb);
    let mask = _mm_movemask_epi8(eq) as u32;

    // mask 的 16 个 bit 都是 1，说明这 16 个字节完全相等。
    let diff_mask = (!mask) & 0xffff;

    if diff_mask == 0 {
        return None;
    }

    // trailing_zeros 等价于 glibc 里的 bsfl。
    let first_diff = diff_mask.trailing_zeros() as usize;
    let idx = offset + first_diff;

    let x = *pa.add(idx);
    let y = *pb.add(idx);

    Some(x as i32 - y as i32)
}

#[target_feature(enable = "sse2")]
#[inline(always)]
unsafe fn cmp_vec64_at(pa: *const u8, pb: *const u8, offset: usize) -> Option<i32> {
    let a0 = _mm_loadu_si128(pa.add(offset) as *const __m128i);
    let b0 = _mm_loadu_si128(pb.add(offset) as *const __m128i);

    let a1 = _mm_loadu_si128(pa.add(offset + 16) as *const __m128i);
    let b1 = _mm_loadu_si128(pb.add(offset + 16) as *const __m128i);

    let a2 = _mm_loadu_si128(pa.add(offset + 32) as *const __m128i);
    let b2 = _mm_loadu_si128(pb.add(offset + 32) as *const __m128i);

    let a3 = _mm_loadu_si128(pa.add(offset + 48) as *const __m128i);
    let b3 = _mm_loadu_si128(pb.add(offset + 48) as *const __m128i);

    let e0 = _mm_cmpeq_epi8(a0, b0);
    let e1 = _mm_cmpeq_epi8(a1, b1);
    let e2 = _mm_cmpeq_epi8(a2, b2);
    let e3 = _mm_cmpeq_epi8(a3, b3);

    // glibc loop_4x 的核心思想：
    // 先把 4 个比较结果按位与起来，只用一次 movemask 判断 64 字节是否全相等。
    let e01 = _mm_and_si128(e0, e1);
    let e23 = _mm_and_si128(e2, e3);
    let all = _mm_and_si128(e01, e23);

    if _mm_movemask_epi8(all) == 0xffff {
        return None;
    }

    diff_from_4_eq_masks(pa, pb, offset, e0, e1, e2, e3)
}

#[target_feature(enable = "sse2")]
#[inline(always)]
unsafe fn diff_from_4_eq_masks(
    pa: *const u8,
    pb: *const u8,
    offset: usize,
    e0: __m128i,
    e1: __m128i,
    e2: __m128i,
    e3: __m128i,
) -> Option<i32> {
    let m0 = _mm_movemask_epi8(e0) as u64;
    let m1 = _mm_movemask_epi8(e1) as u64;
    let m2 = _mm_movemask_epi8(e2) as u64;
    let m3 = _mm_movemask_epi8(e3) as u64;

    // 类似 glibc ret_nonzero_loop：把 4 个 16-bit mismatch mask 合成
    // 一个 64-bit mask，再用 trailing_zeros 找 64B 中第一个不同字节。
    let diff_mask = ((!m0) & 0xffff)
        | (((!m1) & 0xffff) << 16)
        | (((!m2) & 0xffff) << 32)
        | (((!m3) & 0xffff) << 48);

    if diff_mask == 0 {
        return None;
    }
    let first_diff = diff_mask.trailing_zeros() as usize;
    let idx = offset + first_diff;
    Some(*pa.add(idx) as i32 - *pb.add(idx) as i32)
}

#[target_feature(enable = "sse2")]
unsafe fn _memcmp_sse2(a: &[u8], b: &[u8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());

    let len = a.len();

    if len == 0 {
        return 0;
    }

    if len < 16 {
        return cmp_bytes(a, b).unwrap_or(0);
    }
    let pa = a.as_ptr();
    let pb = b.as_ptr();
    // 先比较开头 16 字节。
    if let Some(diff) = cmp_vec16_at(pa, pb, 0) {
        return diff;
    }
    // 17..=32 字节：比较最后 16 字节。
    if len <= 32 {
        return cmp_vec16_at(pa, pb, len - 16).unwrap_or(0);
    }
    let mut i = 16usize;
    // 大循环：每次 64 字节。
    //
    // 原来 64 字节需要 4 次 movemask。
    // 现在 64 字节正常相等路径只需要 1 次 movemask。
    while i + 64 <= len {
        if let Some(diff) = cmp_vec64_at(pa, pb, i) {
            return diff;
        }

        i += 64;
    }
    // 剩余完整 16 字节块。
    while i + 16 <= len {
        if let Some(diff) = cmp_vec16_at(pa, pb, i) {
            return diff;
        }

        i += 16;
    }
    // 最后不足 16 字节时，用最后 16 字节 overlapping compare。
    if i < len {
        return cmp_vec16_at(pa, pb, len - 16).unwrap_or(0);
    }

    0
}

#[target_feature(enable = "avx2")]
unsafe fn cmp_vec32_at(pa: *const u8, pb: *const u8, offset: usize) -> Option<i32> {
    let va = _mm256_loadu_si256(pa.add(offset) as *const __m256i);
    let vb = _mm256_loadu_si256(pb.add(offset) as *const __m256i);

    let eq = _mm256_cmpeq_epi8(va, vb);
    let mask = _mm256_movemask_epi8(eq) as u32;
    let diff_mask = !mask;

    if diff_mask == 0 {
        return None;
    }

    let first_diff = diff_mask.trailing_zeros() as usize;
    let idx = offset + first_diff;

    Some(*pa.add(idx) as i32 - *pb.add(idx) as i32)
}

#[target_feature(enable = "avx2")]
#[inline(always)]
unsafe fn cmp_vec128_at(pa: *const u8, pb: *const u8, offset: usize) -> Option<i32> {
    let a0 = _mm256_loadu_si256(pa.add(offset) as *const __m256i);
    let b0 = _mm256_loadu_si256(pb.add(offset) as *const __m256i);

    let a1 = _mm256_loadu_si256(pa.add(offset + 32) as *const __m256i);
    let b1 = _mm256_loadu_si256(pb.add(offset + 32) as *const __m256i);

    let a2 = _mm256_loadu_si256(pa.add(offset + 64) as *const __m256i);
    let b2 = _mm256_loadu_si256(pb.add(offset + 64) as *const __m256i);

    let a3 = _mm256_loadu_si256(pa.add(offset + 96) as *const __m256i);
    let b3 = _mm256_loadu_si256(pb.add(offset + 96) as *const __m256i);

    let e0 = _mm256_cmpeq_epi8(a0, b0);
    let e1 = _mm256_cmpeq_epi8(a1, b1);
    let e2 = _mm256_cmpeq_epi8(a2, b2);
    let e3 = _mm256_cmpeq_epi8(a3, b3);

    // 对应 glibc loop_4x_vec：4 个 ymm 比较结果先 vpand 归约，
    // 热路径只用一次 vpmovmskb 判断这 128 字节是否全相等。
    let e01 = _mm256_and_si256(e0, e1);
    let e23 = _mm256_and_si256(e2, e3);
    let all = _mm256_and_si256(e01, e23);

    if _mm256_movemask_epi8(all) as u32 == u32::MAX {
        return None;
    }

    diff_from_4_ymm_eq_masks(pa, pb, offset, e0, e1, e2, e3)
}

#[target_feature(enable = "avx2")]
#[inline(always)]
unsafe fn diff_from_4_ymm_eq_masks(
    pa: *const u8,
    pb: *const u8,
    offset: usize,
    e0: __m256i,
    e1: __m256i,
    e2: __m256i,
    e3: __m256i,
) -> Option<i32> {
    let m0 = _mm256_movemask_epi8(e0) as u32;
    let m1 = _mm256_movemask_epi8(e1) as u32;
    let m2 = _mm256_movemask_epi8(e2) as u32;
    let m3 = _mm256_movemask_epi8(e3) as u32;

    let diff_mask =
        (!m0 as u128) | ((!m1 as u128) << 32) | ((!m2 as u128) << 64) | ((!m3 as u128) << 96);

    if diff_mask == 0 {
        return None;
    }

    let first_diff = diff_mask.trailing_zeros() as usize;
    let idx = offset + first_diff;

    Some(*pa.add(idx) as i32 - *pb.add(idx) as i32)
}

#[target_feature(enable = "avx2")]
unsafe fn _memcmp_avx2(a: &[u8], b: &[u8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());
    let len = a.len();
    if len == 0 {
        return 0;
    }
    // glibc 对 len < 32 会在确认不跨页时做 32B 预读，否则走 movbe
    // 小长度路径。Rust slice 不能合法越界读取，因此这里复用安全的短路径。
    if len < 32 {
        return _memcmp_sse2(a, b);
    }
    let pa = a.as_ptr();
    let pb = b.as_ptr();
    // 对应 glibc 入口处 first VEC。
    if let Some(diff) = cmp_vec32_at(pa, pb, 0) {
        return diff;
    }
    if len <= 64 {
        return cmp_vec32_at(pa, pb, len - 32).unwrap_or(0);
    }
    // 对应 glibc “Check second VEC no matter what.”
    if let Some(diff) = cmp_vec32_at(pa, pb, 32) {
        return diff;
    }
    if len <= 128 {
        if let Some(diff) = cmp_vec32_at(pa, pb, len - 64) {
            return diff;
        }

        return cmp_vec32_at(pa, pb, len - 32).unwrap_or(0);
    }
    // 对应 glibc third/fourth VEC optimistic compare。
    if let Some(diff) = cmp_vec32_at(pa, pb, 64) {
        return diff;
    }
    if let Some(diff) = cmp_vec32_at(pa, pb, 96) {
        return diff;
    }
    if len <= 256 {
        return cmp_vec128_at(pa, pb, len - 128).unwrap_or(0);
    }
    let mut i = 128usize;
    // 对应 glibc loop_4x_vec：每轮 4 * VEC_SIZE = 128 字节。
    while i + 128 <= len {
        if let Some(diff) = cmp_vec128_at(pa, pb, i) {
            return diff;
        }

        i += 128;
    }

    // 对应 glibc tail：用最后 4 个 VEC 做 overlapping compare。
    if i < len {
        return cmp_vec128_at(pa, pb, len - 128).unwrap_or(0);
    }

    0
}

fn memcmp_same_len_auto(a: &[u8], b: &[u8]) -> i32 {
    debug_assert_eq!(a.len(), b.len());
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if is_x86_feature_detected!("avx2") {
            return unsafe { _memcmp_avx2(a, b) };
        }
        if is_x86_feature_detected!("sse2") {
            return unsafe { _memcmp_sse2(a, b) };
        }
    }
    _memcmp(a, b)
}

pub fn memcmp(a: &[u8], b: &[u8]) -> i32 {
    let common_len = core::cmp::min(a.len(), b.len());

    let diff = memcmp_same_len_auto(&a[..common_len], &b[..common_len]);
    if diff != 0 {
        return diff;
    }

    // 共同前缀完全相同，则较短的 slice 更小。
    match a.len().cmp(&b.len()) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_memcmp(a: &[u8], b: &[u8]) -> i32 {
        for (&x, &y) in a.iter().zip(b.iter()) {
            if x != y {
                return x as i32 - y as i32;
            }
        }

        match a.len().cmp(&b.len()) {
            core::cmp::Ordering::Less => -1,
            core::cmp::Ordering::Equal => 0,
            core::cmp::Ordering::Greater => 1,
        }
    }

    fn assert_memcmp(a: &[u8], b: &[u8]) {
        assert_eq!(memcmp(a, b), expected_memcmp(a, b), "a={a:?}, b={b:?}",);
    }

    fn assert_memcmp_same_len(a: &[u8], b: &[u8]) {
        debug_assert_eq!(a.len(), b.len());
        assert_eq!(_memcmp(a, b), expected_memcmp(a, b), "a={a:?}, b={b:?}",);
    }

    fn assert_memcmp_sse2(a: &[u8], b: &[u8]) {
        debug_assert_eq!(a.len(), b.len());

        if !is_x86_feature_detected!("sse2") {
            return;
        }

        assert_eq!(
            unsafe { _memcmp_sse2(a, b) },
            expected_memcmp(a, b),
            "a={a:?}, b={b:?}",
        );
    }

    fn assert_memcmp_avx2(a: &[u8], b: &[u8]) {
        debug_assert_eq!(a.len(), b.len());

        if !is_x86_feature_detected!("avx2") {
            return;
        }

        assert_eq!(
            unsafe { _memcmp_avx2(a, b) },
            expected_memcmp(a, b),
            "a={a:?}, b={b:?}",
        );
    }

    #[test]
    fn equal_slices_return_zero() {
        assert_memcmp(b"", b"");
        assert_memcmp(b"abc", b"abc");
        assert_memcmp(&[0, 1, 2, 3, 255], &[0, 1, 2, 3, 255]);
    }

    #[test]
    fn first_different_byte_decides_sign_and_difference() {
        assert_eq!(memcmp(b"abc", b"abd"), b'c' as i32 - b'd' as i32);
        assert_eq!(memcmp(b"abd", b"abc"), b'd' as i32 - b'c' as i32);
        assert_eq!(memcmp(&[1, 2, 200], &[1, 2, 100]), 100);
        assert_eq!(memcmp(&[1, 2, 100], &[1, 2, 200]), -100);
    }

    #[test]
    fn nul_bytes_are_compared_as_regular_bytes() {
        assert_memcmp(&[1, 0, 2], &[1, 0, 2]);
        assert_memcmp(&[1, 0, 2], &[1, 0, 3]);
        assert_memcmp(&[1, 0, 255], &[1, 1, 0]);
    }

    #[test]
    fn common_prefix_uses_length_as_tiebreaker() {
        assert_eq!(memcmp(b"abc", b"abcd"), -1);
        assert_eq!(memcmp(b"abcd", b"abc"), 1);
        assert_eq!(memcmp(b"", b"x"), -1);
        assert_eq!(memcmp(b"x", b""), 1);
    }

    #[test]
    fn mismatches_after_word_sized_prefix_are_found() {
        let mut a = vec![0u8; 128];
        let mut b = vec![0u8; 128];

        for mismatch_index in [16usize, 31, 32, 63, 64, 100, 127] {
            a.fill(0);
            b.fill(0);
            a[mismatch_index] = 10;
            b[mismatch_index] = 20;
            assert_memcmp_same_len(&a, &b);

            a[mismatch_index] = 250;
            b[mismatch_index] = 1;
            assert_memcmp_same_len(&a, &b);
        }
    }

    #[test]
    fn generated_cases_match_reference_comparator() {
        fn generated(len: usize, seed: u64) -> Vec<u8> {
            let mut x = seed | 1;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                out.push((x >> 32) as u8);
            }
            out
        }

        for a_len in 0..80 {
            for b_len in 0..80 {
                let a = generated(a_len, a_len as u64 * 17 + 3);
                let mut b = generated(b_len, b_len as u64 * 31 + 9);

                let common = a_len.min(b_len);
                for i in 0..common.min(24) {
                    b[i] = a[i];
                }

                assert_memcmp(&a, &b);
            }
        }
    }

    #[test]
    fn memcmp_sse2_matches_reference_for_all_mismatch_positions() {
        for len in 0..=256 {
            let a = vec![0u8; len];
            let b = vec![0u8; len];
            assert_memcmp_sse2(&a, &b);

            for mismatch_index in 0..len {
                let mut a = vec![0u8; len];
                let mut b = vec![0u8; len];

                a[mismatch_index] = 1;
                b[mismatch_index] = 2;
                assert_memcmp_sse2(&a, &b);

                a[mismatch_index] = 250;
                b[mismatch_index] = 10;
                assert_memcmp_sse2(&a, &b);
            }
        }
    }

    #[test]
    fn memcmp_sse2_matches_reference_for_generated_same_len_data() {
        fn generated(len: usize, seed: u64) -> Vec<u8> {
            let mut x = seed | 1;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                out.push((x >> 32) as u8);
            }
            out
        }

        for len in 0..512 {
            let a = generated(len, len as u64 * 17 + 3);
            let b = generated(len, len as u64 * 31 + 9);
            assert_memcmp_sse2(&a, &b);

            let mut equal = a.clone();
            assert_memcmp_sse2(&a, &equal);

            if len > 0 {
                equal[len - 1] ^= 0x80;
                assert_memcmp_sse2(&a, &equal);
            }
        }
    }

    #[test]
    fn memcmp_avx2_matches_reference_for_all_mismatch_positions() {
        for len in 0..=512 {
            let a = vec![0u8; len];
            let b = vec![0u8; len];
            assert_memcmp_avx2(&a, &b);

            for mismatch_index in 0..len {
                let mut a = vec![0u8; len];
                let mut b = vec![0u8; len];

                a[mismatch_index] = 1;
                b[mismatch_index] = 2;
                assert_memcmp_avx2(&a, &b);

                a[mismatch_index] = 250;
                b[mismatch_index] = 10;
                assert_memcmp_avx2(&a, &b);
            }
        }
    }

    #[test]
    fn memcmp_avx2_matches_reference_for_generated_same_len_data() {
        fn generated(len: usize, seed: u64) -> Vec<u8> {
            let mut x = seed | 1;
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                out.push((x >> 32) as u8);
            }
            out
        }

        for len in 0..1024 {
            let a = generated(len, len as u64 * 17 + 3);
            let b = generated(len, len as u64 * 31 + 9);
            assert_memcmp_avx2(&a, &b);

            let mut equal = a.clone();
            assert_memcmp_avx2(&a, &equal);

            if len > 0 {
                equal[len - 1] ^= 0x80;
                assert_memcmp_avx2(&a, &equal);
            }
        }
    }

    #[test]
    #[ignore]
    fn bench_memcmp_sse2_vs_memcmp() {
        use std::hint::black_box;
        use std::time::Instant;

        if !is_x86_feature_detected!("sse2") {
            eprintln!("skip: current CPU does not support SSE2");
            return;
        }

        let len = 16 * 1024 * 1024;
        let rounds = 128;
        let mut a = vec![0u8; len];
        let mut b = vec![0u8; len];

        for i in 0..len {
            let v = (i as u8).wrapping_mul(31).wrapping_add(7);
            a[i] = v;
            b[i] = v;
        }

        // 把 mismatch 放在最后，强制两个实现扫描完整 buffer。
        a[len - 1] = 1;
        b[len - 1] = 2;

        let expected = _memcmp(&a, &b);
        assert_eq!(unsafe { _memcmp_sse2(&a, &b) }, expected);

        let start = Instant::now();
        let mut scalar_sum = 0i32;
        for _ in 0..rounds {
            scalar_sum = scalar_sum.wrapping_add(black_box(_memcmp(black_box(&a), black_box(&b))));
        }
        let scalar_elapsed = start.elapsed();

        let start = Instant::now();
        let mut sse2_sum = 0i32;
        for _ in 0..rounds {
            sse2_sum = sse2_sum.wrapping_add(black_box(unsafe {
                _memcmp_sse2(black_box(&a), black_box(&b))
            }));
        }
        let sse2_elapsed = start.elapsed();

        let total_gib = (len as f64 * rounds as f64) / 1024.0 / 1024.0 / 1024.0;
        let scalar_gib_s = total_gib / scalar_elapsed.as_secs_f64();
        let sse2_gib_s = total_gib / sse2_elapsed.as_secs_f64();

        println!(
            "_memcmp     : {:?} ({:.2} GiB/s)",
            scalar_elapsed, scalar_gib_s
        );
        println!("_memcmp_sse2: {:?} ({:.2} GiB/s)", sse2_elapsed, sse2_gib_s);
        println!(
            "speedup     : {:.2}x",
            scalar_elapsed.as_secs_f64() / sse2_elapsed.as_secs_f64()
        );

        assert_eq!(scalar_sum, sse2_sum);
    }

    #[test]
    #[ignore]
    fn bench_memcmp_avx2_vs_sse2_vs_memcmp() {
        use std::hint::black_box;
        use std::time::Instant;

        if !is_x86_feature_detected!("sse2") {
            eprintln!("skip: current CPU does not support SSE2");
            return;
        }

        let has_avx2 = is_x86_feature_detected!("avx2");
        let len = 16 * 1024 * 1024;
        let rounds = 128;
        let mut a = vec![0u8; len];
        let mut b = vec![0u8; len];

        for i in 0..len {
            let v = (i as u8).wrapping_mul(31).wrapping_add(7);
            a[i] = v;
            b[i] = v;
        }

        // 把 mismatch 放在最后，强制三个实现扫描完整 buffer。
        a[len - 1] = 1;
        b[len - 1] = 2;

        let expected = _memcmp(&a, &b);
        assert_eq!(unsafe { _memcmp_sse2(&a, &b) }, expected);
        if has_avx2 {
            assert_eq!(unsafe { _memcmp_avx2(&a, &b) }, expected);
        }

        let start = Instant::now();
        let mut scalar_sum = 0i32;
        for _ in 0..rounds {
            scalar_sum = scalar_sum.wrapping_add(black_box(_memcmp(black_box(&a), black_box(&b))));
        }
        let scalar_elapsed = start.elapsed();

        let start = Instant::now();
        let mut sse2_sum = 0i32;
        for _ in 0..rounds {
            sse2_sum = sse2_sum.wrapping_add(black_box(unsafe {
                _memcmp_sse2(black_box(&a), black_box(&b))
            }));
        }
        let sse2_elapsed = start.elapsed();

        let avx2_elapsed = if has_avx2 {
            let start = Instant::now();
            let mut avx2_sum = 0i32;
            for _ in 0..rounds {
                avx2_sum = avx2_sum.wrapping_add(black_box(unsafe {
                    _memcmp_avx2(black_box(&a), black_box(&b))
                }));
            }
            assert_eq!(scalar_sum, avx2_sum);
            Some(start.elapsed())
        } else {
            None
        };

        let total_gib = (len as f64 * rounds as f64) / 1024.0 / 1024.0 / 1024.0;
        let scalar_gib_s = total_gib / scalar_elapsed.as_secs_f64();
        let sse2_gib_s = total_gib / sse2_elapsed.as_secs_f64();

        println!(
            "_memcmp     : {:?} ({:.2} GiB/s)",
            scalar_elapsed, scalar_gib_s
        );
        println!("_memcmp_sse2: {:?} ({:.2} GiB/s)", sse2_elapsed, sse2_gib_s);

        if let Some(avx2_elapsed) = avx2_elapsed {
            let avx2_gib_s = total_gib / avx2_elapsed.as_secs_f64();
            println!("_memcmp_avx2: {:?} ({:.2} GiB/s)", avx2_elapsed, avx2_gib_s);
            println!(
                "avx2/scalar : {:.2}x",
                scalar_elapsed.as_secs_f64() / avx2_elapsed.as_secs_f64()
            );
            println!(
                "avx2/sse2   : {:.2}x",
                sse2_elapsed.as_secs_f64() / avx2_elapsed.as_secs_f64()
            );
        } else {
            println!("_memcmp_avx2: skipped, current CPU does not support AVX2");
        }

        assert_eq!(scalar_sum, sse2_sum);
    }
}
