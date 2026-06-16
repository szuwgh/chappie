#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

//ptr 这个地址是不是 align 的整数倍
#[inline]
fn is_aligned(ptr: *const u8, align: usize) -> bool {
    debug_assert!(align.is_power_of_two());
    (ptr as usize) & (align - 1) == 0
}

#[inline]
fn repeat_byte(byte: u8) -> usize {
    // 把一个 byte 复制到 usize 的每个字节里。
    //
    // 例如 64-bit:
    //   0xAB -> 0xABAB_ABAB_ABAB_ABAB
    //
    // 例如 32-bit:
    //   0xAB -> 0xABAB_ABAB
    let ones = usize::MAX / 0xFF;
    ones * byte as usize
}

#[inline]
fn zero_byte_mask(x: usize) -> usize {
    let lo = usize::MAX / 0xFF;
    let hi = lo << 7;

    x.wrapping_sub(lo) & !x & hi
}

#[inline]
fn first_zero_byte_index(mask: usize) -> usize {
    debug_assert!(mask != 0);

    #[cfg(target_endian = "little")]
    {
        mask.trailing_zeros() as usize / 8
    }

    #[cfg(target_endian = "big")]
    {
        mask.leading_zeros() as usize / 8
    }
}

pub fn memchr(s: &[u8], c: u8) -> Option<usize> {
    // 小数据不值得进 SIMD。AVX2/EVEX 对齐和 dispatch 都有成本。
    if s.len() < 64 {
        return _memchr(s, c);
    }

    #[cfg(target_arch = "x86_64")]
    {
        // EVEX256 需要 AVX512BW + AVX512VL。
        // 注意：是否真的比 AVX2 快，要看机器和 benchmark。
        if s.len() >= 256
            && std::is_x86_feature_detected!("avx512bw")
            && std::is_x86_feature_detected!("avx512vl")
        {
            return unsafe { _memchr_evex256(s, c) };
        }

        if s.len() >= 128 && std::is_x86_feature_detected!("avx2") {
            return unsafe { _memchr_avx2(s, c) };
        }

        if std::is_x86_feature_detected!("sse2") {
            return unsafe { _memchr_sse2(s, c) };
        }
    }

    _memchr(s, c)
}

