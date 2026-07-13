pub(crate) mod block;
pub(crate) mod edit;
pub(crate) mod edit_block;
pub(crate) mod hex;
pub(crate) mod text;
use crate::common::error::ChapResult;
use crate::common::gap_buffer::GapBuffer;
use crate::common::gap_buffer::GapBytes;
use crate::common::gap_buffer::GapBytesBlockCharIter;
use crate::common::gap_buffer::GapBytesBlockU8Iter;
use crate::common::gap_buffer::GapBytesCharIter;
use crate::common::gap_buffer::GapBytesIter;
use crate::common::ring_vec::RingVec;
use crate::common::util;
use crate::searcher::boyermoore::BoyerMoore;
use crate::searcher::memmem::{memmem, memmem_small_slices_no_alloc, memmem_two_slices_no_alloc};
use crate::textwarp::edit::GapText;
use crate::textwarp::edit_block::GapBlockText;
use crate::textwarp::hex::HexText;
use crate::textwarp::text::MmapText;
use mlua::Either;
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::fmt::Debug;
use std::fmt::Display;
use std::fs;
use std::iter;
use std::ops::Bound;
use std::path::Path;
use std::path::PathBuf;
use std::ptr::NonNull;
use unicode_width::UnicodeWidthChar;
use utf8_iter::Utf8CharIndices;
use utf8_iter::Utf8CharsEx;

const CHUNK_NUM: usize = 5;

macro_rules! get_page_number {
    ($line_number:expr, $lines_per_page:expr) => {{
        let line_num = $line_number;
        let lines_per = $lines_per_page;

        assert!(lines_per > 0, "每页行数必须大于，当前值: {}", lines_per);
        assert!(line_num >= 0, "行号不能为负数，当前值: {}", line_num);

        if line_num == 0 {
            0
        } else {
            (line_num - 1) / lines_per + 1
        }
    }};
}

#[derive(Debug, Clone)]
pub(crate) struct TextSelect(usize, usize);

impl TextSelect {
    pub(crate) fn new() -> Self {
        TextSelect(0, 0)
    }

    pub(crate) fn from_select(start: usize, end: usize) -> Self {
        TextSelect(start, end)
    }

    fn start(&self) -> usize {
        self.0
    }
    fn end(&self) -> usize {
        self.1
    }

    fn len(&self) -> usize {
        self.end() - self.start()
    }

    fn inc_end(&mut self) {
        self.1 += 1;
    }

    pub(crate) fn has_selected(&self) -> bool {
        self.start() < self.end()
    }

    pub(crate) fn is_selected(&self, pos: usize) -> bool {
        pos >= self.start() && pos <= self.end()
    }

    // 递减end
    fn dec_end(&mut self) {
        if self.1 > self.0 {
            self.1 -= 1;
        }
    }

    pub(crate) fn reset_to_start(&mut self) {
        self.1 = self.0;
    }

    pub(crate) fn set_pos(&mut self, pos: usize) {
        self.0 = pos;
        self.1 = pos;
    }

    pub(crate) fn get_start(&self) -> usize {
        self.0
    }

    pub(crate) fn get_end(&self) -> usize {
        self.1
    }

    pub(crate) fn set_start(&mut self, start: usize) {
        self.0 = start;
    }

    pub(crate) fn set_end(&mut self, end: usize) {
        self.1 = end;
    }

    pub(crate) fn set_select(&mut self, start: usize, end: usize) {
        self.0 = start;
        self.1 = end;
    }
}

#[derive(Clone, Copy)]
pub(crate) enum TextWarpType {
    NoWrap,
    SoftWrap,
}

// #[derive(Debug, Clone, Copy)]
// pub(crate) struct PageOffset {
//     line_index: usize,       //第多少行
//     line_offset: usize,      //行在总行的起始位置
//     block_num: usize,        //行所在块编号
//     block_line_index: usize, //行在块内的行号
//     block_offset: usize,     //行所在块偏移
//     line_file_start: usize,  //这一行在整个文件的起始位置
//     start_line_num: usize,   //开始的行数
//     start_page_num: usize,   //这一行在第几页开始
// }

// impl PageOffset {
//     pub(crate) fn new() -> Self {
//         PageOffset {
//             line_index: 0,
//             line_offset: 0,
//             block_num: 0,        //行所在块编号
//             block_line_index: 0, //行在块内的行号
//             block_offset: 0,     //行所在块偏移
//             line_file_start: 0,
//             start_line_num: 0,
//             start_page_num: 0,
//         }
//     }
// }

pub(crate) struct GapBytesCache {
    data: (NonNull<u8>, NonNull<u8>),
    len: (usize, usize),
}

impl GapBytesCache {
    fn from_data(s: GapBytes) -> Self {
        let ptr1 = s.left().as_ptr() as *const u8 as *mut u8; // 获取 &str 的指针
        let len1 = s.left().len(); // 获取 &str 的长度
        let non_null_ptr1 = unsafe { NonNull::new_unchecked(ptr1) }; // 创建 NonNull<str>

        let ptr2 = s.right().as_ptr() as *const u8 as *mut u8; // 获取 &str 的指针
        let len2 = s.right().len(); // 获取 &str 的长度
        let non_null_ptr2 = unsafe { NonNull::new_unchecked(ptr2) }; // 创建 NonNull<str>

        GapBytesCache {
            data: (non_null_ptr1, non_null_ptr2),
            len: (len1, len2),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len.0 + self.len.1
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_str(&self) -> (Cow<str>, Cow<str>) {
        // 将指针转换为 &[u8]，然后转换为 &str
        let slice1 = unsafe { std::slice::from_raw_parts(self.data.0.as_ptr(), self.len.0) };
        let slice2 = unsafe { std::slice::from_raw_parts(self.data.1.as_ptr(), self.len.1) };
        // 使用 String::from_utf8_lossy 处理 UTF-8 字节切片
        let str1 = String::from_utf8_lossy(slice1);
        let str2 = String::from_utf8_lossy(slice2);

        (str1, str2)
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_slice(&self) -> (&[u8], &[u8]) {
        // 将指针转换为 &[u8]，然后转换为 &str
        let slice1 = unsafe { std::slice::from_raw_parts(self.data.0.as_ptr(), self.len.0) };
        let slice2 = unsafe { std::slice::from_raw_parts(self.data.1.as_ptr(), self.len.1) };
        (slice1, slice2)
    }

    pub(crate) fn text(&self, range: &impl std::ops::RangeBounds<usize>) -> (&[u8], &[u8]) {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end,
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.len(),
        };
        if start > self.len() || end > self.len() {
            return (&[], &[]);
        }

        let (left, right) = self.as_slice();

        if start < left.len() {
            if end <= left.len() {
                (&left[start..end], &[])
            } else {
                (left, &right[..end - left.len()])
            }
        } else if right.len() > 0 {
            (&[], &right[start - left.len()..end - left.len()])
        } else {
            return (&[], &[]);
        }
    }

    pub(crate) fn to_gap_bytes(&self) -> GapBytes {
        // 将指针转换为 &[u8]，然后转换为 &str
        let slice1 = unsafe { std::slice::from_raw_parts(self.data.0.as_ptr(), self.len.0) };
        let slice2 = unsafe { std::slice::from_raw_parts(self.data.1.as_ptr(), self.len.1) };
        GapBytes::new(slice1, slice2)
    }
}

pub(crate) struct BytesCache {
    data: NonNull<u8>,
    len: usize,
}

impl BytesCache {
    fn from_slice(s: &[u8]) -> Self {
        let ptr = s.as_ptr() as *const u8 as *mut u8; // 获取 &str 的指针
        let len = s.len(); // 获取 &str 的长度
        let non_null_ptr = unsafe { NonNull::new_unchecked(ptr) }; // 创建 NonNull<str>

        BytesCache {
            data: non_null_ptr,
            len: len,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_str(&self) -> Cow<str> {
        // 将指针转换为 &[u8]，然后转换为 &str
        String::from_utf8_lossy(self.as_slice())
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_slice(&self) -> &[u8] {
        // 将指针转换为 &[u8]，然后转换为 &str
        let slice = unsafe { std::slice::from_raw_parts(self.data.as_ptr(), self.len) };
        slice
    }

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> &[u8] {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len(),
        };
        if start >= self.len() {
            return &[];
        }
        assert!(start <= end);
        &self.as_slice()[start..end]
    }
}

pub(crate) struct VecCache {
    data: Vec<u8>,
}

impl VecCache {
    fn from_vec(s: Vec<u8>) -> Self {
        VecCache { data: s }
    }

    pub(crate) fn len(&self) -> usize {
        self.data.len()
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_str(&self) -> Cow<str> {
        // 将指针转换为 &[u8]，然后转换为 &str
        String::from_utf8_lossy(self.as_slice())
    }

    // 从 CacheStr 获取 &str
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> &[u8] {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len(),
        };
        assert!(start <= end);
        &self.as_slice()[start..end]
    }
}

pub(crate) enum CacheStr {
    Vec(VecCache),
    Bytes(BytesCache),
    Gap(GapBytesCache),
    GapBlock(GapBytesCache, GapBytesCache),
}

impl CacheStr {
    fn from_data(s: LineData) -> Self {
        match s {
            LineData::Bytes(v) => CacheStr::Bytes(BytesCache::from_slice(v)),
            LineData::Own(v) => CacheStr::Vec(VecCache::from_vec(v)),
            LineData::GapBytes(v) => CacheStr::Gap(GapBytesCache::from_data(v)),
            LineData::GapBlockBytes(v1, v2) => {
                CacheStr::GapBlock(GapBytesCache::from_data(v1), GapBytesCache::from_data(v2))
            }
        }
    }

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> LineParts<&[u8]> {
        match self {
            CacheStr::Bytes(v) => LineParts::from_1(v.text(range)),
            CacheStr::Vec(v) => LineParts::from_1(v.text(range)),
            CacheStr::Gap(v) => {
                let (l, r) = v.text(&range);
                LineParts::from_2(l, r)
            }
            CacheStr::GapBlock(v1, v2) => {
                let len1 = v1.len();
                let total_len = len1 + v2.len();
                let start = match range.start_bound() {
                    Bound::Included(&s) => s,
                    Bound::Excluded(&s) => s + 1,
                    Bound::Unbounded => 0,
                };
                let end = match range.end_bound() {
                    Bound::Included(&e) => e,
                    Bound::Excluded(&e) => e,
                    Bound::Unbounded => total_len,
                };
                // v1 取 [start, min(end, len1)]
                let (l1, r1) = v1.text(&(start..end.min(len1)));
                // v2 的 range 需偏移 len1
                let v2_start = start.saturating_sub(len1);
                let v2_end = end.saturating_sub(len1);
                let (l2, r2) = v2.text(&(v2_start..v2_end));
                LineParts::from_4(l1, r1, l2, r2)
            }
        }
    }

    pub(crate) fn as_str(&self) -> LineParts<Cow<str>> {
        match self {
            CacheStr::Vec(v) => LineParts::from_1(v.as_str()),
            CacheStr::Bytes(v) => LineParts::from_1(v.as_str()),
            CacheStr::Gap(v) => LineParts::from_2(v.as_str().0, v.as_str().1),
            CacheStr::GapBlock(v1, v2) => {
                LineParts::from_4(v1.as_str().0, v1.as_str().1, v2.as_str().0, v2.as_str().1)
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            CacheStr::Gap(v) => v.len(),
            CacheStr::Vec(v) => v.len(),
            CacheStr::Bytes(v) => v.len(),
            CacheStr::GapBlock(v1, v2) => v1.len() + v2.len(),
        }
    }

    pub(crate) fn as_slice(&self) -> LineParts<&[u8]> {
        match self {
            CacheStr::Vec(v) => LineParts::from_1(v.as_slice()),
            CacheStr::Bytes(v) => LineParts::from_1(v.as_slice()),
            CacheStr::Gap(v) => LineParts::from_2(v.as_slice().0, v.as_slice().1),
            CacheStr::GapBlock(v1, v2) => LineParts::from_4(
                v1.as_slice().0,
                v1.as_slice().1,
                v2.as_slice().0,
                v2.as_slice().1,
            ),
        }
    }
}

enum LineDataU8Iter<'a> {
    U8Iter(std::slice::Iter<'a, u8>),
    GapU8Iter(GapBytesIter<'a>),
    GapBlockU8Iter(GapBytesBlockU8Iter<'a>),
}

impl Iterator for LineDataU8Iter<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LineDataU8Iter::U8Iter(iter) => iter.next().copied(),
            LineDataU8Iter::GapU8Iter(iter) => iter.next(),
            LineDataU8Iter::GapBlockU8Iter(iter) => iter.next(),
        }
    }
}

enum LineDataCharIter<'a> {
    CharIter(Utf8CharIndices<'a>),
    GapCharIter(GapBytesCharIter<'a>),
    GapBlockCharIter(GapBytesBlockCharIter<'a>),
}

impl<'a> Iterator for LineDataCharIter<'a> {
    type Item = (usize, char);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LineDataCharIter::CharIter(iter) => iter.next(),
            LineDataCharIter::GapCharIter(iter) => iter.next(),
            LineDataCharIter::GapBlockCharIter(iter) => iter.next(),
        }
    }
}

impl DoubleEndedIterator for LineDataCharIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            LineDataCharIter::CharIter(iter) => iter.next_back(),
            LineDataCharIter::GapCharIter(iter) => iter.next_back(),
            LineDataCharIter::GapBlockCharIter(iter) => iter.next_back(),
        }
    }
}

pub enum LineData<'a> {
    Own(Vec<u8>),
    Bytes(&'a [u8]),
    GapBytes(GapBytes<'a>),
    GapBlockBytes(GapBytes<'a>, GapBytes<'a>),
}

impl Debug for LineData<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineData::Own(v) => write!(f, "{}", String::from_utf8_lossy(v)),
            LineData::Bytes(v) => write!(f, "{}", String::from_utf8_lossy(v)),
            LineData::GapBytes(v) => write!(
                f,
                "{}{}",
                v.as_str_parts().0.as_str(),
                v.as_str_parts().1.as_str()
            ),
            LineData::GapBlockBytes(v1, v2) => {
                write!(
                    f,
                    "{}{}{}{}",
                    v1.as_str_parts().0.as_str(),
                    v1.as_str_parts().1.as_str(),
                    v2.as_str_parts().0.as_str(),
                    v2.as_str_parts().1.as_str()
                )
            }
        }
    }
}

