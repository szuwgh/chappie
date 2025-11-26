pub(crate) mod edit;
pub(crate) mod edit_block;
pub(crate) mod hex;
pub(crate) mod text;
use crate::common::error::ChapResult;
use crate::common::gap_buffer::GapBuffer;
use crate::common::gap_buffer::GapBytes;
use crate::common::gap_buffer::GapBytesBlockCharIter;
use crate::common::gap_buffer::GapBytesCharIter;
use crate::common::ring_vec::RingVec;
use crate::common::util;
use crate::fuzzy::boyermoore::BoyerMoore;
use crate::textwarp::edit::GapText;
use crate::textwarp::hex::HexText;
use crate::textwarp::text::MmapText;
use inherit_methods_macro::inherit_methods;
use mlua::Either;
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::ops::Bound;
use std::path::Path;
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct PageOffset {
    line_index: usize,      //第多少行
    line_offset: usize,     //行在总行的起始位置
    line_file_start: usize, //这一行在整个文件的起始位置
    start_line_num: usize,  //开始的行数
    start_page_num: usize,  //这一行在第几页开始
}

impl PageOffset {
    pub(crate) fn new() -> Self {
        PageOffset {
            line_index: 0,
            line_offset: 0,
            line_file_start: 0,
            start_line_num: 0,
            start_page_num: 0,
        }
    }
}

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
                let (l1, r1) = v1.text(&range);
                let (l2, r2) = v2.text(&range);
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
            LineDataCharIter::GapBlockCharIter(iter) => todo!(),
        }
    }
}

pub enum LineData<'a> {
    Own(Vec<u8>),
    Bytes(&'a [u8]),
    GapBytes(GapBytes<'a>),
    GapBlockBytes(GapBytes<'a>, GapBytes<'a>),
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

pub struct LineParts<T: LineDefault> {
    pub(crate) data: [T; 4],
    pub(crate) length: usize,
}

impl<T: LineDefault> LineParts<T> {
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

    fn as_slice(&self) -> LineParts<&[u8]> {
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

        // match self {
        //     LineData::Bytes(v) => (v, &[]),
        //     LineData::GapBytes(v) => v.as_slice(),
        //     LineData::Own(v) => (v.as_slice(), &[]),
        // }
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
                GapBytesBlockCharIter::new(v1.char_indices(), v2.char_indices()),
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
            LineData::Own(v) => LineData::Own(v[start..end].to_vec()),
            LineData::Bytes(v) => LineData::Bytes(&v[start..end]),
            LineData::GapBytes(v) => LineData::GapBytes(v.text(range)),
            LineData::GapBlockBytes(v1, v2) => {
                LineData::GapBlockBytes(v1.text(range.clone()), v2.text(range))
            }
        }
    }
}

pub struct LineStr<'a> {
    pub(crate) line_data: LineData<'a>, //行数据
    pub(crate) line_file_start: usize,  //行在文件开始位置
    pub(crate) line_file_end: usize,    //行在文件结束的位置
}

impl<'a> Line<'a> for LineStr<'a> {
    fn text_len(&self) -> usize {
        self.line_data.len()
    }
    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> LineData<'a> {
        self.line_data.text(range)
    }

    fn get_line_file_start(&self) -> usize {
        self.line_file_start
    }

    fn get_line_file_end(&self) -> usize {
        self.line_file_end
    }
}

impl<'a> LineStr<'a> {
    fn empty() -> LineStr<'a> {
        LineStr {
            line_data: LineData::empty(),
            line_file_start: 0,
            line_file_end: 0,
        }
    }
}

//一行数据 这行数据可能是在两个块中
pub(crate) struct LineBlockStr<'a>([LineStr<'a>; 2]);