/*
* 不是逐字节扫，而是按机器字批量扫
*/
fn _memchr(s: &[u8], c: u8) -> Option<usize> {
    if s.len() == 0 {
        return None;
    }
    let word_size = core::mem::size_of::<usize>();
    let repeated = repeat_byte(c);

    let ptr = s.as_ptr();
    let len = s.len();

    let mut i = 0usize;
    // 1. 先逐字节扫描到 usize 对齐位置
    while i < len && !is_aligned(unsafe { ptr.add(i) }, word_size) {
        if s[i] == c {
            return Some(i);
        }
        i += 1;
    }
    // 2. 按 machine word 扫描
    while i + word_size <= len {
        let word = unsafe {
            // 这里当前指针已经对齐，因此可以 read 一个 usize。
            // 但为了更保守，也可以用 read_unaligned。
            (ptr.add(i) as *const usize).read()
        };

        let x = word ^ repeated;

        let mask = zero_byte_mask(x);

        if mask != 0 {
            return Some(i + first_zero_byte_index(mask));
        }

        i += word_size;
    }

    // 4. 处理尾部不足一个 word 的字节
    while i < len {
        if s[i] == c {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn _memchr_sse2(s: &[u8], c: u8) -> Option<usize> {
    let ptr = s.as_ptr();
    let len = s.len();

    let target = _mm_set1_epi8(c as i8);

    let mut i = 0usize;

    // 1. 先逐字节扫到 16-byte 对齐位置。
    while i < len && ((ptr.add(i) as usize) & 15) != 0 {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }

    // 2. 主循环：一次处理 64 bytes，也就是 4 个 xmm。
    while i + 64 <= len {
        let p0 = ptr.add(i) as *const __m128i;
        let p1 = ptr.add(i + 16) as *const __m128i;
        let p2 = ptr.add(i + 32) as *const __m128i;
        let p3 = ptr.add(i + 48) as *const __m128i;

        let b0 = _mm_load_si128(p0);
        let b1 = _mm_load_si128(p1);
        let b2 = _mm_load_si128(p2);
        let b3 = _mm_load_si128(p3);

        let e0 = _mm_cmpeq_epi8(b0, target);
        let e1 = _mm_cmpeq_epi8(b1, target);
        let e2 = _mm_cmpeq_epi8(b2, target);
        let e3 = _mm_cmpeq_epi8(b3, target);

        // glibc 用 pmaxub 聚合，这里用 OR，效果等价：
        // compare 结果只有 0x00 或 0xff。
        let a01 = _mm_or_si128(e0, e1);
        let a23 = _mm_or_si128(e2, e3);
        let any = _mm_or_si128(a01, a23);

        let any_mask = _mm_movemask_epi8(any) as u32;

        if any_mask != 0 {
            let m0 = _mm_movemask_epi8(e0) as u32;
            if m0 != 0 {
                return Some(i + m0.trailing_zeros() as usize);
            }

            let m1 = _mm_movemask_epi8(e1) as u32;
            if m1 != 0 {
                return Some(i + 16 + m1.trailing_zeros() as usize);
            }

            let m2 = _mm_movemask_epi8(e2) as u32;
            if m2 != 0 {
                return Some(i + 32 + m2.trailing_zeros() as usize);
            }

            let m3 = _mm_movemask_epi8(e3) as u32;
            debug_assert!(m3 != 0);
            return Some(i + 48 + m3.trailing_zeros() as usize);
        }

        i += 64;
    }

    // 3. 剩余完整 16-byte block。
    while i + 16 <= len {
        let block = _mm_load_si128(ptr.add(i) as *const __m128i);
        let eq = _mm_cmpeq_epi8(block, target);
        let mask = _mm_movemask_epi8(eq) as u32;

        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }

        i += 16;
    }

    // 4. 尾部不足 16 bytes。
    while i < len {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }

    None
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn _memchr_avx2(s: &[u8], c: u8) -> Option<usize> {
    let ptr = s.as_ptr();
    let len = s.len();

    let target = _mm256_set1_epi8(c as i8);
    let mut i = 0usize;

    // 先扫到 32-byte 对齐位置，后面才能安全使用 aligned AVX2 load。
    while i < len && ((ptr.add(i) as usize) & 31) != 0 {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }
    while i + 128 <= len {
        let p0 = ptr.add(i) as *const __m256i;
        let p1 = ptr.add(i + 32) as *const __m256i;
        let p2 = ptr.add(i + 64) as *const __m256i;
        let p3 = ptr.add(i + 96) as *const __m256i;

        let b0 = _mm256_load_si256(p0);
        let b1 = _mm256_load_si256(p1);
        let b2 = _mm256_load_si256(p2);
        let b3 = _mm256_load_si256(p3);

        let e0 = _mm256_cmpeq_epi8(b0, target);
        let e1 = _mm256_cmpeq_epi8(b1, target);
        let e2 = _mm256_cmpeq_epi8(b2, target);
        let e3 = _mm256_cmpeq_epi8(b3, target);

        let a01 = _mm256_or_si256(e0, e1);
        let a23 = _mm256_or_si256(e2, e3);
        let any = _mm256_or_si256(a01, a23);

        let any_mask = _mm256_movemask_epi8(any) as u32;

        if any_mask != 0 {
            let m0 = _mm256_movemask_epi8(e0) as u32;
            if m0 != 0 {
                return Some(i + m0.trailing_zeros() as usize);
            }

            let m1 = _mm256_movemask_epi8(e1) as u32;
            if m1 != 0 {
                return Some(i + 32 + m1.trailing_zeros() as usize);
            }

            let m2 = _mm256_movemask_epi8(e2) as u32;
            if m2 != 0 {
                return Some(i + 64 + m2.trailing_zeros() as usize);
            }

            let m3 = _mm256_movemask_epi8(e3) as u32;
            debug_assert!(m3 != 0);
            return Some(i + 96 + m3.trailing_zeros() as usize);
        }

        i += 128;
    }

    while i + 32 <= len {
        let block = _mm256_load_si256(ptr.add(i) as *const __m256i);
        let eq = _mm256_cmpeq_epi8(block, target);
        let mask = _mm256_movemask_epi8(eq) as u32;

        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }

        i += 32;
    }

    while i < len {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }

    None
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512bw")]
#[target_feature(enable = "avx512vl")]
unsafe fn _memchr_evex256(s: &[u8], c: u8) -> Option<usize> {
    let ptr = s.as_ptr();
    let len = s.len();

    let target = _mm256_set1_epi8(c as i8);
    let mut i = 0usize;

    // 先扫到 32-byte 对齐，后面才能用 aligned load。
    while i < len && ((ptr.add(i) as usize) & 31) != 0 {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }

    while i + 128 <= len {
        let b0 = _mm256_load_si256(ptr.add(i) as *const __m256i);
        let b1 = _mm256_load_si256(ptr.add(i + 32) as *const __m256i);
        let b2 = _mm256_load_si256(ptr.add(i + 64) as *const __m256i);
        let b3 = _mm256_load_si256(ptr.add(i + 96) as *const __m256i);

        let m0 = _mm256_cmpeq_epi8_mask(b0, target) as u32;
        let m1 = _mm256_cmpeq_epi8_mask(b1, target) as u32;
        let m2 = _mm256_cmpeq_epi8_mask(b2, target) as u32;
        let m3 = _mm256_cmpeq_epi8_mask(b3, target) as u32;

        if (m0 | m1 | m2 | m3) != 0 {
            if m0 != 0 {
                return Some(i + m0.trailing_zeros() as usize);
            }
            if m1 != 0 {
                return Some(i + 32 + m1.trailing_zeros() as usize);
            }
            if m2 != 0 {
                return Some(i + 64 + m2.trailing_zeros() as usize);
            }

            debug_assert!(m3 != 0);
            return Some(i + 96 + m3.trailing_zeros() as usize);
        }

        i += 128;
    }

    while i + 32 <= len {
        let block = _mm256_load_si256(ptr.add(i) as *const __m256i);
        let mask = _mm256_cmpeq_epi8_mask(block, target) as u32;

        if mask != 0 {
            return Some(i + mask.trailing_zeros() as usize);
        }

        i += 32;
    }

    while i < len {
        if *ptr.add(i) == c {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::_memchr;

    #[cfg(target_arch = "x86_64")]
    use super::{_memchr_avx2, _memchr_evex256, _memchr_sse2};

    fn expected_memchr(s: &[u8], c: u8) -> Option<usize> {
        s.iter().position(|&b| b == c)
    }

    #[test]
    fn empty_slice_returns_none() {
        assert_eq!(_memchr(b"", b'a'), None);
    }

    #[test]
    fn finds_byte_at_start_middle_and_end() {
        assert_eq!(_memchr(b"hello", b'h'), Some(0));
        assert_eq!(_memchr(b"hello", b'l'), Some(2));
        assert_eq!(_memchr(b"hello", b'o'), Some(4));
    }

    #[test]
    fn returns_first_match() {
        assert_eq!(_memchr(b"banana", b'a'), Some(1));
        assert_eq!(_memchr(b"aaaa", b'a'), Some(0));
    }

    #[test]
    fn returns_none_when_missing() {
        assert_eq!(_memchr(b"hello", b'z'), None);
        assert_eq!(_memchr(&[1, 2, 3, 4], 0), None);
    }

    #[test]
    fn finds_zero_and_high_bytes() {
        assert_eq!(_memchr(&[1, 2, 0, 3], 0), Some(2));
        assert_eq!(_memchr(&[0x10, 0x80, 0xff, 0x7f], 0xff), Some(2));
    }

    #[test]
    fn handles_lengths_around_word_size() {
        let word = core::mem::size_of::<usize>();

        for len in 0..=(word * 3 + 3) {
            for hit in 0..len {
                let mut data = vec![b'a'; len];
                data[hit] = b'x';
                assert_eq!(_memchr(&data, b'x'), Some(hit), "len={len} hit={hit}");
            }

            let data = vec![b'a'; len];
            assert_eq!(_memchr(&data, b'x'), None, "len={len} missing");
        }
    }

    #[test]
    fn handles_unaligned_subslices() {
        let word = core::mem::size_of::<usize>();
        let mut backing = vec![b'a'; word * 4 + 16];

        for offset in 0..word {
            for len in 0..=(word * 2 + 3) {
                let start = offset;
                let end = start + len;
                let slice = &backing[start..end];
                assert_eq!(
                    _memchr(slice, b'x'),
                    None,
                    "offset={offset} len={len} missing"
                );

                if len > 0 {
                    let hit = len - 1;
                    backing[start + hit] = b'x';
                    let slice = &backing[start..end];
                    assert_eq!(
                        _memchr(slice, b'x'),
                        Some(hit),
                        "offset={offset} len={len} hit={hit}"
                    );
                    backing[start + hit] = b'a';
                }
            }
        }
    }

    #[test]
    fn matches_naive_scan_for_many_inputs() {
        let needles = [0, b'a', b'x', 0x7f, 0x80, 0xff];

        for len in 0..257usize {
            let mut data = Vec::with_capacity(len);
            for i in 0..len {
                data.push(((i * 37 + len * 11) & 0xff) as u8);
            }

            for &needle in &needles {
                assert_eq!(
                    _memchr(&data, needle),
                    expected_memchr(&data, needle),
                    "len={len} needle={needle}"
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn sse2_memchr(s: &[u8], c: u8) -> Option<usize> {
        if std::is_x86_feature_detected!("sse2") {
            unsafe { _memchr_sse2(s, c) }
        } else {
            expected_memchr(s, c)
        }
    }

    #[cfg(target_arch = "x86_64")]
    fn avx2_memchr(s: &[u8], c: u8) -> Option<usize> {
        assert!(std::is_x86_feature_detected!("avx2"));
        unsafe { _memchr_avx2(s, c) }
    }

    #[cfg(target_arch = "x86_64")]
    fn has_evex256() -> bool {
        std::is_x86_feature_detected!("avx512bw") && std::is_x86_feature_detected!("avx512vl")
    }

    #[cfg(target_arch = "x86_64")]
    fn evex256_memchr(s: &[u8], c: u8) -> Option<usize> {
        assert!(has_evex256());
        unsafe { _memchr_evex256(s, c) }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_matches_naive_for_basic_cases() {
        assert_eq!(sse2_memchr(b"", b'a'), None);
        assert_eq!(sse2_memchr(b"hello", b'h'), Some(0));
        assert_eq!(sse2_memchr(b"hello", b'l'), Some(2));
        assert_eq!(sse2_memchr(b"hello", b'o'), Some(4));
        assert_eq!(sse2_memchr(b"hello", b'z'), None);
        assert_eq!(sse2_memchr(&[1, 2, 0, 3], 0), Some(2));
        assert_eq!(sse2_memchr(&[0x10, 0x80, 0xff, 0x7f], 0xff), Some(2));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_handles_16_and_64_byte_boundaries() {
        for len in 0..=160usize {
            for hit in 0..len {
                let mut data = vec![b'a'; len];
                data[hit] = b'x';
                assert_eq!(sse2_memchr(&data, b'x'), Some(hit), "len={len} hit={hit}");
            }

            let data = vec![b'a'; len];
            assert_eq!(sse2_memchr(&data, b'x'), None, "len={len} missing");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_handles_unaligned_subslices() {
        let mut backing = vec![b'a'; 256];

        for offset in 0..32usize {
            for len in 0..=128usize {
                let start = offset;
                let end = start + len;
                let slice = &backing[start..end];
                assert_eq!(
                    sse2_memchr(slice, b'x'),
                    None,
                    "offset={offset} len={len} missing"
                );

                if len > 0 {
                    for &hit in &[0usize, len / 2, len - 1] {
                        backing[start + hit] = b'x';
                        let slice = &backing[start..end];
                        assert_eq!(
                            sse2_memchr(slice, b'x'),
                            Some(hit),
                            "offset={offset} len={len} hit={hit}"
                        );
                        backing[start + hit] = b'a';
                    }
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn sse2_matches_naive_scan_for_many_inputs() {
        let needles = [0, b'a', b'x', 0x7f, 0x80, 0xff];

        for len in 0..=512usize {
            let mut data = Vec::with_capacity(len);
            for i in 0..len {
                data.push(((i * 37 + len * 11) & 0xff) as u8);
            }

            for &needle in &needles {
                assert_eq!(
                    sse2_memchr(&data, needle),
                    expected_memchr(&data, needle),
                    "len={len} needle={needle}"
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matches_naive_for_basic_cases() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("avx2 not detected; skipping test");
            return;
        }

        assert_eq!(avx2_memchr(b"", b'a'), None);
        assert_eq!(avx2_memchr(b"hello", b'h'), Some(0));
        assert_eq!(avx2_memchr(b"hello", b'l'), Some(2));
        assert_eq!(avx2_memchr(b"hello", b'o'), Some(4));
        assert_eq!(avx2_memchr(b"hello", b'z'), None);
        assert_eq!(avx2_memchr(&[1, 2, 0, 3], 0), Some(2));
        assert_eq!(avx2_memchr(&[0x10, 0x80, 0xff, 0x7f], 0xff), Some(2));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_handles_32_and_128_byte_boundaries() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("avx2 not detected; skipping test");
            return;
        }

        for len in 0..=320usize {
            for hit in 0..len {
                let mut data = vec![b'a'; len];
                data[hit] = b'x';
                assert_eq!(avx2_memchr(&data, b'x'), Some(hit), "len={len} hit={hit}");
            }

            let data = vec![b'a'; len];
            assert_eq!(avx2_memchr(&data, b'x'), None, "len={len} missing");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_handles_unaligned_subslices() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("avx2 not detected; skipping test");
            return;
        }

        let mut backing = vec![b'a'; 512];

        for offset in 0..64usize {
            for len in 0..=256usize {
                let start = offset;
                let end = start + len;
                let slice = &backing[start..end];
                assert_eq!(
                    avx2_memchr(slice, b'x'),
                    None,
                    "offset={offset} len={len} missing"
                );

                if len > 0 {
                    for &hit in &[0usize, len / 2, len - 1] {
                        backing[start + hit] = b'x';
                        let slice = &backing[start..end];
                        assert_eq!(
                            avx2_memchr(slice, b'x'),
                            Some(hit),
                            "offset={offset} len={len} hit={hit}"
                        );
                        backing[start + hit] = b'a';
                    }
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_matches_naive_scan_for_many_inputs() {
        if !std::is_x86_feature_detected!("avx2") {
            eprintln!("avx2 not detected; skipping test");
            return;
        }

        let needles = [0, b'a', b'x', 0x7f, 0x80, 0xff];

        for len in 0..=1024usize {
            let mut data = Vec::with_capacity(len);
            for i in 0..len {
                data.push(((i * 37 + len * 11) & 0xff) as u8);
            }

            for &needle in &needles {
                assert_eq!(
                    avx2_memchr(&data, needle),
                    expected_memchr(&data, needle),
                    "len={len} needle={needle}"
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn evex256_matches_naive_for_basic_cases() {
        if !has_evex256() {
            eprintln!("evex256 not detected; skipping test");
            return;
        }

        assert_eq!(evex256_memchr(b"", b'a'), None);
        assert_eq!(evex256_memchr(b"hello", b'h'), Some(0));
        assert_eq!(evex256_memchr(b"hello", b'l'), Some(2));
        assert_eq!(evex256_memchr(b"hello", b'o'), Some(4));
        assert_eq!(evex256_memchr(b"hello", b'z'), None);
        assert_eq!(evex256_memchr(&[1, 2, 0, 3], 0), Some(2));
        assert_eq!(evex256_memchr(&[0x10, 0x80, 0xff, 0x7f], 0xff), Some(2));
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn evex256_handles_32_and_128_byte_boundaries() {
        if !has_evex256() {
            eprintln!("evex256 not detected; skipping test");
            return;
        }

        for len in 0..=320usize {
            for hit in 0..len {
                let mut data = vec![b'a'; len];
                data[hit] = b'x';
                assert_eq!(
                    evex256_memchr(&data, b'x'),
                    Some(hit),
                    "len={len} hit={hit}"
                );
            }

            let data = vec![b'a'; len];
            assert_eq!(evex256_memchr(&data, b'x'), None, "len={len} missing");
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn evex256_handles_unaligned_subslices() {
        if !has_evex256() {
            eprintln!("evex256 not detected; skipping test");
            return;
        }

        let mut backing = vec![b'a'; 512];

        for offset in 0..64usize {
            for len in 0..=256usize {
                let start = offset;
                let end = start + len;
                let slice = &backing[start..end];
                assert_eq!(
                    evex256_memchr(slice, b'x'),
                    None,
                    "offset={offset} len={len} missing"
                );

                if len > 0 {
                    for &hit in &[0usize, len / 2, len - 1] {
                        backing[start + hit] = b'x';
                        let slice = &backing[start..end];
                        assert_eq!(
                            evex256_memchr(slice, b'x'),
                            Some(hit),
                            "offset={offset} len={len} hit={hit}"
                        );
                        backing[start + hit] = b'a';
                    }
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn evex256_matches_naive_scan_for_many_inputs() {
        if !has_evex256() {
            eprintln!("evex256 not detected; skipping test");
            return;
        }

        let needles = [0, b'a', b'x', 0x7f, 0x80, 0xff];

        for len in 0..=1024usize {
            let mut data = Vec::with_capacity(len);
            for i in 0..len {
                data.push(((i * 37 + len * 11) & 0xff) as u8);
            }

            for &needle in &needles {
                assert_eq!(
                    evex256_memchr(&data, needle),
                    expected_memchr(&data, needle),
                    "len={len} needle={needle}"
                );
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    #[ignore = "performance comparison; run with `cargo test --release bench_memchr_sse2_vs_swar -- --ignored --nocapture`"]
    fn bench_memchr_sse2_vs_swar() {
        use std::hint::black_box;
        use std::time::Instant;

        if !std::is_x86_feature_detected!("sse2") {
            eprintln!("sse2 not detected; skipping benchmark");
            return;
        }

        const LEN: usize = 8 * 1024 * 1024;
        const ITERS: usize = 200;

        let mut data = vec![b'a'; LEN];
        data[LEN - 1] = b'x';

        assert_eq!(_memchr(&data, b'x'), Some(LEN - 1));
        assert_eq!(unsafe { _memchr_sse2(&data, b'x') }, Some(LEN - 1));
        if std::is_x86_feature_detected!("avx2") {
            assert_eq!(unsafe { _memchr_avx2(&data, b'x') }, Some(LEN - 1));
        }
        if has_evex256() {
            assert_eq!(unsafe { _memchr_evex256(&data, b'x') }, Some(LEN - 1));
        }

        let start = Instant::now();
        let mut swar_acc = 0usize;
        for _ in 0..ITERS {
            swar_acc ^= black_box(_memchr(black_box(&data), black_box(b'x')).unwrap());
        }
        let swar_elapsed = start.elapsed();

        let start = Instant::now();
        let mut sse2_acc = 0usize;
        for _ in 0..ITERS {
            sse2_acc ^=
                black_box(unsafe { _memchr_sse2(black_box(&data), black_box(b'x')).unwrap() });
        }
        let sse2_elapsed = start.elapsed();

        let avx2_elapsed = if std::is_x86_feature_detected!("avx2") {
            let start = Instant::now();
            let mut avx2_acc = 0usize;
            for _ in 0..ITERS {
                avx2_acc ^=
                    black_box(unsafe { _memchr_avx2(black_box(&data), black_box(b'x')).unwrap() });
            }
            black_box(avx2_acc);
            Some(start.elapsed())
        } else {
            eprintln!("avx2 not detected; skipping memchr_avx2 benchmark");
            None
        };

        let evex256_elapsed = if has_evex256() {
            let start = Instant::now();
            let mut evex256_acc = 0usize;
            for _ in 0..ITERS {
                evex256_acc ^= black_box(unsafe {
                    _memchr_evex256(black_box(&data), black_box(b'x')).unwrap()
                });
            }
            black_box(evex256_acc);
            Some(start.elapsed())
        } else {
            eprintln!("evex256 not detected; skipping memchr_evex256 benchmark");
            None
        };

        black_box((swar_acc, sse2_acc));

        let scanned_gib = (LEN as f64 * ITERS as f64) / (1024.0 * 1024.0 * 1024.0);
        let swar_gib_s = scanned_gib / swar_elapsed.as_secs_f64();
        let sse2_gib_s = scanned_gib / sse2_elapsed.as_secs_f64();
        let sse2_speedup = swar_elapsed.as_secs_f64() / sse2_elapsed.as_secs_f64();

        eprintln!("memchr     : {:?} ({:.2} GiB/s)", swar_elapsed, swar_gib_s);
        eprintln!("memchr_sse2: {:?} ({:.2} GiB/s)", sse2_elapsed, sse2_gib_s);
        eprintln!("sse2/swar  : {:.2}x", sse2_speedup);

        if let Some(avx2_elapsed) = avx2_elapsed {
            let avx2_gib_s = scanned_gib / avx2_elapsed.as_secs_f64();
            let avx2_vs_swar = swar_elapsed.as_secs_f64() / avx2_elapsed.as_secs_f64();
            let avx2_vs_sse2 = sse2_elapsed.as_secs_f64() / avx2_elapsed.as_secs_f64();

            eprintln!("memchr_avx2: {:?} ({:.2} GiB/s)", avx2_elapsed, avx2_gib_s);
            eprintln!("avx2/swar  : {:.2}x", avx2_vs_swar);
            eprintln!("avx2/sse2  : {:.2}x", avx2_vs_sse2);
        }

        if let Some(evex256_elapsed) = evex256_elapsed {
            let evex256_gib_s = scanned_gib / evex256_elapsed.as_secs_f64();
            let evex256_vs_swar = swar_elapsed.as_secs_f64() / evex256_elapsed.as_secs_f64();
            let evex256_vs_sse2 = sse2_elapsed.as_secs_f64() / evex256_elapsed.as_secs_f64();

            eprintln!(
                "memchr_evex256: {:?} ({:.2} GiB/s)",
                evex256_elapsed, evex256_gib_s
            );
            eprintln!("evex256/swar  : {:.2}x", evex256_vs_swar);
            eprintln!("evex256/sse2  : {:.2}x", evex256_vs_sse2);

            if let Some(avx2_elapsed) = avx2_elapsed {
                let evex256_vs_avx2 = avx2_elapsed.as_secs_f64() / evex256_elapsed.as_secs_f64();
                eprintln!("evex256/avx2  : {:.2}x", evex256_vs_avx2);
            }
        }
    }
}