impl Display for LineData<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LineData::Own(v) => write!(f, "{}", String::from_utf8_lossy(v)),
            LineData::Bytes(v) => write!(f, "{}", String::from_utf8_lossy(v)),
            LineData::GapBytes(v) => write!(
                f,
                "{},{}",
                v.as_str_parts().0.as_str(),
                v.as_str_parts().1.as_str()
            ),
            LineData::GapBlockBytes(v1, v2) => {
                write!(
                    f,
                    "{}{}{}{}",
                    v1.as_str_parts().0.as_str(),
                    v1.as_str_parts().1.as_str(),
                    v2.as_str_parts().0.as_str(),
                    v2.as_str_parts().1.as_str()
                )
            }
        }
    }
}

pub(crate) trait LineDefault {
    fn empty() -> Self;
}

impl<'a> LineDefault for Cow<'a, str> {
    fn empty() -> Self {
        Cow::Borrowed("")
    }
}

impl LineDefault for &str {
    fn empty() -> Self {
        ""
    }
}

impl LineDefault for &[u8] {
    fn empty() -> Self {
        &[]
    }
}

impl LineDefault for usize {
    fn empty() -> Self {
        0
    }
}

#[derive(Debug, Clone)]
pub struct LineParts<T: LineDefault + Debug> {
    pub(crate) data: [T; 4],
    pub(crate) length: usize,
}

impl<T: LineDefault + Debug> LineParts<T> {
    pub(crate) fn empty() -> Self {
        LineParts {
            data: [T::empty(), T::empty(), T::empty(), T::empty()],
            length: 0,
        }
    }

    pub(crate) fn append(&mut self, t: T) {
        if self.length < 4 {
            self.data[self.length] = t;
            self.length += 1;
        } else {
            panic!("LineParts is full");
        }
    }

    fn from_1(t: T) -> LineParts<T> {
        LineParts {
            data: [t, T::empty(), T::empty(), T::empty()],
            length: 1,
        }
    }

    fn from_2(t1: T, t2: T) -> LineParts<T> {
        LineParts {
            data: [t1, t2, T::empty(), T::empty()],
            length: 2,
        }
    }

    fn from_3(t1: T, t2: T, t3: T) -> LineParts<T> {
        LineParts {
            data: [t1, t2, t3, T::empty()],
            length: 3,
        }
    }

    fn from_4(t1: T, t2: T, t3: T, t4: T) -> LineParts<T> {
        LineParts {
            data: [t1, t2, t3, t4],
            length: 4,
        }
    }

    pub(crate) fn as_parts(&self) -> &[T] {
        &self.data[..self.length]
    }

    pub(crate) fn as_2parts(&self) -> (&T, &T) {
        (&self.data[0], &self.data[1])
    }
}

impl<'a> LineData<'a> {
    fn empty() -> LineData<'a> {
        LineData::Bytes(&[])
    }

    fn empty_gap_bytes() -> LineData<'a> {
        LineData::GapBytes(GapBytes::empty())
    }

    fn as_slice(&self) -> &[u8] {
        match self {
            LineData::Own(v) => v,
            LineData::Bytes(v) => v,
            LineData::GapBytes(v) => todo!(),
            LineData::GapBlockBytes(v1, v2) => todo!(),
        }
    }

    fn as_str_parts(&self) -> LineParts<Cow<str>> {
        match self {
            LineData::Own(v) => LineParts::from_1(String::from_utf8_lossy(v)),
            LineData::Bytes(v) => LineParts::from_1(String::from_utf8_lossy(v)),
            LineData::GapBytes(v) => LineParts::from_2(v.as_str_parts().0, v.as_str_parts().1),
            LineData::GapBlockBytes(v1, v2) => LineParts::from_4(
                v1.as_str_parts().0,
                v1.as_str_parts().1,
                v2.as_str_parts().0,
                v2.as_str_parts().1,
            ),
        }
    }

    fn as_parts(&self) -> LineParts<&[u8]> {
        match self {
            LineData::Own(v) => LineParts::from_1(v.as_slice()),
            LineData::Bytes(v) => LineParts::from_1(v),
            LineData::GapBytes(v) => {
                let (l, r) = v.as_slice();
                LineParts::from_2(l, r)
            }
            LineData::GapBlockBytes(v1, v2) => {
                let (l1, r1) = v1.as_slice();
                let (l2, r2) = v2.as_slice();
                LineParts::from_4(l1, r1, l2, r2)
            }
        }
    }

    fn len(&self) -> usize {
        match self {
            LineData::Own(v) => v.len(),
            LineData::Bytes(v) => v.len(),
            LineData::GapBytes(v) => v.len(),
            LineData::GapBlockBytes(v1, v2) => v1.len() + v2.len(),
        }
    }

    fn char_indices(&self) -> LineDataCharIter<'_> {
        match self {
            LineData::Own(v) => LineDataCharIter::CharIter(v.char_indices()),
            LineData::Bytes(v) => LineDataCharIter::CharIter(v.char_indices()),
            LineData::GapBytes(v) => LineDataCharIter::GapCharIter(v.char_indices()),
            LineData::GapBlockBytes(v1, v2) => LineDataCharIter::GapBlockCharIter(
                GapBytesBlockCharIter::new(v1.char_indices(), v2.char_indices(), v1.len()),
            ),
        }
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> LineData<'a> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end + 1,
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.len(),
        };
        assert!(start <= end);

        match self {
            LineData::Own(v) => LineData::Own(v.to_vec()),
            LineData::Bytes(v) => LineData::Bytes(&v[start..end]),
            LineData::GapBytes(v) => LineData::GapBytes(v.text(range)),
            LineData::GapBlockBytes(v1, v2) => {
                LineData::GapBlockBytes(v1.text(range.clone()), v2.text(range))
            }
        }
    }

    fn as_ref(&self) -> LineData<'a> {
        match self {
            LineData::Own(v) => LineData::Own(v.to_vec()),
            LineData::Bytes(v) => LineData::Bytes(v.clone()),
            LineData::GapBytes(v) => LineData::GapBytes(v.clone()),
            LineData::GapBlockBytes(v1, v2) => LineData::GapBlockBytes(v1.clone(), v2.clone()),
        }
    }
}

pub struct LineStr<'a> {
    pub(crate) data: LineData<'a>, //行数据
    pub(crate) block_id: usize,
    pub(crate) block_offset: usize,    //块内偏移
    pub(crate) line_file_start: usize, //行在文件开始位置
    pub(crate) line_file_end: usize,   //行在文件结束的位置
}

impl Display for LineStr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.data)
    }
}