impl<'a> Line<'a> for LineBlockStr<'a> {
    fn text_len(&self) -> usize {
        self.0[0].line_data.len() + self.0[1].line_data.len()
    }
    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> LineData<'a> {
        match (&self.0[0].line_data, &self.0[1].line_data) {
            (LineData::GapBytes(v1), LineData::GapBytes(v2)) => {
                LineData::GapBlockBytes(v1.text(range.clone()), v2.text(range))
            }
            _ => {
                panic!("LineBlockStr 只能是 GapBytes 类型");
            }
        }
    }

    fn get_line_file_start(&self) -> usize {
        self.0[0].line_file_start
    }

    fn get_line_file_end(&self) -> usize {
        self.0[1].line_file_end
    }
}

pub(crate) trait TextOper {
    //滑动上一行
    fn scroll_pre_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    //滑动下一行
    fn scroll_next_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    //插入
    fn insert(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) -> ChapResult<()>;

    //
    fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()>;

    fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        count: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()>;

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()>;

    //获取一页数据 从line_num 行开始
    fn get_one_page(
        &self,
        line_num: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)>;

    fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)>;

    fn get_current_line_meta(&self) -> ChapResult<&RingVec<EditLineMeta>>;

    fn get_text_len_from_index(&self, line_index: usize) -> usize;

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8>;

    fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize>;

    fn get_file_size(&self) -> usize;
}

pub(crate) trait Line<'a> {
    fn text_len(&self) -> usize;
    fn text(&self, range: impl std::ops::RangeBounds<usize> + Clone) -> LineData<'a>;
    fn get_line_file_start(&self) -> usize;
    fn get_line_file_end(&self) -> usize;
}

pub(crate) trait Text {
    type LineItem<'a>: Line<'a>
    where
        Self: 'a;
    fn get_file_size(&self) -> usize;

    //是否有下一行
    fn has_next_line(&self, meta: &EditLineMeta) -> bool;

    //获取一行
    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> Self::LineItem<'a>;

    //获取行的文本长度
    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize;

    //获取选中文本
    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8>;

    //迭代一行数据
    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = Self::LineItem<'a>>;

    //反向迭代
    fn iter_rev<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = Self::LineItem<'a>>;

    //迭代字节数据
    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8>;
}

pub(crate) trait TextIndex {
    fn get_page_offset(&self, line_num: usize) -> PageOffset;
    fn set_page_offset(&mut self, page_num: usize, page_offset: PageOffset);
}

pub(crate) trait EditText {
    // cursor_y: 行号 从0开始
    // bytes_cursor: 字节偏移 从0开始
    // line_meta: 当前行的元信息
    fn insert(&mut self, cursor_y: usize, bytes_cursor: usize, line_meta: &EditLineMeta, c: char);
    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta);
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &EditLineMeta,
    );
    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()>;
}

#[derive(Debug, Default)]
pub(crate) struct EditLineMeta {
    txt_len: usize,         //文本长度
    char_len: usize,        //char字符大小
    page_num: usize,        //所在页数 从1开始
    line_num: usize,        //行数 从1开始  这个代表实际行号 不是索引 代表视觉上的行号
    line_index: usize,      //行号 从0开始 这个代表一行一行数据 用 '\n' 分隔的行号
    line_offset: usize,     //这一行在总行的偏移量位置
    line_file_start: usize, //行在文件开始位置
    line_file_end: usize,   //行在文件结束的位置
}

impl EditLineMeta {
    pub(crate) fn new(
        txt_len: usize,
        char_len: usize,
        page_num: usize,
        line_num: usize,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> EditLineMeta {
        EditLineMeta {
            txt_len,
            char_len,
            page_num,
            line_num,
            line_index,
            line_offset,
            line_file_start: line_file_start,
            line_file_end: line_file_end,
        }
    }

    pub(crate) fn get_hex_len(&self) -> usize {
        (self.txt_len * 3 + self.txt_len / 8).saturating_sub(1)
    }

    pub(crate) fn get_line_num(&self) -> usize {
        self.line_num
    }

    pub(crate) fn get_page_num(&self) -> usize {
        self.page_num
    }

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

    pub(crate) fn get_line_file_start(&self) -> usize {
        self.line_file_start
    }

    pub(crate) fn get_line_file_end(&self) -> usize {
        self.line_file_end
    }
}

pub(crate) enum TextDisplay {
    Text(TextWarp<MmapText>),
    Hex(TextWarp<HexText>),
    Edit(EditTextWarp<GapText>),
}

impl TextOper for TextDisplay {
    fn get_current_page(&self) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)> {
        match self {
            TextDisplay::Text(v) => v.get_current_page(),
            TextDisplay::Hex(v) => v.get_current_page(),
            TextDisplay::Edit(v) => v.get_current_page(),
        }
    }

    fn find(&self, pattern: &[u8], line_file_start: usize) -> Option<usize> {
        match self {
            TextDisplay::Text(v) => v.find(pattern, line_file_start),
            TextDisplay::Hex(v) => v.find(pattern, line_file_start),
            TextDisplay::Edit(v) => todo!(),
        }
    }

    fn get_file_size(&self) -> usize {
        match self {
            TextDisplay::Text(v) => v.get_file_size(),
            TextDisplay::Hex(v) => v.get_file_size(),
            TextDisplay::Edit(v) => todo!(),
        }
    }

    fn get_current_line_meta(&self) -> ChapResult<&RingVec<EditLineMeta>> {
        match self {
            TextDisplay::Text(v) => v.get_current_line_meta(),
            TextDisplay::Hex(v) => v.get_current_line_meta(),
            TextDisplay::Edit(v) => v.get_current_line_meta(),
        }
    }

    fn get_text_len_from_index(&self, line_index: usize) -> usize {
        match self {
            TextDisplay::Text(v) => v.get_text_len(line_index),
            TextDisplay::Hex(v) => v.get_text_len(line_index),
            TextDisplay::Edit(v) => v.get_text_len(line_index),
        }
    }

    fn insert(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.insert(cursor_y, bytes_cursor, line_meta, c),
        }
    }

    fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.insert_newline(cursor_y, cursor_x, line_meta),
        }
    }

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.save(filepath),
        }
    }

    fn scroll_next_one_line(&self, meta: &EditLineMeta) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => v.scroll_next_one_line(meta),
            TextDisplay::Hex(v) => v.scroll_next_one_line(meta),
            TextDisplay::Edit(v) => v.scroll_next_one_line(meta),
        }
    }

    fn scroll_pre_one_line(&self, meta: &EditLineMeta) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => v.scroll_pre_one_line(meta),
            TextDisplay::Hex(v) => v.scroll_pre_one_line(meta),
            TextDisplay::Edit(v) => v.scroll_pre_one_line2(meta),
        }
    }

    fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        count: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.backspace(cursor_y, cursor_x, count, line_meta),
        }
    }

    fn get_one_page(
        &self,
        line_num: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)> {
        match self {
            TextDisplay::Text(v) => v.get_one_page(line_num),
            TextDisplay::Hex(v) => v.get_one_page(line_num),
            TextDisplay::Edit(v) => v.get_one_page(line_num),
        }
    }

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        match self {
            TextDisplay::Text(v) => v.get_text_from_sel(sel),
            TextDisplay::Hex(v) => v.get_text_from_sel(sel),
            TextDisplay::Edit(v) => todo!("Not implement get_text_from_sel for EditTextWarp"),
        }
    }
}

pub(crate) struct TextWarp<T: Text + TextIndex> {
    lines: UnsafeCell<T>,                               // 文本
    cache_lines: UnsafeCell<RingVec<CacheStr>>,         // 缓存行
    cache_line_meta: UnsafeCell<RingVec<EditLineMeta>>, // 缓存行元数据
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

    fn borrow_cache_line_meta(&self) -> &RingVec<EditLineMeta> {
        unsafe { &*self.cache_line_meta.get() }
    }