impl<'a> Line<'a> for LineStr<'a> {
    fn text_len(&self) -> usize {
        self.data.len()
    }
    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> Self {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end + 1,
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.text_len(),
        };
        assert!(start <= end);
        let line_file_start = self.line_file_start + start;
        let line_file_end = self.line_file_end + end;

        LineStr {
            data: self.data.text(range),
            block_id: self.block_id,
            block_offset: self.block_offset + start,
            line_file_start: line_file_start,
            line_file_end: line_file_end,
        }
    }

    fn get_line_file_start(&self) -> usize {
        self.line_file_start
    }

    fn get_line_file_end(&self) -> usize {
        self.line_file_end
    }

    fn get_block_id(&self) -> usize {
        self.block_id
    }

    fn get_block_line_index(&self) -> usize {
        0
    }

    fn get_block_offset(&self) -> usize {
        self.block_offset
    }

    fn get_data(&self) -> LineData<'a> {
        self.data.as_ref()
    }

    fn iter_u8(&self) -> impl Iterator<Item = u8> {
        iter::empty()
    }

    // fn char_indices(&self) -> LineDataCharIter {
    //     self.data.char_indices()
    // }
}

#[inline]
fn find_in_parts(parts: &[&[u8]], key: &[u8]) -> Option<usize> {
    match parts.len() {
        0 => None,
        1 => memmem(parts[0], key),
        2 => memmem_two_slices_no_alloc(parts[0], parts[1], key),
        _ => memmem_small_slices_no_alloc(parts, key),
    }
}

#[inline]
fn suffix_parts<'b>(parts: &[&'b [u8]], mut offset: usize) -> ([&'b [u8]; 4], usize) {
    let mut suffix: [&'b [u8]; 4] = [&[]; 4];
    let mut suffix_len = 0;

    for part in parts {
        if offset >= part.len() {
            offset -= part.len();
            continue;
        }

        suffix[suffix_len] = &part[offset..];
        suffix_len += 1;
        offset = 0;
    }

    (suffix, suffix_len)
}

impl<'a> LineStr<'a> {
    fn empty() -> LineStr<'a> {
        LineStr {
            data: LineData::empty(),
            block_id: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        }
    }

    fn empty_gap_bytes() -> LineStr<'a> {
        LineStr {
            data: LineData::empty_gap_bytes(),
            block_id: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        }
    }

    pub(crate) fn search(&self, key: &[u8]) -> Vec<usize> {
        if key.is_empty() {
            return Vec::new();
        }
        let total_len = self.data.len();
        let mut matches = Vec::new();
        let mut search_start = 0;
        while search_start < total_len {
            let (suffix, suffix_len) = suffix_parts(&[self.data.as_slice()], search_start);
            let Some(relative_pos) = find_in_parts(&suffix[..suffix_len], key) else {
                break;
            };

            let match_pos = search_start + relative_pos;
            matches.push(match_pos);
            search_start = match_pos + key.len();
        }

        matches
    }
}

#[derive(Debug)]
pub(crate) struct BlockLineData<'a> {
    data: LineData<'a>,
    block_file_start: usize, //块在文件中的起始位置
    block_id: usize,         //块编号
    block_line_index: usize, //块内行号
    block_offset: usize,     //块内偏移
}

impl Default for BlockLineData<'_> {
    fn default() -> Self {
        BlockLineData::empty_gap_bytes()
    }
}

impl<'a> BlockLineData<'a> {
    fn empty_gap_bytes() -> BlockLineData<'a> {
        BlockLineData {
            data: LineData::empty_gap_bytes(),
            block_file_start: 0,
            block_id: 0,
            block_line_index: 0,
            block_offset: 0,
        }
    }
}

//一行数据 这行数据可能是在两个块中
pub(crate) struct LineBlockStr<'a>(Option<BlockLineData<'a>>, Option<BlockLineData<'a>>);

pub struct LineBlockStrU8Iter<'a> {
    block1_iter: Option<LineDataU8Iter<'a>>,
    block2_iter: Option<LineDataU8Iter<'a>>,
}

impl Iterator for LineBlockStrU8Iter<'_> {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(iter) = &mut self.block1_iter {
            if let Some(byte) = iter.next() {
                return Some(byte);
            }
        }
        if let Some(iter) = &mut self.block2_iter {
            return iter.next();
        }
        None
    }
}

impl Display for LineBlockStr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(b1) = &self.0 {
            write!(f, "{}", b1.data)?;
        }
        if let Some(b2) = &self.1 {
            write!(f, "{}", b2.data)
        } else {
            write!(f, "{}", "none")
        }
    }
}

impl<'a> LineBlockStr<'a> {
    fn get_end_block_offset(&self) -> usize {
        if let Some(b2) = &self.1 {
            b2.block_offset + b2.data.len()
        } else if let Some(b1) = &self.0 {
            b1.block_offset + b1.data.len()
        } else {
            0
        }
    }

    fn get_end_block_id(&self) -> usize {
        if let Some(b2) = &self.1 {
            b2.block_id
        } else if let Some(b1) = &self.0 {
            b1.block_id
        } else {
            0
        }
    }

    fn get_len1(&self) -> usize {
        self.0
            .as_ref()
            .unwrap_or(&BlockLineData::default())
            .data
            .len()
    }

    fn get_len2(&self) -> usize {
        self.1
            .as_ref()
            .unwrap_or(&BlockLineData::default())
            .data
            .len()
    }

    /// 提取行的原始字节（用于测试）
    pub(crate) fn to_bytes(&self) -> Vec<u8> {
        let mut result = Vec::new();
        if let Some(b1) = &self.0 {
            let slices = b1.data.as_parts();
            for i in 0..slices.length {
                result.extend_from_slice(slices.data[i]);
            }
        }
        if let Some(b2) = &self.1 {
            let slices = b2.data.as_parts();
            for i in 0..slices.length {
                result.extend_from_slice(slices.data[i]);
            }
        }
        result
    }

    pub(crate) fn search(&self, key: &[u8]) -> Vec<usize> {
        if key.is_empty() {
            return Vec::new();
        }

        fn append_line_data_parts<'b>(
            data: &'b LineData<'b>,
            parts: &mut [&'b [u8]; 4],
            parts_len: &mut usize,
        ) {
            let slices = data.as_parts();
            for part in slices.as_parts() {
                if !part.is_empty() {
                    parts[*parts_len] = part;
                    *parts_len += 1;
                }
            }
        }

        let mut parts: [&[u8]; 4] = [&[]; 4];
        let mut parts_len = 0;

        match (&self.0, &self.1) {
            (Some(v1), Some(v2)) => {
                append_line_data_parts(&v1.data, &mut parts, &mut parts_len);
                append_line_data_parts(&v2.data, &mut parts, &mut parts_len);
            }
            (Some(v1), None) => {
                append_line_data_parts(&v1.data, &mut parts, &mut parts_len);
            }
            (None, Some(v2)) => {
                append_line_data_parts(&v2.data, &mut parts, &mut parts_len);
            }
            (None, None) => {}
        }

        let total_len = parts[..parts_len].iter().map(|part| part.len()).sum();
        let mut matches = Vec::new();
        let mut search_start = 0;

        while search_start < total_len {
            let (suffix, suffix_len) = suffix_parts(&parts[..parts_len], search_start);
            let Some(relative_pos) = find_in_parts(&suffix[..suffix_len], key) else {
                break;
            };

            let match_pos = search_start + relative_pos;
            matches.push(match_pos);
            search_start = match_pos + key.len();
        }

        matches
    }
}

// struct LineBlockStrCharIter<'a> {
//     block1_iter: Option<GapBytesCharIter<'a>>,
//     block2_iter: Option<GapBytesCharIter<'a>>,
// }

impl<'a> Line<'a> for LineBlockStr<'a> {
    fn text_len(&self) -> usize {
        self.get_len1() + self.get_len2()
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> Self {
        let len1 = self.get_len1();
        //  let len2 = self.get_len2();
        let total_len = self.text_len();
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end,
            std::ops::Bound::Excluded(&end) => end,
            std::ops::Bound::Unbounded => self.text_len(),
        };
        if start == end || start > end || end > total_len {
            return LineBlockStr(None, None);
        }

        // 然后清晰地处理每种有效情况
        let (start1, end1, start2, end2) = if end <= len1 {
            // 范围完全在第一个块内
            (start, end, 0, 0)
        } else if start < len1 && end > len1 {
            // 范围跨越两个块
            (start, len1, 0, end - len1)
        } else {
            // 范围完全在第二个块内
            (0, 0, start - len1, end - len1)
        };
        // 定义处理块的匿名函数（闭包）
        let process_block = |block: Option<&BlockLineData<'a>>,
                             range_start: usize,
                             range_end: usize,
                             prev_offset: usize|
         -> Option<BlockLineData<'a>> {
            if range_start >= range_end {
                return None;
            }

            block.and_then(|block_line_data| match &block_line_data.data {
                LineData::GapBytes(v) => {
                    let data = v.text(range_start..range_end);
                    Some(BlockLineData {
                        data: LineData::GapBytes(data),
                        block_id: block_line_data.block_id,
                        block_line_index: block_line_data.block_line_index,
                        block_offset: block_line_data.block_offset + prev_offset + range_start,
                        block_file_start: block_line_data.block_file_start,
                    })
                }
                _ => None,
            })
        };

        // 使用闭包处理两个块
        let new_block1 = process_block(self.0.as_ref(), start1, end1, 0);
        let new_block2 = process_block(self.1.as_ref(), start2, end2, len1);

        LineBlockStr(new_block1, new_block2)
    }

    fn get_line_file_start(&self) -> usize {
        if let Some(b1) = &self.0 {
            b1.block_file_start + b1.block_offset
        } else if let Some(b2) = &self.1 {
            b2.block_file_start + b2.block_offset
        } else {
            0
        }
    }

    fn get_line_file_end(&self) -> usize {
        if let Some(b1) = &self.0 {
            b1.block_file_start + b1.block_offset + b1.data.len()
        } else if let Some(b2) = &self.1 {
            b2.block_file_start + b2.block_offset + b2.data.len()
        } else {
            0
        }
    }

    fn get_block_id(&self) -> usize {
        if let Some(b1) = &self.0 {
            b1.block_id
        } else if let Some(b2) = &self.1 {
            b2.block_id
        } else {
            0
        }
    }

    fn get_block_offset(&self) -> usize {
        if let Some(b1) = &self.0 {
            b1.block_offset
        } else if let Some(b2) = &self.1 {
            b2.block_offset
        } else {
            0
        }
    }

    fn get_block_line_index(&self) -> usize {
        if let Some(b1) = &self.0 {
            b1.block_line_index
        } else if let Some(b2) = &self.1 {
            b2.block_line_index
        } else {
            0
        }
    }

    fn get_data(&self) -> LineData<'a> {
        // match (&self.0.data, &self.1.data) {
        //     (LineData::GapBytes(v1), LineData::GapBytes(v2)) => {
        //         LineData::GapBlockBytes(v1.clone(), v2.clone())
        //     }
        //     _ => {
        //         panic!("LineBlockStr 只能是 GapBytes 类型");
        //     }
        // }
        let get_gap_bytes = |block: &Option<BlockLineData<'a>>| -> GapBytes<'a> {
            if let Some(block_data) = block {
                match &block_data.data {
                    LineData::GapBytes(v) => v.clone(),
                    _ => panic!("LineBlockStr 的块必须是 GapBytes 类型"),
                }
            } else {
                GapBytes::empty() // 或者 GapBytes::default()，取决于您的实现
            }
        };

        let v1 = get_gap_bytes(&self.0);
        let v2 = get_gap_bytes(&self.1);

        LineData::GapBlockBytes(v1, v2)
    }

    fn iter_u8(&self) -> impl Iterator<Item = u8> {
        LineBlockStrU8Iter {
            block1_iter: self.0.as_ref().and_then(|b| match &b.data {
                LineData::GapBytes(v) => Some(LineDataU8Iter::GapU8Iter(v.iter())),
                LineData::GapBlockBytes(v1, v2) => {
                    Some(LineDataU8Iter::GapBlockU8Iter(GapBytesBlockU8Iter {
                        left: v1.iter(),
                        right: v2.iter(),
                    }))
                }
                _ => None,
            }),
            block2_iter: self.1.as_ref().and_then(|b| match &b.data {
                LineData::GapBytes(v) => Some(LineDataU8Iter::GapU8Iter(v.iter())),
                LineData::GapBlockBytes(v1, v2) => {
                    Some(LineDataU8Iter::GapBlockU8Iter(GapBytesBlockU8Iter {
                        left: v1.iter(),
                        right: v2.iter(),
                    }))
                }
                _ => None,
            }),
        }
    }
}