    fn borrow_cache_line_meta_mut(&self) -> &mut RingVec<EditLineMeta> {
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
    pub(crate) fn scroll_pre_one_line2(&self, meta: &EditLineMeta) -> ChapResult<()> {
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
        meta: &EditLineMeta,
        line_count: usize,
        is_rev: bool,
    ) -> (Option<CacheStr>, EditLineMeta) {
        if is_rev {
            self.get_pre_line2(meta, line_count)
        } else {
            self.get_next_line(meta, line_count)
        }
    }

    //从当前行开始获取前面n行
    pub(crate) fn get_pre_line2<'a>(
        &self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        assert!(meta.get_line_num() >= 1);

        if meta.get_line_num() == 1 {
            return (None, EditLineMeta::default());
        }

        let mut line_index = meta.get_line_index();
        let mut line_offset = meta.get_line_offset();
        let mut line_file_start = meta.get_line_file_start();
        let mut start_line_num = meta.get_line_num();
        let mut start_page_num = meta.get_page_num();

        //这行已经是最开始的一行了
        if meta.get_line_offset() == 0 {
            line_index = meta.get_line_index().saturating_sub(1);
            line_offset = 0;
            line_file_start = 0;
            start_line_num = start_line_num;
            start_page_num = 0;
        }

        let mut s = LineData::empty();
        let mut m = EditLineMeta::default();
        let page_offset = PageOffset {
            line_index,
            line_offset: line_offset,
            line_file_start: line_file_start,
            start_line_num: start_line_num,
            start_page_num: start_page_num,
        };

        self.get_char_text_fn(
            &page_offset,
            line_count,
            page_offset.start_line_num,
            page_offset.start_page_num,
            0,
            true,
            &mut |txt, m1| {
                s = txt;
                m = m1;
            },
        );
        (Some(CacheStr::from_data(s)), m)
    }

    //从当前行开始获取前面n行
    pub(crate) fn get_pre_line<'a>(
        &'a self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        assert!(meta.get_line_num() >= 1);