pub(crate) trait TextOper {
    //滑动上一行
    fn scroll_pre_one_line(&self, meta: &LineState) -> ChapResult<()>;

    //滑动下一行
    fn scroll_next_one_line(&self, meta: &LineState) -> ChapResult<()>;

    //插入
    fn insert_char(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()>;

    fn insert_bytes(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        bytes: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()>;
    //
    fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()>;

    fn delete_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()>;

    fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>>;

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()>;

    //获取一页数据 从line_num 行开始
    // fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)>;

    fn get_one_page_from_state(
        &self,
        line_state: &LineState,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)>;

    fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)>;

    fn get_current_line_meta(&self) -> ChapResult<&RingVec<LineState>>;

    fn get_text_len_from_index(&self, line_index: usize) -> usize;

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8>;

    fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize>;

    fn search(&self, pattern: &[u8], line_state: &LineState) -> ChapResult<Option<Vec<LineState>>>;

    fn get_file_size(&self) -> usize;
}

pub(crate) trait Line<'a>: Display {
    fn text_len(&self) -> usize;
    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> Self;
    fn get_line_file_start(&self) -> usize;
    fn get_line_file_end(&self) -> usize;
    fn get_block_id(&self) -> usize;
    fn get_block_line_index(&self) -> usize;
    fn get_block_offset(&self) -> usize;
    fn get_data(&self) -> LineData<'a>;
    fn iter_u8(&self) -> impl Iterator<Item = u8>;
}

#[derive(Debug, Default, Clone)]
pub(crate) struct LineState {
    pub(crate) char_with: usize,
    pub(crate) txt_len: usize,  //文本长度
    pub(crate) char_len: usize, //char字符大小
    // pub(crate) page_num: usize,         //所在页数 从1开始
    pub(crate) block_num: usize,        //块的编号
    pub(crate) block_line_index: usize, //块内行号 从0开始 这个代表一行一行数据 用 '\n' 分隔的行号
    pub(crate) block_offset: usize,     //行在块的偏移
    //pub(crate) line_num: usize,         //行数 从1开始  这个代表实际行号 不是索引 代表视觉上的行号
    pub(crate) line_index: usize, //行号 从0开始 这个代表一行一行数据 用 '\n' 分隔的行号
    pub(crate) line_offset: usize, //这一行在总行的偏移量位置
    pub(crate) line_file_start: usize, //行在文件开始位置
    pub(crate) line_file_end: usize, //行在文件结束的位置
    // pub(crate) start_line_num: usize, //开始的行数
    //pub(crate) start_page_num: usize,   //这一行在第几页开始
    pub(crate) highlight: Option<Vec<usize>>, //高亮范围  有搜索的时候
}

impl LineState {
    pub(crate) fn get_block_line_end(&self) -> usize {
        self.block_offset + self.line_offset + self.txt_len
    }

    pub(crate) fn has_pre_line(&self) -> bool {
        if self.line_offset == 0 && self.line_index == 0 {
            return false;
        }
        return true;
    }
}

// 用于构建 LineState 的建造者结构体
pub(crate) struct LineStateBuilder {
    char_with: Option<usize>,
    txt_len: Option<usize>,
    char_len: Option<usize>,
    page_num: Option<usize>,
    block_num: Option<usize>,
    block_line_index: Option<usize>,
    block_offset: Option<usize>,
    line_num: Option<usize>,
    line_index: Option<usize>,
    line_offset: Option<usize>,
    line_file_start: Option<usize>,
    line_file_end: Option<usize>,
    start_line_num: Option<usize>,
    start_page_num: Option<usize>,
}

impl LineStateBuilder {
    /// 创建一个新的 LineStateBuilder，所有字段初始为 None
    pub(crate) fn new() -> Self {
        LineStateBuilder {
            char_with: None,
            txt_len: None,
            char_len: None,
            page_num: None,
            block_num: None,
            block_line_index: None,
            block_offset: None,
            line_num: None,
            line_index: None,
            line_offset: None,
            line_file_start: None,
            line_file_end: None,
            start_line_num: None,
            start_page_num: None,
        }
    }

    // 以下是每个字段的设置方法，它们返回建造者自身以支持链式调用

    pub(crate) fn txt_len(mut self, value: usize) -> Self {
        self.txt_len = Some(value);
        self
    }

    pub(crate) fn char_len(mut self, value: usize) -> Self {
        self.char_len = Some(value);
        self
    }

    pub(crate) fn page_num(mut self, value: usize) -> Self {
        self.page_num = Some(value);
        self
    }

    pub(crate) fn block_num(mut self, value: usize) -> Self {
        self.block_num = Some(value);
        self
    }

    pub(crate) fn block_line_index(mut self, value: usize) -> Self {
        self.block_line_index = Some(value);
        self
    }

    pub(crate) fn block_offset(mut self, value: usize) -> Self {
        self.block_offset = Some(value);
        self
    }

    pub(crate) fn line_num(mut self, value: usize) -> Self {
        self.line_num = Some(value);
        self
    }

    pub(crate) fn line_index(mut self, value: usize) -> Self {
        self.line_index = Some(value);
        self
    }

    pub(crate) fn line_offset(mut self, value: usize) -> Self {
        self.line_offset = Some(value);
        self
    }

    pub(crate) fn line_file_start(mut self, value: usize) -> Self {
        self.line_file_start = Some(value);
        self
    }

    pub(crate) fn line_file_end(mut self, value: usize) -> Self {
        self.line_file_end = Some(value);
        self
    }

    pub(crate) fn start_line_num(mut self, value: usize) -> Self {
        self.start_line_num = Some(value);
        self
    }

    pub(crate) fn start_page_num(mut self, value: usize) -> Self {
        self.start_page_num = Some(value);
        self
    }

    /// 构建最终的 LineState 对象。
    /// 对于未设置的字段，将使用 LineState 的默认值。
    pub(crate) fn build(self) -> LineState {
        let default = LineState::default();
        LineState {
            char_with: self.char_with.unwrap_or(default.char_with),
            txt_len: self.txt_len.unwrap_or(default.txt_len),
            char_len: self.char_len.unwrap_or(default.char_len),
            //page_num: self.page_num.unwrap_or(default.page_num),
            block_num: self.block_num.unwrap_or(default.block_num),
            block_line_index: self.block_line_index.unwrap_or(default.block_line_index),
            block_offset: self.block_offset.unwrap_or(default.block_offset),
            // line_num: self.line_num.unwrap_or(default.line_num),
            line_index: self.line_index.unwrap_or(default.line_index),
            line_offset: self.line_offset.unwrap_or(default.line_offset),
            line_file_start: self.line_file_start.unwrap_or(default.line_file_start),
            line_file_end: self.line_file_end.unwrap_or(default.line_file_end),
            //start_line_num: self.start_line_num.unwrap_or(default.start_line_num),
            // start_page_num: self.start_page_num.unwrap_or(default.start_page_num),
            highlight: None, // 高亮范围默认设置为 None
        }
    }
}

impl LineState {
    pub(crate) fn builder() -> LineStateBuilder {
        LineStateBuilder::new()
    }
}

pub(crate) trait Text {
    type LineItem<'a>: Line<'a>
    where
        Self: 'a;
    fn get_file_size(&self) -> usize;

    //是否有上一行
    fn has_pre_line(&self, meta: &LineState) -> bool;

    //是否有下一行
    fn has_next_line(&self, meta: &LineState) -> bool;

    //获取一行
    fn get_line<'a>(&'a self, state: &LineState) -> Option<Self::LineItem<'a>>;

    //获取下一行的状态
    fn get_next_line_state(&self, state: &LineState) -> Option<LineState>;

    //获取上一行的状态
    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState>;

    //获取行的文本长度
    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize;

    //获取选中文本
    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8>;

    //迭代一行数据
    fn iter<'a>(&'a mut self, state: &LineState) -> impl Iterator<Item = Self::LineItem<'a>>;

    //反向迭代
    fn iter_rev<'a>(&'a mut self, state: &LineState) -> impl Iterator<Item = Self::LineItem<'a>>;

    //迭代字节数据
    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8>;

    fn search(&mut self, partten: &[u8], state: &LineState) -> ChapResult<Option<Vec<LineState>>>;
}

pub(crate) trait TextIndex {
    fn get_page_offset(&self, line_num: usize) -> LineState;
    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState);
}

pub(crate) trait EditText {
    // 插入
    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()>;
    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()>;
    // 插入新行
    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()>;
    //删除行
    fn delete_line(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()>;
    // 删除
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>>;

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()>;

    fn save_file<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        let backup_name = Self::get_backup_name(&filepath)?;
        self.make_backup(&backup_name)?;
        Self::rename_backup(&filepath, &backup_name)?;
        Ok(())
    }

    fn rename_backup<P1: AsRef<Path>, P2: AsRef<Path>>(
        filepath: P1,
        backup_name: P2,
    ) -> ChapResult<()> {
        fs::rename(backup_name, filepath)?;
        Ok(())
    }

    fn get_backup_name<P: AsRef<Path>>(filepath: P) -> ChapResult<PathBuf> {
        let mut path = filepath.as_ref().to_path_buf();
        if let Some(file_name) = path.file_name() {
            path.set_file_name(format!(".{}.{}", file_name.to_string_lossy(), "chap"));
        }
        Ok(path)
    }

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.save_file(filepath)
    }

    fn rollback() -> ChapResult<()>;

    fn ensure_block_loaded(&mut self, block_num: usize) -> ChapResult<()>;
}

impl LineState {
    pub(crate) fn new(
        char_with: usize,
        txt_len: usize,
        char_len: usize,
        // page_num: usize,
        block_num: usize,
        block_line_index: usize,
        block_offset: usize,
        // line_num: usize,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> LineState {
        LineState {
            char_with,
            txt_len,
            char_len,
            // page_num,
            block_num,
            block_line_index,
            block_offset,
            //line_num,
            line_index,
            line_offset,
            line_file_start: line_file_start,
            line_file_end: line_file_end,
            // start_line_num: 0,
            // start_page_num: 0,
            highlight: None,
        }
    }

    pub(crate) fn get_hex_len(&self) -> usize {
        (self.txt_len * 3 + self.txt_len / 8).saturating_sub(1)
    }

    // pub(crate) fn get_line_num(&self) -> usize {
    //     self.line_num
    // }

    // pub(crate) fn get_page_num(&self) -> usize {
    //     self.page_num
    // }

    pub(crate) fn get_line_offset(&self) -> usize {
        self.line_offset
    }

    pub(crate) fn get_line_end(&self) -> usize {
        self.line_offset + self.txt_len
    }

    pub(crate) fn get_line_index(&self) -> usize {
        self.line_index
    }

    pub(crate) fn get_txt_len(&self) -> usize {
        self.txt_len
    }

    pub(crate) fn get_char_len(&self) -> usize {
        self.char_len
    }

    pub(crate) fn get_char_with(&self) -> usize {
        self.char_with
    }

    pub(crate) fn get_line_file_start(&self) -> usize {
        self.line_file_start
    }

    pub(crate) fn get_line_file_end(&self) -> usize {
        self.line_file_end
    }

    pub(crate) fn get_block_num(&self) -> usize {
        self.block_num
    }

    pub(crate) fn get_block_line_index(&self) -> usize {
        self.block_line_index
    }

    pub(crate) fn get_block_offset(&self) -> usize {
        self.block_offset
    }
}

pub(crate) enum TextDisplay {
    Text(TextWarp<MmapText>),
    Hex(EditTextWarp<HexText>),
    Edit(EditTextWarp<GapText>),
    EditBlock(EditTextWarp<GapBlockText>),
}

impl TextOper for TextDisplay {
    fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        match self {
            TextDisplay::Text(v) => v.get_current_page(),
            TextDisplay::Hex(v) => v.get_current_page(),
            TextDisplay::Edit(v) => v.get_current_page(),
            TextDisplay::EditBlock(v) => v.get_current_page(),
        }
    }

    fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize> {
        match self {
            TextDisplay::Text(v) => v.find(pattern, line_file_start),
            TextDisplay::Hex(v) => v.find(pattern, line_file_start),
            TextDisplay::Edit(v) => None,
            TextDisplay::EditBlock(v) => None,
        }
    }

    fn search(&self, pattern: &[u8], line_state: &LineState) -> ChapResult<Option<Vec<LineState>>> {
        match self {
            TextDisplay::Text(v) => v.search(pattern, line_state),
            TextDisplay::Hex(v) => Ok(None),
            TextDisplay::Edit(v) => Ok(None),
            TextDisplay::EditBlock(v) => v.search(pattern, line_state),
        }
    }

    fn get_file_size(&self) -> usize {
        match self {
            TextDisplay::Text(v) => v.get_file_size(),
            TextDisplay::Hex(v) => v.get_file_size(),
            TextDisplay::Edit(v) => v.get_file_size(),
            TextDisplay::EditBlock(v) => v.get_file_size(),
        }
    }

    fn get_current_line_meta(&self) -> ChapResult<&RingVec<LineState>> {
        match self {
            TextDisplay::Text(v) => v.get_current_line_meta(),
            TextDisplay::Hex(v) => v.get_current_line_meta(),
            TextDisplay::Edit(v) => v.get_current_line_meta(),
            TextDisplay::EditBlock(v) => v.get_current_line_meta(),
        }
    }

    fn get_text_len_from_index(&self, line_index: usize) -> usize {
        match self {
            TextDisplay::Text(v) => v.get_text_len(line_index),
            TextDisplay::Hex(v) => v.get_text_len(line_index),
            TextDisplay::Edit(v) => v.get_text_len(line_index),
            TextDisplay::EditBlock(v) => v.get_text_len(line_index),
        }
    }

    fn insert_char(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => v.insert_char(cursor_y, bytes_cursor, line_meta, c),
            TextDisplay::Edit(v) => v.insert_char(cursor_y, bytes_cursor, line_meta, c),
            TextDisplay::EditBlock(v) => v.insert_char(cursor_y, bytes_cursor, line_meta, c),
        }
    }

    fn insert_bytes(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => {
                v.insert_bytes(cursor_y, bytes_cursor, line_meta, c, is_overwrite)
            }
            TextDisplay::Edit(v) => {
                v.insert_bytes(cursor_y, bytes_cursor, line_meta, c, is_overwrite)
            }
            TextDisplay::EditBlock(v) => {
                v.insert_bytes(cursor_y, bytes_cursor, line_meta, c, is_overwrite)
            }
        }
    }

    fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.insert_newline(cursor_y, cursor_x, line_meta),
            TextDisplay::EditBlock(v) => v.insert_newline(cursor_y, cursor_x, line_meta),
        }
    }

    fn delete_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.delete_newline(cursor_y, cursor_x, line_meta),
            TextDisplay::EditBlock(v) => v.delete_newline(cursor_y, cursor_x, line_meta),
        }
    }

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.save(filepath),
            TextDisplay::EditBlock(v) => v.save(filepath),
        }
    }

    fn scroll_next_one_line(&self, meta: &LineState) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => v.scroll_next_one_line(meta),
            TextDisplay::Hex(v) => v.scroll_next_one_line(meta),
            TextDisplay::Edit(v) => v.scroll_next_one_line(meta),
            TextDisplay::EditBlock(v) => v.scroll_next_one_line(meta),
        }
    }

    fn scroll_pre_one_line(&self, meta: &LineState) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => v.scroll_pre_one_line2(meta),
            TextDisplay::Hex(v) => v.scroll_pre_one_line2(meta),
            TextDisplay::Edit(v) => v.scroll_pre_one_line2(meta),
            TextDisplay::EditBlock(v) => v.scroll_pre_one_line2(meta),
        }
    }

    fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>> {
        match self {
            TextDisplay::Text(v) => Ok(vec![]),
            TextDisplay::Hex(v) => v.backspace(cursor_y, cursor_x, count, line_meta),
            TextDisplay::Edit(v) => v.backspace(cursor_y, cursor_x, count, line_meta),
            TextDisplay::EditBlock(v) => v.backspace(cursor_y, cursor_x, count, line_meta),
        }
    }

    // fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
    //     match self {
    //         TextDisplay::Text(v) => v.get_one_page(line_num),
    //         TextDisplay::Hex(v) => v.get_one_page(line_num),
    //         TextDisplay::Edit(v) => v.get_one_page(line_num),
    //         TextDisplay::EditBlock(v) => v.get_one_page(line_num),
    //     }
    // }

    fn get_one_page_from_state(
        &self,
        line_state: &LineState,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        match self {
            TextDisplay::Text(v) => v.get_one_page_from_line_state(line_state),
            TextDisplay::Hex(v) => todo!(),
            TextDisplay::Edit(v) => todo!(),
            TextDisplay::EditBlock(v) => v.get_one_page_from_line_state(line_state),
        }
    }

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        match self {
            TextDisplay::Text(v) => v.get_text_from_sel(sel),
            TextDisplay::Hex(v) => v.get_text_from_sel(sel),
            TextDisplay::Edit(v) => todo!("Not implement get_text_from_sel for EditTextWarp"),
            TextDisplay::EditBlock(v) => todo!("Not implement get_text_from_sel for EditTextWarp"),
        }
    }
}

pub(crate) struct TextWarp<T: Text + TextIndex> {
    lines: UnsafeCell<T>,                            // 文本
    cache_lines: UnsafeCell<RingVec<CacheStr>>,      // 缓存行
    cache_line_meta: UnsafeCell<RingVec<LineState>>, // 缓存行元数据
    //page_offset_list: UnsafeCell<Vec<PageOffset>>,      // 每页的偏移量
    height: usize, //最大行数
    with: usize,
    text_warp_type: TextWarpType,
}

impl<T: Text + TextIndex> TextWarp<T> {
    pub(crate) fn new(
        lines: T,
        height: usize,
        with: usize,
        text_warp_type: TextWarpType,
    ) -> TextWarp<T> {
        TextWarp {
            lines: UnsafeCell::new(lines),
            cache_lines: UnsafeCell::new(RingVec::with_capacity(height)),
            cache_line_meta: UnsafeCell::new(RingVec::with_capacity(height)),
            // page_offset_list: UnsafeCell::new(vec![PageOffset {
            //     line_index: 0,
            //     line_offset: 0,
            //     line_file_start: 0,
            // }]),
            height,
            with,
            text_warp_type: text_warp_type,
        }
    }

    fn borrow_lines(&self) -> &T {
        unsafe { &*self.lines.get() }
    }

    fn borrow_lines_mut(&self) -> &mut T {
        unsafe { &mut *self.lines.get() }
    }

    // fn borrow_page_offset_list(&self) -> &Vec<PageOffset> {
    //     unsafe { &*self.page_offset_list.get() }
    // }

    // fn borrow_page_offset_list_mut(&self) -> &mut Vec<PageOffset> {
    //     unsafe { &mut *self.page_offset_list.get() }
    // }

    fn borrow_cache_lines(&self) -> &RingVec<CacheStr> {
        unsafe { &*self.cache_lines.get() }
    }

    fn borrow_cache_lines_mut(&self) -> &mut RingVec<CacheStr> {
        unsafe { &mut *self.cache_lines.get() }
    }

    fn borrow_cache_line_meta(&self) -> &RingVec<LineState> {
        unsafe { &*self.cache_line_meta.get() }
    }

    fn borrow_cache_line_meta_mut(&self) -> &mut RingVec<LineState> {
        unsafe { &mut *self.cache_line_meta.get() }
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        self.borrow_lines().get_line_text_len(index, 0, 0)
    }

    fn get_file_size(&self) -> usize {
        self.borrow_lines().get_file_size()
    }

    // 计算页码，等同于向上取整
    fn get_page_num(&self, num: usize) -> usize {
        (num + self.height - 1) / self.height
    }

    // 计算行数，等同于向上取整
    fn calculate_lines(text_len: usize, with: usize) -> usize {
        if text_len == 0 {
            return 1;
        }
        (text_len as f64 / with as f64).ceil() as usize
    }