        if meta.get_line_num() == 1 {
            return (None, EditLineMeta::default());
        }
        let mut s = LineData::empty();
        let mut m = EditLineMeta::default();
        self.get_text_from_line_num(meta.get_line_num() - line_count, line_count, |txt, meta| {
            s = txt;
            m = meta;
        });
        (Some(CacheStr::from_data(s)), m)
    }

    //从当前行开始获取后面n行
    pub(crate) fn get_next_line<'a>(
        &'a self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();
        let mut line_file_start = meta.get_line_file_start();
        if !self.borrow_lines().has_next_line(meta) {
            return (None, EditLineMeta::default());
        }

        let line =
            self.borrow_lines_mut()
                .get_line(line_index, meta.line_file_start, meta.line_file_end); //&self.borrow_lines()[line_index];

        //这行已经读完 开始下一行
        if line_end == line.text_len() {
            line_file_start = meta.get_line_file_end();
            line_end = 0;
            line_index += 1;
        }

        let p = PageOffset {
            line_index: line_index,
            line_offset: line_end,
            line_file_start: line_file_start,
            start_line_num: 0,
            start_page_num: 0,
        };
        let mut s = LineData::empty();
        let mut m = EditLineMeta::default();
        let start_page_num = meta.get_line_num() / self.height;
        self.get_char_text_fn(
            &p,
            line_count,
            meta.get_line_num(),
            start_page_num,
            0,
            false,
            &mut |x, m1| {
                s = x;
                m = m1;
            },
        );
        (Some(CacheStr::from_data(s)), m)
    }

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(&self, meta: &EditLineMeta) -> ChapResult<()> {
        let (s, l) = self.get_next_line(meta, 1);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push(s);
            self.borrow_cache_line_meta_mut().push(l);
        }
        Ok(())
    }

    /**
     * 滚动上一行
     */
    pub(crate) fn scroll_pre_one_line(&self, meta: &EditLineMeta) -> ChapResult<()> {
        let (s, l) = self.get_pre_line(meta, 1);
        if let Some(s) = s {
            self.borrow_cache_lines_mut().push_front(s);
            self.borrow_cache_line_meta_mut().push_front(l);
        }
        Ok(())
    }

    pub(crate) fn get_one_page(
        &self,
        line_num: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)> {
        self.get_line_content(line_num, self.height)
    }

    pub(crate) fn get_current_page(
        &self,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)> {
        Ok((self.borrow_cache_lines(), self.borrow_cache_line_meta()))
    }

    pub(crate) fn get_current_line_meta(&self) -> ChapResult<&RingVec<EditLineMeta>> {
        Ok(self.borrow_cache_line_meta())
    }

    // 从第n行开始获取内容
    pub(crate) fn get_line_content(
        &self,
        line_num: usize,
        line_count: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)> {
        self.borrow_cache_lines_mut().clear();
        self.borrow_cache_line_meta_mut().clear();

        self.get_text_from_line_num(line_num, line_count, |txt, meta| {
            self.borrow_cache_lines_mut().push(CacheStr::from_data(txt));
            self.borrow_cache_line_meta_mut().push(meta);
        });

        Ok((self.borrow_cache_lines(), self.borrow_cache_line_meta()))
    }

    pub(crate) fn get_line_content_with_count(
        &self,
        line_num: usize,
        line_count: usize,
    ) -> (Vec<CacheStr>, Vec<EditLineMeta>) {
        let mut lines = Vec::new();
        let mut lines_meta = Vec::new();
        self.get_text_from_line_num(line_num, line_count, |txt, meta| {
            lines.push(CacheStr::from_data(txt));
            lines_meta.push(meta);
        });
        (lines, lines_meta)
    }

    fn get_text_from_line_num<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        assert!(line_num >= 1);
        // // 计算页码
        // let page_num = self.get_page_num(line_num);
        // // 计算页码
        // let mut index = (page_num - 1) / PAGE_GROUP;
        // let page_offset_list = self.borrow_page_offset_list();
        // let page_offset = if index >= page_offset_list.len() {
        //     index = page_offset_list.len() - 1;
        //     page_offset_list.last().unwrap()
        // } else {
        //     &page_offset_list[index]
        // };
        // let start_page_num = index * PAGE_GROUP;

        let page_offset = self.borrow_lines().get_page_offset(line_num);

        assert!(line_num >= page_offset.start_line_num);
        //跳过的行数
        let skip_line = line_num;
        // println!("skip_line:{}", skip_line);
        // println!("page_offset:{:?}", page_offset);
        self.get_char_text_fn(
            &page_offset,
            line_count,
            page_offset.start_line_num,
            page_offset.start_page_num,
            skip_line,
            false,
            &mut f,
        );
    }

    // fn get_hex_text_fn<'a, F>(
    //     &'a self,
    //     page_offset: &PageOffset,
    //     line_count: usize,
    //     start_line_num: usize,
    //     start_page_num: usize,
    //     skip_line: usize,
    //     f: &mut F,
    // ) where
    //     // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
    //     F: FnMut(&'a [u8], EditLineMeta),
    // {
    //     todo!()
    // }

    fn get_char_text_fn<'a, F>(
        &'a self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        let mut cur_line_count = 0;
        let mut line_num = start_line_num;
        let mut page_num = start_page_num;

        let iter = if is_rev {
            Either::Left(self.borrow_lines_mut().iter_rev(
                page_offset.line_index,
                page_offset.line_offset,
                page_offset.line_file_start,
            ))
        } else {
            Either::Right(self.borrow_lines_mut().iter(
                page_offset.line_index,
                page_offset.line_offset,
                page_offset.line_file_start,
            ))
        };

        for (i, v) in iter.enumerate() {
            let line_offset = if i == 0 { page_offset.line_offset } else { 0 };
            let line_index = if is_rev {
                page_offset.line_index.saturating_sub(i)
            } else {
                page_offset.line_index + i
            };
            Self::set_line_char_txt(
                v,
                line_index,
                line_offset,
                self.with,
                self.height,
                self.borrow_lines_mut(),
                &mut line_num,
                line_count,
                &mut page_num,
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

    fn set_line_char_txt<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,                   // 行内容
        line_index: usize,             // 行索引
        line_start: usize,             // 行起始位置
        with: usize,                   // 每行宽度
        height: usize,                 // 每页行数
        text_index: &mut I,            // 文本索引
        line_num: &mut usize,          // 当前行号 (视觉)
        line_count: usize,             // 需要获取的行数
        page_num: &mut usize,          // 当前页码
        cur_line_count: &mut usize,    // 已获取的行数
        skip_line: usize,              // 跳过的行数
        text_warp_type: &TextWarpType, // 文本换行类型
        is_rev: bool,                  // 是否是反向读取
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        //空行

        let line_txt = if is_rev && line_start > 0 {
            line_str.text(..line_start)
        } else {
            line_str.text(line_start..)
        };
        if line_txt.len() == 0 {
            if is_rev {
                *line_num = (*line_num).saturating_sub(1);
            } else {
                *line_num += 1; //行数加1
            }
            if *line_num >= skip_line {
                *cur_line_count += 1;
                f(
                    LineData::empty(),
                    EditLineMeta::new(
                        0,
                        0,
                        get_page_number!(*line_num, height),
                        *line_num,
                        line_index,
                        0,
                        line_str.get_line_file_start(),
                        line_str.get_line_file_end(),
                    ),
                );
            }
            if *line_num % height == 0 && !is_rev {
                //到达一页
                *page_num += 1; //页数加1
                                //let m = *page_num / PAGE_GROUP;
                                // let n = *page_num % PAGE_GROUP;
                text_index.set_page_offset(
                    *page_num,
                    PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.get_line_file_end(),
                        start_line_num: 0,
                        start_page_num: 0,
                    },
                );
                // if n == 0 && m > page_offset_list.len() - 1 {
                //     //保存页数的偏移量
                //     page_offset_list.push(PageOffset {
                //         line_index: line_index + 1,
                //         line_offset: 0,
                //         line_file_start: line_str.line_file_end,
                //         start_line_num: 0,
                //         start_page_num: 0,
                //     });
                // }
            }

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
                    line_start,
                    with,
                    height,
                    text_index,
                    line_num,
                    line_count,
                    page_num,
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
                    line_start,
                    with,
                    height,
                    text_index,
                    line_num,
                    line_count,
                    page_num,
                    cur_line_count,
                    skip_line,
                    is_rev,
                    f,
                );
            }
        }
    }

    fn no_warp<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        let line_txt = line_str.text(line_start..);
        if is_rev {
            *line_num = (*line_num).saturating_sub(1); //行数减1
        } else {
            *line_num += 1; //行数加1
        }
        if *line_num >= skip_line {
            *cur_line_count += 1;
            let len = line_txt.len();
            let char_len = line_txt.char_indices().count();
            f(
                line_txt,
                EditLineMeta::new(
                    len,
                    char_len,
                    *page_num + 1,
                    *line_num,
                    line_index,
                    line_start + 0,
                    line_str.get_line_file_start(),
                    line_str.get_line_file_end(),
                ),
            );
        }
        if *line_num % height == 0 && !is_rev {
            //到达一页
            *page_num += 1; //页数加1

            text_index.set_page_offset(
                *page_num,
                PageOffset {
                    line_index,
                    line_offset: line_start + 0,
                    line_file_start: line_str.get_line_file_end(),
                    start_line_num: 0,
                    start_page_num: 0,
                },
            );

            // let m = *page_num / PAGE_GROUP;
            // let n = *page_num % PAGE_GROUP;
            // if n == 0 && m > page_offset_list.len() - 1 {
            //     //保存页数的偏移量
            //     page_offset_list.push();
            // }
        }
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
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        let line_txt = line_str.text(line_start..);
        let mut current_width = 0; // 当前行宽度
        let mut line_offset = 0; //当前行偏移量
        let mut char_index = 0; // 当前行字符索引
        let mut char_count = 0; // 当前行字符数
        let mut last_offset = 0;
        for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
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
        *line_num = (*line_num).saturating_sub(1); //行数减1
        let txt = if current_width > 0 {
            //当前行没有到达屏幕宽度 但还是一行 这里就是最后一行
            line_txt.text(line_offset..)
        } else {
            line_txt.text(last_offset..)
        };
        let len = txt.len();
        f(
            txt,
            EditLineMeta::new(
                len,
                char_count - char_index, //计算char 个数
                get_page_number!(*line_num, height),
                *line_num,
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
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        if line_start > 0 {
            // 反向迭代
            let line_txt = line_str.text(..line_start);
            let mut current_width = 0; // 当前行宽度
            let mut line_offset = 0; //当前行偏移量
            let mut current_bytes = 0; //当前行字节数
            let mut char_index = 0; // 当前行字符索引
            let mut char_count = 0; // 当前行字符数

            let iter = if line_start > 0 {
                Either::Left(line_txt.char_indices().rev().enumerate())
            } else {
                Either::Right(line_txt.char_indices().enumerate()) //要正向迭代 取出最后一行的数据 才是正确的
            };

            for (i, (byte_index, ch)) in iter {
                let ch_width = ch.width().unwrap_or(0);
                //检查是否超过屏幕宽度
                if current_width + ch_width > with {
                    let end = (line_offset + current_bytes).min(line_txt.len());
                    *line_num = (*line_num).saturating_sub(1); //行数减1
                    if *line_num >= skip_line {
                        *cur_line_count += 1;
                        let txt = line_txt.text((line_txt.len() - end)..);
                        let len: usize = txt.len();
                        let meta_line_offset = if line_start > 0 {
                            line_start - (line_offset + current_bytes)
                        } else {
                            line_txt.len() - (line_offset + current_bytes)
                        };
                        f(
                            txt,
                            EditLineMeta::new(
                                len,
                                char_count - char_index,
                                get_page_number!(*line_num, height),
                                *line_num,
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
                *line_num = (*line_num).saturating_sub(1); //行数减1
                if *line_num >= skip_line {
                    let txt = line_txt.text(..);
                    *cur_line_count += 1;
                    let len = txt.len();
                    let meta_line_offset = if line_start > 0 {
                        line_start - (line_offset + current_bytes)
                    } else {
                        0
                    };

                    f(
                        txt,
                        EditLineMeta::new(
                            len,
                            char_count - char_index,
                            get_page_number!(*line_num, height),
                            *line_num,
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
            //line_str.text(line_start..)
            Self::get_last_sort_warp_line(
                line_str,
                line_index,
                line_start,
                with,
                height,
                text_index,
                line_num,
                line_count,
                page_num,
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
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        let line_txt = line_str.text(line_start..);

        let mut current_width = 0; // 当前行宽度
        let mut line_offset = 0; //当前行偏移量
        let mut current_bytes = 0; //当前行字节数
        let mut char_index = 0; // 当前行字符索引
        let mut char_count = 0; // 当前行字符数

        for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            //检查是否超过屏幕宽度
            if current_width + ch_width > with {
                let end = (line_offset + current_bytes).min(line_txt.len());
                *line_num += 1; //行数加1
                if *line_num >= skip_line {
                    *cur_line_count += 1;
                    let txt = line_txt.text(line_offset..end);
                    let len: usize = txt.len();
                    let meta_line_offset = line_start + line_offset;
                    f(
                        txt,
                        EditLineMeta::new(
                            len,
                            i - char_index,
                            get_page_number!(*line_num, height),
                            *line_num,
                            line_index,
                            meta_line_offset,
                            line_str.get_line_file_start(),
                            line_str.get_line_file_end(),
                        ),
                    );
                }
                if *line_num % height == 0 {
                    //到达一页
                    *page_num += 1; //页数加1
                    text_index.set_page_offset(
                        *page_num,
                        PageOffset {
                            line_index,
                            line_offset: line_start + byte_index,
                            line_file_start: line_str.get_line_file_start(),
                            start_line_num: 0,
                            start_page_num: 0,
                        },
                    );
                    // let m = *page_num / PAGE_GROUP;
                    // let n = *page_num % PAGE_GROUP;
                    // if n == 0 && m > page_offset_list.len() - 1 {
                    //     //保存页数的偏移量
                    //     page_offset_list.push(PageOffset {
                    //         line_index,
                    //         line_offset: line_start + byte_index,
                    //         line_file_start: line_str.line_file_start,
                    //         start_line_num: 0,
                    //         start_page_num: 0,
                    //     });
                    // }
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
            *line_num += 1; //行数加1
            if *line_num >= skip_line {
                let txt = line_txt.text(line_offset..);
                *cur_line_count += 1;
                let len = txt.len();
                let meta_line_offset = line_start + line_offset;
                f(
                    txt,
                    EditLineMeta::new(
                        len,
                        char_count - char_index,
                        get_page_number!(*line_num, height),
                        *line_num,
                        line_index,
                        meta_line_offset,
                        line_str.get_line_file_start(),
                        line_str.get_line_file_end(),
                    ),
                );
            }
            if *line_num % height == 0 {
                *page_num += 1; //页数加1
                text_index.set_page_offset(
                    *page_num,
                    PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.get_line_file_end(),
                        start_line_num: 0,
                        start_page_num: 0,
                    },
                );

                // let m = *page_num / PAGE_GROUP;
                // let n = *page_num % PAGE_GROUP;
                // if n == 0 && m > page_offset_list.len() - 1 {
                //     //保存页数的偏移量
                //     page_offset_list.push(PageOffset {
                //         line_index: line_index + 1,
                //         line_offset: 0,
                //         line_file_start: line_str.line_file_end,
                //         start_line_num: 0,
                //         start_page_num: 0,
                //     });
                // }
            }
            if *cur_line_count >= line_count {
                return;
            }
        }
    }

    fn sort_warp<'a, F, I: TextIndex, L: Line<'a>>(
        line_str: L,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        text_index: &mut I,
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        is_rev: bool,
        f: &mut F,
    ) where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        if is_rev {
            Self::sort_warp_desc(
                line_str,
                line_index,
                line_start,
                with,
                height,
                text_index,
                line_num,
                line_count,
                page_num,
                cur_line_count,
                skip_line,
                is_rev,
                f,
            );
        } else {
            Self::sort_warp_asc(
                line_str,
                line_index,
                line_start,
                with,
                height,
                text_index,
                line_num,
                line_count,
                page_num,
                cur_line_count,
                skip_line,
                f,
            );
        }
    }

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        self.borrow_lines().text_from_sel(sel)
    }
}

pub(crate) struct EditTextWarp<T: Text + TextIndex + EditText> {
    edit_text: TextWarp<T>,
}

#[inherit_methods(from = "self.edit_text")]
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

    pub(crate) fn get_one_page(
        &self,
        line_num: usize,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)>;

    pub(crate) fn get_current_page(
        &self,
    ) -> ChapResult<(&RingVec<CacheStr>, &RingVec<EditLineMeta>)>;

    pub(crate) fn get_current_line_meta(&self) -> ChapResult<&RingVec<EditLineMeta>>;

    /**
     * 滚动下一行
     */
    pub(crate) fn scroll_next_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    /**
     * 滚动上一行
     */
    pub(crate) fn scroll_pre_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    pub(crate) fn scroll_pre_one_line2(&self, meta: &EditLineMeta) -> ChapResult<()>;

    pub(crate) fn get_text_len(&self, index: usize) -> usize;

    // 插入字符
    // 计算光标所在行
    // 计算光标所在列
    pub(crate) fn insert(
        &self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .insert(cursor_y, bytes_cursor, line_meta, c);
        //切断page_offset_list 索引
        // todo
        // let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        // unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    //插入换行
    pub(crate) fn insert_newline(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .insert_newline(cursor_y, cursor_x, line_meta);
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
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .backspace(cursor_y, bytes_cursor, count, line_meta);
        // todo
        //let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        //unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.edit_text.borrow_lines_mut().save(filepath)
    }
}