    pub(crate) fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize> {
        let hex_u8_iter = self.borrow_lines_mut().iter_u8(0, 0, line_file_start);
        let bm = BoyerMoore::new(pattern);
        let mut j = None;
        for i in bm.stream(hex_u8_iter) {
            j = Some(i);
            break;
        }
        j
    }

    /**
     * 滚动上一行
     */
    pub(crate) fn scroll_pre_one_line2(&self, meta: &LineState) -> ChapResult<()> {
        let (s, l) = self.scroll(meta, 1, true);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push_front(s);
            self.borrow_cache_line_meta_mut().push_front(l);
        }
        Ok(())
    }

    //滚动指定行数
    pub(crate) fn scroll(
        &self,
        meta: &LineState,
        line_count: usize,
        is_rev: bool,
    ) -> (Option<CacheStr>, LineState) {
        if is_rev {
            self.get_pre_line2(meta, line_count)
        } else {
            self.get_next_line(meta, line_count)
        }
    }

    //从当前行开始获取前面n行
    pub(crate) fn get_pre_line2<'a>(
        &self,
        meta: &LineState,
        line_count: usize,
    ) -> (Option<CacheStr>, LineState) {
        //assert!(meta.get_line_num() >= 1);

        if !self.borrow_lines().has_pre_line(meta) {
            return (None, LineState::default());
        }

        let pre_line_state = self.borrow_lines_mut().get_pre_line_state(meta).unwrap();
        // let mut line_index = meta.get_line_index();
        // let mut line_offset = meta.get_line_offset();
        // let mut line_file_start = meta.get_line_file_start();
        // let mut start_line_num = meta.get_line_num();
        // let mut start_page_num = meta.get_page_num();

        // //这行已经是最开始的一行了
        // if meta.get_line_offset() == 0 {
        //     line_index = meta.get_line_index().saturating_sub(1);
        //     line_offset = 0;
        //     line_file_start = 0;
        //     start_line_num = start_line_num;
        //     start_page_num = 0;
        // }

        // let page_offset = PageOffset {
        //     line_index,
        //     line_offset: line_offset,
        //     block_num: 0,
        //     block_line_index: 0,
        //     block_offset: 0,
        //     line_file_start: line_file_start,
        //     start_line_num: start_line_num,
        //     start_page_num: start_page_num,
        // };

        let mut s = LineData::empty();
        let mut m = LineState::default();

        self.get_char_text_fn(
            &pre_line_state,
            line_count,
            // page_offset.start_line_num,
            // page_offset.start_page_num,
            0,
            true,
            &mut |txt, m1| {
                s = txt;
                m = m1;
            },
        );
        (Some(CacheStr::from_data(s)), m)
    }

    // //从当前行开始获取前面n行
    // pub(crate) fn get_pre_line<'a>(
    //     &'a self,
    //     meta: &LineState,
    //     line_count: usize,
    // ) -> (Option<CacheStr>, LineState) {
    //     // assert!(meta.get_line_num() >= 1);

    //     // if meta.get_line_num() == 1 {
    //     //     return (None, LineState::default());
    //     // }
    //     if !self.borrow_lines().has_pre_line(meta) {
    //         return (None, LineState::default());
    //     }
    //     let mut s = LineData::empty();
    //     let mut m = LineState::default();
    //     self.get_text_from_line_num(meta.get_line_num() - line_count, line_count, |txt, meta| {
    //         s = txt;
    //         m = meta;
    //     });
    //     (Some(CacheStr::from_data(s)), m)
    // }

    //从当前行开始获取后面n行
    pub(crate) fn get_next_line<'a>(
        &'a self,
        meta: &LineState,
        line_count: usize,
    ) -> (Option<CacheStr>, LineState) {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();
        let mut line_file_start = meta.get_line_file_start();
        if !self.borrow_lines().has_next_line(meta) {
            log::debug!("没有下一行了");
            return (None, LineState::default());
        }
        let mut next_line_state = self.borrow_lines_mut().get_next_line_state(&meta).unwrap();

        //这行已经读完 开始下一行
        // if line_end == meta.get_txt_len() {
        //     line_file_start = meta.get_line_file_end();
        //     line_end = 0;
        //     line_index += 1;
        // }

        // let p = PageOffset {
        //     line_index: line_index,
        //     line_offset: line_end,
        //     block_num: 0,        //行所在块编号
        //     block_line_index: 0, //行在块内的行号
        //     block_offset: 0,     //行所在块偏移
        //     line_file_start: line_file_start,
        //     start_line_num: meta.get_line_num(),
        //     start_page_num: meta.get_line_num() / self.height,
        // };
        //next_line_state.start_page_num = meta.get_line_num() / self.height;
        let mut s = LineData::empty();
        let mut m = LineState::default();
        self.get_char_text_fn(&next_line_state, line_count, 0, false, &mut |x, m1| {
            s = x;
            m = m1;
        });
        (Some(CacheStr::from_data(s)), m)
    }

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(&self, meta: &LineState) -> ChapResult<()> {
        let (s, l) = self.get_next_line(meta, 1);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push(s);
            self.borrow_cache_line_meta_mut().push(l);
        }
        Ok(())
    }

    // /**
    //  * 滚动上一行
    //  */
    // pub(crate) fn scroll_pre_one_line(&self, meta: &LineState) -> ChapResult<()> {
    //     let (s, l) = self.get_pre_line(meta, 1);
    //     if let Some(s) = s {
    //         self.borrow_cache_lines_mut().push_front(s);
    //         self.borrow_cache_line_meta_mut().push_front(l);
    //     }
    //     Ok(())
    // }

    // pub(crate) fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
    //     assert!(line_num >= 1);

    //     let page_offset = self.borrow_lines().get_page_offset(line_num);
    //     assert!(line_num >= page_offset.start_line_num);
    //     //跳过的行数
    //     let skip_line = line_num;
    //     self.get_line_content(&page_offset, skip_line, self.height)
    // }

    pub(crate) fn get_one_page_from_line_state(
        &self,
        line_state: &LineState,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        // assert!(line_state.line_num >= 1);

        //跳过的行数
        let skip_line = 0; //line_state.line_num;
        self.get_line_content(line_state, skip_line, self.height)
    }

    pub(crate) fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        Ok((self.borrow_cache_lines(), self.borrow_cache_line_meta()))
    }

    pub(crate) fn get_current_line_meta(&self) -> ChapResult<&RingVec<LineState>> {
        Ok(self.borrow_cache_line_meta())
    }

    // 从第n行开始获取内容
    pub(crate) fn get_line_content(
        &self,
        line_state: &LineState,
        skip_line: usize,
        line_count: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        self.borrow_cache_lines_mut().clear();
        self.borrow_cache_line_meta_mut().clear();

        self.get_char_text_fn(
            line_state,
            line_count,
            skip_line,
            false,
            &mut |txt, meta| {
                self.borrow_cache_lines_mut().push(CacheStr::from_data(txt));
                self.borrow_cache_line_meta_mut().push(meta);
            },
        );

        Ok((self.borrow_cache_lines(), self.borrow_cache_line_meta()))
    }

    // fn get_text_from_line_num<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    // where
    //     F: FnMut(LineData<'a>, LineState),
    // {
    //     //assert!(line_num >= 1);
    //     // // 计算页码
    //     // let page_num = self.get_page_num(line_num);
    //     // // 计算页码
    //     // let mut index = (page_num - 1) / PAGE_GROUP;
    //     // let page_offset_list = self.borrow_page_offset_list();
    //     // let page_offset = if index >= page_offset_list.len() {
    //     //     index = page_offset_list.len() - 1;
    //     //     page_offset_list.last().unwrap()
    //     // } else {
    //     //     &page_offset_list[index]
    //     // };
    //     // let start_page_num = index * PAGE_GROUP;

    //     //let page_offset = self.borrow_lines().get_page_offset(line_num);

    //     //assert!(line_num >= page_offset.start_line_num);
    //     //跳过的行数
    //     //let skip_line = line_num;
    //     // println!("skip_line:{}", skip_line);
    //     // println!("page_offset:{:?}", page_offset);
    //     self.get_char_text_fn(&page_offset, line_count, skip_line, false, &mut f);
    // }

    fn get_char_text_fn<'a, F>(
        &'a self,
        line_state: &LineState,
        line_count: usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, LineState),
    {
        let mut cur_line_count = 0;
        //let mut line_num = line_state.start_line_num;
        // let mut page_num = line_state.start_page_num;

        // let line_state = LineState {
        //     line_index: page_offset.line_index,
        //     line_offset: page_offset.line_offset,
        //     block_num: page_offset.block_num,
        //     block_line_index: page_offset.block_line_index,
        //     block_offset: page_offset.block_offset,
        //     line_file_start: page_offset.line_file_start,
        //     line_file_end: 0,
        //     start_line_num: page_offset.start_line_num,
        //     start_page_num: page_offset.start_page_num,
        // };

        let iter = if is_rev {
            Either::Left(self.borrow_lines_mut().iter_rev(&line_state))
        } else {
            Either::Right(self.borrow_lines_mut().iter(&line_state))
        };

        for (i, v) in iter.enumerate() {
            let line_offset = if i == 0 { line_state.line_offset } else { 0 };
            let line_index = if is_rev {
                line_state.line_index.saturating_sub(i)
            } else {
                line_state.line_index + i
            };
            Self::set_line_char_txt(
                v,
                line_index,
                line_offset,
                self.with,
                self.height,
                self.borrow_lines_mut(),
                //  &mut line_num,
                line_count,
                //  &mut page_num,
                &mut cur_line_count,
                skip_line,
                &self.text_warp_type,
                is_rev,
                f,
            );
            if cur_line_count >= line_count {
                return;
            }
        }
    }

    fn set_line_char_txt<'a, F, I: TextIndex, L: Line<'a> + 'a>(
        line_str: L,        // 行内容
        line_index: usize,  // 行索引
        line_offset: usize, // 行起始位置
        with: usize,        // 每行宽度
        height: usize,      // 每页行数
        text_index: &mut I, // 文本索引
        // line_num: &mut usize,          // 当前行号 (视觉)
        line_count: usize, // 需要获取的行数
        // page_num: &mut usize,          // 当前页码
        cur_line_count: &mut usize,    // 已获取的行数
        skip_line: usize,              // 跳过的行数
        text_warp_type: &TextWarpType, // 文本换行类型
        is_rev: bool,                  // 是否是反向读取
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, LineState),
    {
        //空行

        let line_txt = if is_rev && line_offset > 0 {
            line_str.text(..line_offset)
        } else {
            line_str.text(line_offset..)
        };
        if line_txt.text_len() == 0 {
            // if is_rev {
            //     *line_num = (*line_num).saturating_sub(1);
            // } else {
            //     *line_num += 1; //行数加1
            // }
            if true {
                //*line_num >= skip_line
                *cur_line_count += 1;
                f(
                    LineData::empty(),
                    LineState::new(
                        0,
                        0,
                        0,
                        //  get_page_number!(*line_num, height),
                        line_str.get_block_id(),
                        line_str.get_block_line_index(),
                        line_str.get_block_offset(),
                        // *line_num,
                        line_index,
                        0,
                        line_str.get_line_file_start(),
                        line_str.get_line_file_end(),
                    ),
                );
            }
            // if *line_num % height == 0 && !is_rev {
            //     //到达一页
            //     *page_num += 1; //页数加1
            //                     //let m = *page_num / PAGE_GROUP;
            //                     // let n = *page_num % PAGE_GROUP;
            //     let line_state = LineState::builder()
            //         .line_index(line_index + 1)
            //         .line_offset(0)
            //         .block_num(0) //行所在块编号
            //         .block_line_index(0) //行在块内的行号
            //         .block_offset(0) //行所在块偏移
            //         .line_file_start(line_str.get_line_file_end())
            //         .start_line_num(0)
            //         .start_page_num(0)
            //         .build();

            //     text_index.set_page_offset(*page_num, line_state);
            //     // if n == 0 && m > page_offset_list.len() - 1 {
            //     //     //保存页数的偏移量
            //     //     page_offset_list.push(PageOffset {
            //     //         line_index: line_index + 1,
            //     //         line_offset: 0,
            //     //         line_file_start: line_str.line_file_end,
            //     //         start_line_num: 0,
            //     //         start_page_num: 0,
            //     //     });
            //     // }
            // }

            if *cur_line_count >= line_count {
                return;
            }
            return;
        }

        match text_warp_type {
            TextWarpType::NoWrap => {
                Self::no_warp(
                    line_str,
                    line_index,
                    line_offset,
                    with,
                    height,
                    text_index,
                    //line_num,
                    line_count,
                    //page_num,
                    cur_line_count,
                    skip_line,
                    is_rev,
                    f,
                );
            }
            TextWarpType::SoftWrap => {
                Self::sort_warp(
                    line_str,
                    line_index,
                    line_offset,
                    with,
                    height,
                    text_index,
                    // line_num,
                    line_count,
                    //page_num,
                    cur_line_count,
                    skip_line,
                    is_rev,
                    f,
                );
            }
        }
    }

    fn no_warp<'a, F, I: TextIndex, L: Line<'a> + 'a>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        // line_num: &mut usize,
        line_count: usize,
        // page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, LineState),
    {
        let line_txt = line_str.text(line_start..);
        // if is_rev {
        //     *line_num = (*line_num).saturating_sub(1); //行数减1
        // } else {
        //     *line_num += 1; //行数加1
        // }
        if true {
            *cur_line_count += 1;
            let txt_len = line_txt.text_len();
            // 单次遍历同时统计显示宽度和字符数，避免两次独立扫描
            let (char_with, char_len) = line_txt
                .get_data()
                .char_indices()
                .fold((0usize, 0usize), |(w, n), (_, ch)| {
                    (w + ch.width().unwrap_or(0), n + 1)
                });
            f(
                line_txt.get_data(),
                LineState::new(
                    char_with,
                    txt_len,
                    char_len,
                    //  *page_num + 1,
                    line_str.get_block_id(),
                    line_str.get_block_line_index(),
                    line_str.get_block_offset(),
                    //  *line_num,
                    line_index,
                    line_start + 0,
                    line_str.get_line_file_start(),
                    line_str.get_line_file_end(),
                ),
            );
        }
        // if *line_num % height == 0 && !is_rev {
        //     //到达一页
        //     *page_num += 1; //页数加1
        //     let state = LineState::builder()
        //         .line_index(line_index + 1)
        //         .line_offset(0)
        //         .block_num(0) //行所在块编号
        //         .block_line_index(0) //行在块内的行号
        //         .block_offset(0) //行所在块偏移
        //         .line_file_start(line_str.get_line_file_end())
        //         .start_line_num(0)
        //         .start_page_num(0)
        //         .build();
        //     text_index.set_page_offset(*page_num, state);

        //     // let m = *page_num / PAGE_GROUP;
        //     // let n = *page_num % PAGE_GROUP;
        //     // if n == 0 && m > page_offset_list.len() - 1 {
        //     //     //保存页数的偏移量
        //     //     page_offset_list.push();
        //     // }
        // }
        if *cur_line_count >= line_count {
            return;
        }
    }

    fn get_last_sort_warp_line<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        //line_num: &mut usize,
        line_count: usize,
        // page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, LineState),
    {
        let line_txt = line_str.text(line_start..);
        let mut current_width = 0; // 当前行宽度
        let mut line_offset = 0; //当前行偏移量
        let mut char_index = 0; // 当前行字符索引
        let mut char_count = 0; // 当前行字符数
        let mut last_offset = 0;
        for (i, (byte_index, ch)) in line_txt.get_data().char_indices().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            //检查是否超过屏幕宽度
            if current_width + ch_width > with {
                last_offset = line_offset;
                char_index = i;
                line_offset = byte_index;
                current_width = 0;
            }
            char_count += 1;
            current_width += ch_width;
        }
        *cur_line_count += 1;
        // *line_num = (*line_num).saturating_sub(1); //行数减1
        let txt = if current_width > 0 {
            //当前行没有到达屏幕宽度 但还是一行 这里就是最后一行
            line_txt.text(line_offset..)
        } else {
            line_txt.text(last_offset..)
        };
        let len = txt.text_len();
        f(
            txt.get_data(),
            LineState::new(
                current_width,
                len,
                char_count - char_index, //计算char 个数
                //    get_page_number!(*line_num, height),
                line_str.get_block_id(),         //行所在块编号
                line_str.get_block_line_index(), //行所在块行索引
                line_str.get_block_offset(),     //行所在块偏移
                //   *line_num,
                line_index,
                line_offset,
                line_str.get_line_file_start(),
                line_str.get_line_file_end(),
            ),
        );
    }

    // 软换行
    fn sort_warp_desc<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        // line_num: &mut usize,
        line_count: usize,
        // page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, LineState),
    {
        if line_start > 0 {
            // 反向迭代
            let line_txt = line_str.text(..line_start);
            let mut current_width = 0; // 当前行宽度
            let mut line_offset = 0; //当前行偏移量
            let mut current_bytes = 0; //当前行字节数
            let mut char_index = 0; // 当前行字符索引
            let mut char_count = 0; // 当前行字符数
            let data = line_txt.get_data();
            // 此处 line_start > 0 已由外层 if 保证，直接反向迭代
            for (i, (_, ch)) in data.char_indices().rev().enumerate() {
                let ch_width = ch.width().unwrap_or(0);
                //检查是否超过屏幕宽度
                if current_width + ch_width > with {
                    let end = (line_offset + current_bytes).min(line_txt.text_len());
                    // *line_num = (*line_num).saturating_sub(1); //行数减1
                    if true {
                        *cur_line_count += 1;
                        let txt = line_txt.text((line_txt.text_len() - end)..);
                        let len: usize = txt.text_len();
                        // line_start > 0 由外层 if 保证
                        let meta_line_offset = line_start - (line_offset + current_bytes);
                        f(
                            txt.get_data(),
                            LineState::new(
                                current_width,
                                len,
                                char_count - char_index,
                                //get_page_number!(*line_num, height),
                                line_str.get_block_id(),
                                line_str.get_block_line_index(),
                                line_str.get_block_offset(),
                                // *line_num,
                                line_index,
                                meta_line_offset,
                                line_str.get_line_file_start(),
                                line_str.get_line_file_end(),
                            ),
                        );
                    }
                    if *cur_line_count >= line_count {
                        return;
                    }
                    char_index = i;
                    line_offset += current_bytes;
                    current_width = 0;
                    current_bytes = 0;
                }
                char_count += 1;
                current_width += ch_width;
                current_bytes += util::get_char_byte_len(ch);
            }
            //当前行没有到达屏幕宽度 但还是一行
            if current_bytes > 0 {
                //*line_num = (*line_num).saturating_sub(1); //行数减1
                if true {
                    let txt = line_txt.text(..);
                    *cur_line_count += 1;
                    let len = txt.text_len();
                    // line_start > 0 由外层 if 保证
                    let meta_line_offset = line_start - (line_offset + current_bytes);

                    f(
                        txt.get_data(),
                        LineState::new(
                            current_bytes,
                            len,
                            char_count - char_index,
                            //get_page_number!(*line_num, height),
                            line_str.get_block_id(),
                            line_str.get_block_line_index(),
                            line_str.get_block_offset(),
                            // *line_num,
                            line_index,
                            meta_line_offset,
                            line_str.get_line_file_start(),
                            line_str.get_line_file_end(),
                        ),
                    );
                }
                if *cur_line_count >= line_count {
                    return;
                }
            }
        } else {
            Self::get_last_sort_warp_line(
                line_str,
                line_index,
                line_start,
                with,
                height,
                text_index,
                // line_num,
                line_count,
                // page_num,
                cur_line_count,
                skip_line,
                is_rev,
                f,
            )
        };
    }

    // 软换行
    fn sort_warp_asc<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        // line_num: &mut usize,
        line_count: usize,
        // page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, LineState),
    {
        let line_txt = line_str.text(line_start..);

        let mut current_width = 0; // 当前行宽度
        let mut line_offset = 0; //当前行偏移量
        let mut current_bytes = 0; //当前行字节数
        let mut char_index = 0; // 当前行字符索引
        let mut char_count = 0; // 当前行字符数

        for (i, (byte_index, ch)) in line_txt.get_data().char_indices().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            //检查是否超过屏幕宽度
            if current_width + ch_width > with {
                let end = (line_offset + current_bytes).min(line_txt.text_len());
                //*line_num += 1; //行数加1
                if true {
                    *cur_line_count += 1;
                    let txt = line_txt.text(line_offset..end);
                    let len: usize = txt.text_len();
                    let meta_line_offset = line_start + line_offset;
                    f(
                        txt.get_data(),
                        LineState::new(
                            current_width,
                            len,
                            i - char_index,
                            // get_page_number!(*line_num, height),
                            line_str.get_block_id(),
                            line_str.get_block_line_index(),
                            line_str.get_block_offset(),
                            // *line_num,
                            line_index,
                            meta_line_offset,
                            line_str.get_line_file_start(),
                            line_str.get_line_file_end(),
                        ),
                    );
                }
                // if *line_num % height == 0 {
                //     //到达一页
                //     *page_num += 1; //页数加1
                //     let state = LineState::builder()
                //         .line_index(line_index)
                //         .line_offset(line_start + byte_index)
                //         .block_num(0) //行所在块编号
                //         .block_line_index(0) //行在块内的行号
                //         .block_offset(0) //行所在块偏移
                //         .line_file_start(line_str.get_line_file_start())
                //         .start_line_num(0)
                //         .start_page_num(0)
                //         .build();
                //     text_index.set_page_offset(*page_num, state);
                //     // let m = *page_num / PAGE_GROUP;
                //     // let n = *page_num % PAGE_GROUP;
                //     // if n == 0 && m > page_offset_list.len() - 1 {
                //     //     //保存页数的偏移量
                //     //     page_offset_list.push(PageOffset {
                //     //         line_index,
                //     //         line_offset: line_start + byte_index,
                //     //         line_file_start: line_str.line_file_start,
                //     //         start_line_num: 0,
                //     //         start_page_num: 0,
                //     //     });
                //     // }
                // }
                if *cur_line_count >= line_count {
                    return;
                }
                char_index = i;
                line_offset += current_bytes;
                current_width = 0;
                current_bytes = 0;
            }
            char_count += 1;
            current_width += ch_width;
            current_bytes += util::get_char_byte_len(ch);
        }
        //当前行没有到达屏幕宽度 但还是一行
        if current_bytes > 0 {
            // *line_num += 1; //行数加1
            if true {
                let txt = line_txt.text(line_offset..);
                *cur_line_count += 1;
                let len = txt.text_len();
                let meta_line_offset = line_start + line_offset;
                f(
                    txt.get_data(),
                    LineState::new(
                        current_width,
                        len,
                        char_count - char_index,
                        //get_page_number!(*line_num, height),
                        line_str.get_block_id(),
                        line_str.get_block_line_index(),
                        line_str.get_block_offset(),
                        //  *line_num,
                        line_index,
                        meta_line_offset,
                        line_str.get_line_file_start(),
                        line_str.get_line_file_end(),
                    ),
                );
            }
            // if *line_num % height == 0 {
            //     *page_num += 1; //页数加1
            //     let state = LineState::builder()
            //         .line_index(line_index + 1)
            //         .line_offset(0)
            //         .block_num(0) //行所在块编号
            //         .block_line_index(0) //行在块内的行号
            //         .block_offset(0) //行所在块偏移
            //         .line_file_start(line_str.get_line_file_end())
            //         .start_line_num(0)
            //         .start_page_num(0)
            //         .build();
            //     text_index.set_page_offset(*page_num, state);

            //     // let m = *page_num / PAGE_GROUP;
            //     // let n = *page_num % PAGE_GROUP;
            //     // if n == 0 && m > page_offset_list.len() - 1 {
            //     //     //保存页数的偏移量
            //     //     page_offset_list.push(PageOffset {
            //     //         line_index: line_index + 1,
            //     //         line_offset: 0,
            //     //         line_file_start: line_str.line_file_end,
            //     //         start_line_num: 0,
            //     //         start_page_num: 0,
            //     //     });
            //     // }
            // }
            if *cur_line_count >= line_count {
                return;
            }
        }
    }

    fn sort_warp<'a, F, I: TextIndex, L: Line<'a> + 'a>(
        line_str: L,
        line_index: usize,
        line_offset: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        //  line_num: &mut usize,
        line_count: usize,
        // page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, LineState),
    {
        if is_rev {
            Self::sort_warp_desc(
                line_str,
                line_index,
                line_offset,
                with,
                height,
                text_index,
                //line_num,
                line_count,
                //page_num,
                cur_line_count,
                skip_line,
                is_rev,
                f,
            );
        } else {
            Self::sort_warp_asc(
                line_str,
                line_index,
                line_offset,
                with,
                height,
                text_index,
                // line_num,
                line_count,
                //page_num,
                cur_line_count,
                skip_line,
                f,
            );
        }
    }

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        self.borrow_lines().text_from_sel(sel)
    }

    fn search(&self, pattern: &[u8], line_state: &LineState) -> ChapResult<Option<Vec<LineState>>> {
        self.borrow_lines_mut().search(pattern, line_state)
    }
}

pub(crate) struct EditTextWarp<T: Text + TextIndex + EditText> {
    edit_text: TextWarp<T>,
}

impl<T: Text + TextIndex + EditText> EditTextWarp<T> {
    pub(crate) fn new(
        lines: T,
        height: usize,
        with: usize,
        text_warp_type: TextWarpType,
    ) -> EditTextWarp<T> {
        EditTextWarp {
            edit_text: TextWarp::new(lines, height, with, text_warp_type),
        }
    }

    // pub(crate) fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
    //     self.edit_text.get_one_page(line_num)
    // }

    pub(crate) fn get_one_page_from_line_state(
        &self,
        line_state: &LineState,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        self.edit_text.get_one_page_from_line_state(line_state)
    }

    pub(crate) fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<LineState>)> {
        self.edit_text.get_current_page()
    }

    pub(crate) fn get_current_line_meta(&self) -> ChapResult<&RingVec<LineState>> {
        self.edit_text.get_current_line_meta()
    }

    pub(crate) fn get_file_size(&self) -> usize {
        self.edit_text.get_file_size()
    }

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(&self, meta: &LineState) -> ChapResult<()> {
        self.edit_text.scroll_next_one_line(meta)
    }

    // /**
    //  * 滚动上一行
    //  */
    // pub(crate) fn scroll_pre_one_line(&self, meta: &LineState) -> ChapResult<()> {
    //     self.edit_text.scroll_pre_one_line(meta)
    // }

    pub(crate) fn scroll_pre_one_line2(&self, meta: &LineState) -> ChapResult<()> {
        self.edit_text.scroll_pre_one_line2(meta)
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        self.edit_text.get_text_len(index)
    }

    pub(crate) fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        self.edit_text.get_text_from_sel(sel)
    }

    pub(crate) fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize> {
        self.edit_text.find(pattern, line_file_start)
    }

    pub(crate) fn ensure_block_loaded(&self, block_num: usize) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .ensure_block_loaded(block_num)
    }

    pub(crate) fn search(
        &self,
        pattern: &[u8],
        line_state: &LineState,
    ) -> ChapResult<Option<Vec<LineState>>> {
        self.edit_text.search(pattern, line_state)
    }

    // 插入字符
    // 计算光标所在行
    // 计算光标所在列
    pub(crate) fn insert_char(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .insert_char(cursor_y, bytes_cursor, line_meta, c)?;
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    pub(crate) fn insert_bytes(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        bytes: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()> {
        self.edit_text.borrow_lines_mut().insert_bytes(
            cursor_y,
            bytes_cursor,
            line_meta,
            bytes,
            is_overwrite,
        )?;
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    //插入换行
    pub(crate) fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .insert_newline(cursor_y, cursor_x, line_meta)?;
        // todo
        // let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    pub(crate) fn delete_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .delete_line(cursor_y, cursor_x, line_meta)?;
        // todo
        // let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    // 删除光标前一个字符
    pub(crate) fn backspace(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>> {
        let delete_bytes = self.edit_text.borrow_lines_mut().backspace(
            cursor_y,
            bytes_cursor,
            count,
            line_meta,
        )?;
        // todo
        //let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        //unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(delete_bytes)
    }

    pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.edit_text.borrow_lines_mut().save(filepath)
    }
}

impl EditTextWarp<GapBlockText> {
    pub(crate) fn resolve_block_for_file_offset(
        &self,
        file_offset: usize,
    ) -> Option<(usize, usize)> {
        self.edit_text
            .borrow_lines()
            .resolve_block_for_file_offset(file_offset)
    }

    pub(crate) fn find_block_line_for_offset(
        &self,
        block_id: usize,
        abs_byte: usize,
    ) -> Option<usize> {
        self.edit_text
            .borrow_lines_mut()
            .find_block_line_for_offset(block_id, abs_byte)
    }

    #[cfg(test)]
    pub(crate) fn assert_block_storage_consistent_for_test(&self) {
        self.edit_text
            .borrow_lines()
            .assert_storage_consistent_for_test();
    }
}

#[cfg(test)]
impl CacheStr {
    /// 从 Vec<u8> 构造 CacheStr，仅用于测试
    pub(crate) fn from_vec_for_test(data: Vec<u8>) -> Self {
        CacheStr::Vec(VecCache::from_vec(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap_block_line<'a>(left: &'a [u8], right: &'a [u8]) -> BlockLineData<'a> {
        BlockLineData {
            data: LineData::GapBytes(GapBytes::new(left, right)),
            block_file_start: 0,
            block_id: 0,
            block_line_index: 0,
            block_offset: 0,
        }
    }

    #[test]
    fn test_line_block_str_u8_iter_yields_block1_then_block2_bytes() {
        let mut iter = LineBlockStrU8Iter {
            block1_iter: Some(LineDataU8Iter::U8Iter(b"ab".iter())),
            block2_iter: Some(LineDataU8Iter::U8Iter(b"cd".iter())),
        };

        let collected: Vec<u8> = iter.by_ref().collect();
        assert_eq!(collected, b"abcd");
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn test_line_block_str_search_finds_match_in_single_slice() {
        let line = LineBlockStr(Some(gap_block_line(b"alpha needle omega", b"")), None);

        assert_eq!(line.search(b"needle"), vec![6]);
    }

    #[test]
    fn test_line_block_str_search_finds_match_across_two_slices() {
        let line = LineBlockStr(Some(gap_block_line(b"alpha nee", b"dle omega")), None);

        assert_eq!(line.search(b"needle"), vec![6]);
    }

    #[test]
    fn test_line_block_str_search_offsets_match_in_second_block() {
        let line = LineBlockStr(
            Some(gap_block_line(b"alpha ", b"")),
            Some(gap_block_line(b"needle omega", b"")),
        );

        assert_eq!(line.search(b"needle"), vec![6]);
    }

    #[test]
    fn test_line_block_str_search_finds_match_across_block_boundary() {
        let line = LineBlockStr(
            Some(gap_block_line(b"alpha nee", b"")),
            Some(gap_block_line(b"dle omega", b"")),
        );

        assert_eq!(line.search(b"needle"), vec![6]);
    }

    #[test]
    fn test_line_block_str_search_finds_non_overlapping_matches_across_four_slices() {
        let line = LineBlockStr(
            Some(gap_block_line(b"xxne", b"edle yy ")),
            Some(gap_block_line(b"needle zz ne", b"edle")),
        );

        assert_eq!(line.search(b"needle"), vec![2, 12, 22]);
    }

    #[test]
    fn test_line_block_str_search_returns_empty_for_empty_or_missing_key() {
        let line = LineBlockStr(Some(gap_block_line(b"alpha", b" beta")), None);

        assert_eq!(line.search(b""), Vec::<usize>::new());
        assert_eq!(line.search(b"needle"), Vec::<usize>::new());
    }
}
