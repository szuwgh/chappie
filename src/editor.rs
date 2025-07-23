use crate::gap_buffer::GapBytes;
use crate::gap_buffer::GapBytesCharIter;
use crate::mmap_file;
use crate::tui::TextSelect;
use crate::util;
use crate::{error::ChapResult, gap_buffer::GapBuffer};
use anyhow::Ok;
use inherit_methods_macro::inherit_methods;
use memmap2::Mmap;
use ratatui::symbols::line;
use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::ops::Bound;
use std::ops::RangeBounds;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use unicode_width::UnicodeWidthChar;
use utf8_iter::Utf8CharIndices;
use utf8_iter::Utf8CharsEx;

#[derive(Clone, Copy)]
pub(crate) enum TextWarpType {
    NoWrap,
    SoftWrap,
}

const PAGE_GROUP: usize = 1;
const CHUNK_SIZE: usize = 4 * 1024;
const CHAR_GAP_SIZE: usize = 128;
const HEX_GAP_SIZE: usize = 5;
pub(crate) const HEX_WITH: usize = 19;

pub(crate) trait TextOper {
    //滑动上一行
    fn scroll_pre_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    //滑动下一行
    fn scroll_next_one_line(&self, meta: &EditLineMeta) -> ChapResult<()>;

    //插入
    fn insert(
        &self,
        cursor_y: usize,
        cursor_x: usize,
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

    // fn jump_to(&self, line_num: usize) -> ChapResult<()> {
    //     match self {
    //         TextDisplay::Text(v) => v.jump_to(line_index),
    //         TextDisplay::Hex(v) => v.jump_to(line_index),
    //         TextDisplay::Edit(v) => v.jump_to(line_index),
    //     }
    // }

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
        cursor_x: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.insert(cursor_y, cursor_x, line_meta, c),
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
            TextDisplay::Edit(v) => v.scroll_pre_one_line(meta),
        }
    }

    fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        match self {
            TextDisplay::Text(v) => Ok(()),
            TextDisplay::Hex(v) => Ok(()),
            TextDisplay::Edit(v) => v.backspace(cursor_y, cursor_x, line_meta),
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

#[derive(Debug, Default)]
pub(crate) struct EditLineMeta {
    txt_len: usize,         //文本长度
    char_len: usize,        //char字符大小
    page_num: usize,        //所在页数
    line_num: usize,        //行数
    line_index: usize,      //行号
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

pub(crate) struct RingVec<T> {
    cache: Vec<T>,
    start: usize,
    size: usize,
}

impl<T> RingVec<T> {
    pub(crate) fn new(size: usize) -> Self {
        RingVec {
            cache: Vec::with_capacity(size),
            start: 0,
            size: size,
        }
    }

    //删除第最后元素 并返回这个元素
    pub(crate) fn remove_last(&mut self) -> Option<T> {
        self.remove(self.cache.len() - 1)
    }

    //删除第n个元素 并返回这个元素
    pub(crate) fn remove(&mut self, index: usize) -> Option<T> {
        if index >= self.cache.len() {
            return None;
        }
        let idx = (self.start + index) % self.cache.len();
        let item = self.cache.remove(idx);
        if self.start > idx {
            self.start -= 1;
        }

        Some(item)
    }

    pub(crate) fn push_front(&mut self, item: T) {
        if self.cache.len() < self.size {
            self.cache.insert(self.start, item);
        } else {
            if self.start == 0 {
                self.start = self.size - 1;
            } else {
                self.start = (self.start - 1) % self.size;
            }
            self.cache[self.start] = item;
        }
    }

    pub(crate) fn push(&mut self, item: T) {
        if self.cache.len() < self.size {
            if self.start == 0 {
                self.cache.push(item);
            } else {
                let end = (self.start + self.cache.len()) % self.cache.len();
                self.cache.insert(end, item);
                self.start = (self.start + 1) % self.cache.len();
            }
        } else {
            self.cache[self.start] = item;
            self.start = (self.start + 1) % self.size;
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.cache.len()
    }

    pub(crate) fn get(&self, index: usize) -> Option<&T> {
        if index >= self.cache.len() {
            return None;
        }
        let idx = (self.start + index) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn last(&self) -> Option<&T> {
        if self.cache.is_empty() {
            return None;
        }
        let idx = (self.start + self.cache.len() - 1) % self.cache.len();
        Some(&self.cache[idx])
    }

    pub(crate) fn clear(&mut self) {
        self.cache.clear();
        self.start = 0;
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.cache
            .iter()
            .cycle()
            .skip(self.start)
            .take(self.cache.len())
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

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> (&[u8], &[u8]) {
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
        if start > end && end > self.len() {
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
    Gap(GapBytesCache),
    Vec(VecCache),
    Bytes(BytesCache),
}

impl CacheStr {
    fn from_data(s: LineData) -> Self {
        match s {
            LineData::Bytes(v) => CacheStr::Bytes(BytesCache::from_slice(v)),
            LineData::GapBytes(v) => CacheStr::Gap(GapBytesCache::from_data(v)),
            LineData::Own(v) => CacheStr::Vec(VecCache::from_vec(v)),
        }
    }

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> (Cow<str>, Cow<str>) {
        match self {
            CacheStr::Gap(v) => {
                let (l, r) = v.text(range);
                (String::from_utf8_lossy(l), String::from_utf8_lossy(r))
            }
            CacheStr::Bytes(v) => (String::from_utf8_lossy(v.text(range)), Cow::Borrowed("")),
            CacheStr::Vec(v) => (String::from_utf8_lossy(v.text(range)), Cow::Borrowed("")),
        }
    }

    pub(crate) fn as_str(&self) -> (Cow<str>, Cow<str>) {
        match self {
            CacheStr::Gap(v) => v.as_str(),
            CacheStr::Vec(v) => (v.as_str(), Cow::Borrowed("")),
            CacheStr::Bytes(v) => (v.as_str(), Cow::Borrowed("")),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            CacheStr::Gap(v) => v.len(),
            CacheStr::Vec(v) => v.len(),
            CacheStr::Bytes(v) => v.len(),
        }
    }

    pub(crate) fn as_slice(&self) -> (&[u8], &[u8]) {
        match self {
            CacheStr::Gap(v) => v.as_slice(),
            CacheStr::Vec(v) => (v.as_slice(), &[]),
            CacheStr::Bytes(v) => (v.as_slice(), &[]),
        }
    }
}

pub(crate) trait Line {
    fn text_len(&self) -> usize;
    fn text(&self, range: impl RangeBounds<usize>) -> GapBytes;
}

pub(crate) trait Text {
    //是否有下一行
    fn has_next_line(&self, meta: &EditLineMeta) -> bool;

    //获取一行
    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> LineStr<'a>;

    //获取行的文本长度
    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize;

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8>;

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>>;
}

pub(crate) trait EditText {
    fn insert(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta, c: char);
    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta);
    fn backspace(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta);
    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()>;
}

struct GapTextIter<'a> {
    lines: std::slice::IterMut<'a, GapBuffer>,
}

impl<'a> GapTextIter<'a> {
    fn new(lines: &'a mut [GapBuffer], line_index: usize) -> GapTextIter<'a> {
        GapTextIter {
            lines: lines[line_index..].iter_mut(),
        }
    }
}

impl<'a> Iterator for GapTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        self.lines.next().map(|line| line.get_line_str())
    }
}

enum LineDataCharIter<'a> {
    CharIter(Utf8CharIndices<'a>),
    GapCharIter(GapBytesCharIter<'a>),
}

impl<'a> Iterator for LineDataCharIter<'a> {
    type Item = (usize, char);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LineDataCharIter::CharIter(iter) => iter.next(),
            LineDataCharIter::GapCharIter(iter) => iter.next(),
        }
    }
}

pub enum LineData<'a> {
    Own(Vec<u8>),
    Bytes(&'a [u8]),
    GapBytes(GapBytes<'a>),
}

impl<'a> LineData<'a> {
    fn empty() -> LineData<'a> {
        LineData::Bytes(&[])
    }

    fn as_str(&self) -> (Cow<str>, Cow<str>) {
        match self {
            LineData::Bytes(v) => (String::from_utf8_lossy(v), Cow::Borrowed("")),
            LineData::GapBytes(v) => v.as_str(),
            LineData::Own(v) => (String::from_utf8_lossy(v), Cow::Borrowed("")),
        }
    }

    fn as_slice(&self) -> (&[u8], &[u8]) {
        match self {
            LineData::Bytes(v) => (v, &[]),
            LineData::GapBytes(v) => v.as_slice(),
            LineData::Own(v) => (v.as_slice(), &[]),
        }
    }

    fn len(&self) -> usize {
        match self {
            LineData::Bytes(v) => v.len(),
            LineData::GapBytes(v) => v.len(),
            LineData::Own(v) => v.len(),
        }
    }

    fn char_indices(&self) -> LineDataCharIter<'_> {
        match self {
            LineData::Bytes(v) => LineDataCharIter::CharIter(v.char_indices()),
            LineData::GapBytes(v) => LineDataCharIter::GapCharIter(v.char_indices()),
            LineData::Own(v) => LineDataCharIter::CharIter(v.char_indices()),
        }
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize>) -> LineData<'a> {
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
        //log::debug!("text start: {} end: {} len: {}", start, end, self.len());
        assert!(start <= end);

        match self {
            LineData::Bytes(v) => LineData::Bytes(&v[start..end]),
            LineData::GapBytes(v) => LineData::GapBytes(v.text(range)),
            LineData::Own(v) => LineData::Own(v[start..end].to_vec()),
        }
    }
}

pub struct LineStr<'a> {
    // pub(crate) line: GapBytes<'a>,
    pub(crate) line_data: LineData<'a>,
    pub(crate) line_file_start: usize,
    pub(crate) line_file_end: usize,
}

impl<'a> LineStr<'a> {
    fn text_len(&self) -> usize {
        self.line_data.len()
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize>) -> LineData<'a> {
        self.line_data.text(range)
    }
}

pub(crate) struct FileIoText {
    file: BufReader<File>,
}

impl FileIoText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<FileIoText> {
        let file = File::open(filename)?;
        Ok(FileIoText {
            file: BufReader::new(file),
        })
    }
}

pub struct FileIoTextIter<'a> {
    file: &'a BufReader<File>,
    line_index: usize,
    line_file_start: usize,
    line_file_end: usize,
}

pub(crate) struct Chunk {
    buffer: GapBuffer,
    file_start: usize,
    file_end: usize,
    is_modified: bool,
}

impl Chunk {
    fn text(&self, range: impl RangeBounds<usize>) -> GapBytes<'_> {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&start) => start,
            std::ops::Bound::Excluded(&start) => start + 1,
            std::ops::Bound::Unbounded => self.file_start,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&end) => end, //包含
            std::ops::Bound::Excluded(&end) => end, //排除
            std::ops::Bound::Unbounded => self.file_end,
        };
        assert!(start <= end && start >= self.file_start && end >= self.file_start);
        self.buffer
            .text((start - self.file_start)..(end - self.file_start))
    }

    fn text_len(&self) -> usize {
        self.buffer.text_len()
    }
}

const CHUNK_NUM: usize = 5;

pub(crate) struct HexText {
    chunks: RingVec<Chunk>,
    file: File,
    cache: HashMap<usize, Chunk>,
    file_size: usize,
}

impl HexText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<HexText> {
        let mut file = File::open(filename)?;
        let file_size = file.metadata()?.len() as usize;
        let mut chunks = RingVec::new(CHUNK_NUM);

        let mut buf = [0u8; CHUNK_SIZE];
        let mut bytes_start = 0;
        for _ in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < CHUNK_SIZE {
                let n = file.read(&mut buf)?;
                if n == 0 {
                    //跳出 for 循环
                    break;
                }
                bytes_read += n;
                buffer.insert(buffer.text_len(), &buf[..n]);
            }
            if bytes_read == 0 {
                break;
            }
            chunks.push(Chunk {
                buffer: buffer,
                file_start: bytes_start,
                file_end: bytes_start + bytes_read,
                is_modified: false,
            });
            bytes_start += bytes_read;
        }

        Ok(HexText {
            chunks: chunks,
            file,
            cache: HashMap::new(),
            file_size,
        })
    }

    pub(crate) fn get_file_size(&self) -> usize {
        self.file_size
    }

    pub(crate) fn read_chunks(&mut self, file_seek: usize) -> ChapResult<()> {
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut chunks = RingVec::new(CHUNK_NUM);

        let mut buf = [0u8; CHUNK_SIZE];
        let mut bytes_start = file_seek;
        for _ in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < CHUNK_SIZE {
                let n = self.file.read(&mut buf)?;
                if n == 0 {
                    //跳出 for 循环
                    break;
                }
                bytes_read += n;
                buffer.insert(buffer.text_len(), &buf[..n]);
            }
            if bytes_read == 0 {
                break;
            }
            chunks.push(Chunk {
                buffer: buffer,
                file_start: bytes_start,
                file_end: bytes_start + bytes_read,
                is_modified: false,
            });
            bytes_start += bytes_read;
        }
        return Ok(());
    }

    pub(crate) fn read_last_chunk(&mut self, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出最后一个块
        let chunk2 = self.chunks.remove_last();
        if let Some(c) = chunk2 {
            if c.is_modified {
                self.cache.insert(c.file_start, c);
            }
        }
        self.chunks.push_front(Chunk {
            buffer: buffer,
            file_start: file_seek,
            file_end: file_seek + bytes_read,
            is_modified: false,
        });
        Ok(())
    }

    pub(crate) fn read_next_chunk(&mut self, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出第一个块
        let chunk0 = self.chunks.remove(0);
        if let Some(c) = chunk0 {
            if c.is_modified {
                self.cache.insert(c.file_start, c);
            }
        }
        self.chunks.push(Chunk {
            buffer: buffer,
            file_start: file_seek,
            file_end: file_seek + bytes_read,
            is_modified: false,
        });
        Ok(())
    }
}

impl Text for HexText {
    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut start = sel.get_start();
        let end = sel.get_end();
        for s in self.chunks.iter() {
            // 跳过选区起点位于此块之后的情况
            if start >= s.file_end {
                continue;
            }
            // 如果选区在此块之前结束，则无需继续
            if end < s.file_start {
                break;
            }
            // 计算当前块与选区的重叠范围
            let from = (start.max(s.file_start) - s.file_start) as usize;
            let to = (end.min(s.file_end) - s.file_start) as usize;
            // 提取并追加子片段
            buf.extend_from_slice(&s.buffer.text(from..=to).to_vec());
            // 如果选区在此块内完全结束，则跳出循环
            if end <= s.file_end {
                break;
            }
            // 更新起点为当前块末尾，继续下一块
            start = s.file_end;
            // if start >= s.file_start && end <= s.file_end {
            //     buf.extend_from_slice(
            //         &s.buffer
            //             .text(start - s.file_start..=end - s.file_start)
            //             .to_vec(),
            //     );
            // } else if start >= s.file_start && end > s.file_end && start < s.file_end {
            //     buf.extend_from_slice(&s.buffer.text(start - s.file_start..).to_vec());
            //     start = s.file_end;
            // }
        }
        buf
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> LineStr<'a> {
        let with = line_file_end - line_file_start;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if line_file_start > chunk.file_end || line_file_start < chunk.file_start {
                continue;
            }
            let buffer = chunk.text(line_file_start..);
            let line_start = line_file_start;
            if with > buffer.len() {
                let len = buffer.len();
                if i < self.chunks.len() - 1 {
                    if let Some(c1) = self.chunks.get(i + 1) {
                        let mut v: Vec<u8> = Vec::with_capacity(with);
                        v.extend_from_slice(buffer.left());
                        v.extend_from_slice(buffer.right());
                        let remaining = with - buffer.len();
                        let buf1 = c1.text(line_file_start + buffer.len()..);
                        if remaining >= buf1.len() {
                            v.extend_from_slice(buf1.left());
                            v.extend_from_slice(buf1.right());
                            return LineStr {
                                line_data: LineData::Own(v),
                                line_file_start: line_start,
                                line_file_end: line_start + len + buf1.len(),
                            };
                        } else {
                            let buf2 = buf1.text(..remaining);
                            v.extend_from_slice(buf2.left());
                            v.extend_from_slice(buf2.right());
                            return LineStr {
                                line_data: LineData::Own(v),
                                line_file_start: line_start,
                                line_file_end: line_start + with,
                            };
                        }
                    } else {
                        return LineStr {
                            line_data: LineData::GapBytes(buffer),
                            line_file_start: line_start,
                            line_file_end: line_file_end,
                        };
                    }
                } else {
                    return LineStr {
                        line_data: LineData::GapBytes(buffer),
                        line_file_start: line_start,
                        line_file_end: line_start + len,
                    };
                }
            } else {
                return LineStr {
                    line_data: LineData::GapBytes(buffer.text(..with)),
                    line_file_start: line_start,
                    line_file_end: line_start + with,
                };
            }
        }
        assert!(false, "not found line");
        return LineStr {
            // line: buffer,
            line_data: LineData::Bytes(&[]),
            line_file_start: 0,
            line_file_end: 0,
        };
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        if meta.line_file_end >= self.file_size {
            return false;
        }
        return true;
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        let mut j = None;
        for (i, chunk) in self.chunks.iter().enumerate() {
            if line_file_start >= chunk.file_start && line_file_start < chunk.file_end {
                j = Some(i);
                break;
            }
        }
        if let Some(j) = j {
            if j == 0 {
                //读取上一个块 把最后一个块弹出
                let mut last_chunk = self.chunks.get(0).unwrap().file_start;
                if last_chunk == 0 {
                    return HexTextIter::new(
                        [self.chunks.get(0), self.chunks.get(1)],
                        HEX_WITH,
                        line_file_start,
                    );
                } else {
                    last_chunk = last_chunk.saturating_sub(CHUNK_SIZE);
                    log::debug!(
                        "last_chunk: {},line_file_start:{}",
                        last_chunk,
                        line_file_start
                    );
                    self.read_last_chunk(last_chunk).unwrap();
                    for c in self.chunks.iter() {
                        log::debug!(
                            "chunk.file_start: {} chunk.file_end: {}",
                            c.file_start,
                            c.file_end
                        );
                    }
                    return HexTextIter::new(
                        [self.chunks.get(1), self.chunks.get(2)],
                        HEX_WITH,
                        line_file_start,
                    );
                }
            } else if j == self.chunks.len() - 1 {
                //最后一个块
                //读取下一个块 把第一个块弹出
                let next_file_seek = self.chunks.get(j).unwrap().file_end;
                if next_file_seek >= self.file_size {
                    return HexTextIter::new([self.chunks.get(j), None], HEX_WITH, line_file_start);
                } else {
                    self.read_next_chunk(next_file_seek).unwrap();
                    return HexTextIter::new(
                        [self.chunks.get(j - 1), self.chunks.get(j)],
                        HEX_WITH,
                        line_file_start,
                    );
                }
            } else {
                //不是最后一个块
                return HexTextIter::new(
                    [self.chunks.get(j), self.chunks.get(j + 1)],
                    HEX_WITH,
                    line_file_start,
                );
            }
        }
        //如果没有找到块 从新重读chunks
        //通过line_file_start 计算在哪一个块 每个块的大小是 CHUNK_SIZE
        let chunk_start = line_file_start / CHUNK_SIZE * CHUNK_SIZE;
        self.read_chunks(chunk_start).unwrap();
        return HexTextIter::new(
            [self.chunks.get(0), self.chunks.get(1)],
            HEX_WITH,
            line_file_start,
        );
    }
}

struct HexTextIter<'a> {
    hex_chunk: [Option<&'a Chunk>; 2],
    with: usize,
    line_file_start: usize,
}

impl<'a> HexTextIter<'a> {
    fn new(
        hex_chunk: [Option<&'a Chunk>; 2],
        with: usize,
        line_file_start: usize,
    ) -> HexTextIter<'a> {
        HexTextIter {
            hex_chunk,
            with,
            line_file_start: line_file_start,
        }
    }
}

impl<'a> Iterator for HexTextIter<'a> {
    type Item = LineStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for (i, chunk) in self.hex_chunk.iter().enumerate() {
            if let Some(c) = chunk {
                if self.line_file_start > c.file_end || self.line_file_start < c.file_start {
                    continue;
                }
                let buffer = c.text(self.line_file_start..);
                let line_start = self.line_file_start;
                // log::debug!(
                //     "next line_file_start: {} chunk.file_end: {} chunk.file_start: {}",
                //     self.line_file_start,
                //     c.file_end,
                //     c.file_start
                // );
                //长度大于 buffer 说明当前chunk 不足以显示一行
                if self.with > buffer.len() {
                    let len = buffer.len();
                    if i == 0 {
                        //从第一个块读取完毕
                        if let Some(c1) = self.hex_chunk[1] {
                            let mut v: Vec<u8> = Vec::with_capacity(self.with);
                            v.extend_from_slice(buffer.left());
                            v.extend_from_slice(buffer.right());
                            let remaining = self.with - buffer.len();
                            //从下一个块读取
                            let buf1 = c1.text(self.line_file_start + len..);
                            //
                            if remaining >= buf1.len() {
                                // let buf2 = buf1.text(..need);
                                v.extend_from_slice(buf1.left());
                                v.extend_from_slice(buf1.right());
                                self.line_file_start += len + buf1.len();
                                return Some(LineStr {
                                    // line: buffer,
                                    line_data: LineData::Own(v),
                                    line_file_start: line_start,
                                    line_file_end: line_start + len + buf1.len(),
                                });
                            } else {
                                self.line_file_start += self.with;
                                let buf2 = buf1.text(..remaining);
                                v.extend_from_slice(buf2.left());
                                v.extend_from_slice(buf2.right());
                                return Some(LineStr {
                                    // line: buffer,
                                    line_data: LineData::Own(v),
                                    line_file_start: line_start,
                                    line_file_end: line_start + self.with,
                                });
                            }
                        } else {
                            self.line_file_start += len;
                            // println!("读取完毕");
                            return Some(LineStr {
                                //   line: buffer,
                                line_data: LineData::GapBytes(buffer),
                                line_file_start: line_start,
                                line_file_end: line_start + len,
                            });
                        }
                    } else {
                        self.line_file_start += len;
                        // println!("读取完毕");
                        return Some(LineStr {
                            //   line: buffer,
                            line_data: LineData::GapBytes(buffer),
                            line_file_start: line_start,
                            line_file_end: line_start + len,
                        });
                    }
                } else {
                    self.line_file_start += self.with;
                    return Some(LineStr {
                        // line: buffer.text(..self.with),
                        line_data: LineData::GapBytes(buffer.text(..self.with)),
                        line_file_start: line_start,
                        line_file_end: line_start + self.with,
                    });
                }
            }
        }
        return None;
    }
}

pub(crate) struct MmapText {
    mmap: Mmap,
}

impl MmapText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<MmapText> {
        let mmap = mmap_file(filename)?;
        Ok(MmapText { mmap })
    }

    pub(crate) fn new(mmap: Mmap) -> MmapText {
        MmapText { mmap }
    }
}

pub struct MmapTextIter<'a> {
    mmap: &'a Mmap,
    line_index: usize,
    line_file_start: usize,
    line_file_end: usize,
}

impl<'a> Iterator for MmapTextIter<'a> {
    type Item = LineStr<'a>;
    fn next(&mut self) -> Option<LineStr<'a>> {
        if self.line_file_start >= self.line_file_end {
            return None;
        }
        let mmap = &self.mmap[self.line_file_start..];
        let mut end = 0;
        for (i, byte) in mmap.iter().enumerate() {
            if *byte == b'\n' || i == mmap.len() - 1 {
                end = i;
                break;
            }
            end = i;
        }
        let line = &mmap[..end];
        let line_start = self.line_file_start;
        self.line_file_start += end + 1;
        Some(LineStr {
            line_data: LineData::GapBytes(GapBytes::new(line, &[])),
            line_file_start: line_start,
            line_file_end: line_start + end,
        })
    }
}

impl<'a> MmapTextIter<'a> {
    fn new(
        mmap: &'a Mmap,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> MmapTextIter<'a> {
        MmapTextIter {
            mmap,
            line_index,
            line_file_start,
            line_file_end,
        }
    }
}

impl Text for MmapText {
    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        todo!("Not implement text_from_sel for MmapText");
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> LineStr<'a> {
        let line = &self.mmap[line_file_start..line_file_end];
        LineStr {
            line_data: LineData::GapBytes(GapBytes::new(line, &[])),
            line_file_start: line_file_start,
            line_file_end: line_file_end,
        }
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        if meta.get_line_file_start() + meta.get_line_offset() + meta.get_txt_len()
            >= self.mmap.len() - 1
        {
            return false;
        }
        true
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        MmapTextIter::new(&self.mmap, line_index, line_start, self.mmap.len())
    }
}

pub(crate) struct GapText {
    lines: Vec<GapBuffer>, //每行使用 GapBuffer 存储
}

impl GapText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<GapText> {
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);
        let mut buffer = Vec::new();
        let mut gap_buffers: Vec<GapBuffer> = Vec::new();

        while reader.read_until(b'\n', &mut buffer)? > 0 {
            if let Some(&b'\n') = buffer.last() {
                buffer.pop();
            }
            // 兼容 CRLF，去除末尾的 '\r'
            if let Some(&b'\r') = buffer.last() {
                buffer.pop();
            }

            let mut gap_buffer = GapBuffer::new(buffer.len() + CHAR_GAP_SIZE);
            log::debug!("buffer: len: {:?},{:?}", buffer.len(), buffer);
            gap_buffer.insert(0, &buffer);
            gap_buffers.push(gap_buffer);
            buffer.clear(); // 清空缓冲区，准备读取下一行
        }

        Ok(GapText { lines: gap_buffers })
    }

    fn borrow_lines(&self) -> &Vec<GapBuffer> {
        &self.lines
    }

    fn borrow_lines_mut(&mut self) -> &mut Vec<GapBuffer> {
        &mut self.lines
    }

    fn get_iter(&mut self, line_index: usize) -> GapTextIter {
        GapTextIter::new(&mut self.lines, line_index)
    }

    pub(crate) fn get_text_len(&self, index: usize) -> usize {
        if index >= self.lines.len() {
            return 0;
        }
        self.lines[index].text_len()
    }

    fn rename_backup<P1: AsRef<Path>, P2: AsRef<Path>>(
        filepath: P1,
        backup_name: P2,
    ) -> ChapResult<()> {
        fs::rename(backup_name, filepath)?;
        Ok(())
    }

    pub(crate) fn save_file<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        let backup_name = Self::get_backup_name(&filepath)?;
        self.make_backup(&backup_name)?;
        Self::rename_backup(&filepath, &backup_name)?;
        Ok(())
    }

    fn get_backup_name<P: AsRef<Path>>(filepath: P) -> ChapResult<PathBuf> {
        let mut path = filepath.as_ref().to_path_buf();
        if let Some(file_name) = path.file_name() {
            path.set_file_name(format!(".{}.{}", file_name.to_string_lossy(), "chap"));
        }
        Ok(path)
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        // 备份文件
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        for line in self.borrow_lines_mut().iter_mut() {
            let txt = line.text(..);
            w.write(txt.left()).unwrap();
            w.write(txt.right()).unwrap();
            w.write(b"\n").unwrap();
        }
        w.flush()?;
        Ok(())
    }
}

impl Text for GapText {
    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        todo!("Not implement text_from_sel for GapText");
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        let mut line_index = meta.get_line_index();
        let mut line_end = meta.get_line_end();
        // 结束
        if line_index == self.lines.len() - 1 && line_end == self.get_text_len(line_index) {
            return false; // return (None, EditLineMeta::default());
        }
        true
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> LineStr<'a> {
        self.borrow_lines_mut()[line_index].get_line_str()
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        self.borrow_lines()[line_index].text_len()
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_start: usize,
    ) -> impl Iterator<Item = LineStr<'a>> {
        self.get_iter(line_index)
    }
}

impl EditText for GapText {
    fn backspace(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );
        if self.borrow_lines_mut()[line_index].text_len() == 0 && line_offset == 0 {
            //删除一行
            self.borrow_lines_mut().remove(line_index);
            return;
        }
        //表示当前行和前一行合并
        if line_offset == 0 {
            if line_index == 0 {
                return;
            }
            let (pre_lines, cur_lines) = self.borrow_lines_mut().split_at_mut(line_index);
            let pre_line = &mut pre_lines[line_index - 1];
            if pre_line.text_len() == 0 {
                self.borrow_lines_mut().remove(line_index - 1);
                return;
            } else {
                let cur_line = &mut cur_lines[0];
                let cur_line_txt = cur_line.text(..);
                pre_line.insert(pre_line.text_len(), cur_line_txt.left());
                pre_line.insert(pre_line.text_len(), cur_line_txt.right());
                self.borrow_lines_mut().remove(line_index);
            }
            return;
        }
        self.borrow_lines_mut()[line_index].backspace(line_offset);
    }

    fn insert(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta, c: char) {
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );

        let mut buf = [0u8; 4]; // 一个 char 最多需要 4 个字节存储 UTF-8 编码
        let s: &str = c.encode_utf8(&mut buf);
        let line = &mut self.borrow_lines_mut()[line_index];
        //如果line_offset大于文本长度 要填充空格
        if line_offset > line.text_len() {
            let gap_len = line_offset - line.text_len();
            line.insert(line.text_len(), " ".repeat(gap_len).as_bytes());
        }
        line.insert(line_offset, s.as_bytes());
    }

    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {
        // let (line_index, line_offset) = self.calculate_x_y(cursor_y, cursor_x);
        let (line_index, line_offset) = (
            line_meta.get_line_index(),
            line_meta.get_line_offset() + cursor_x,
        );
        //    println!("line_index: {}, line_offset: {}", line_index, line_offset);
        let line_txt = self.borrow_lines_mut()[line_index].text(..);
        let line_len = line_txt.len();
        {
            if line_offset > line_len {
                // 如果光标不在行尾，插入新行
                let new_gap_buffer = GapBuffer::new(10);
                self.borrow_lines_mut()
                    .insert(line_index + 1, new_gap_buffer);
            } else {
                let b = &line_txt.text(line_offset..);
                let mut new_gap_buffer = GapBuffer::new(b.len() + 5);
                new_gap_buffer.insert(0, b.left());
                new_gap_buffer.insert(new_gap_buffer.text_len(), b.left());
                self.borrow_lines_mut()
                    .insert(line_index + 1, new_gap_buffer);
            }
        }

        {
            if line_len > line_offset {
                let delete_len = line_len - line_offset;
                // 删除当前行的剩余部分
                self.borrow_lines_mut()[line_index].delete(line_len, delete_len);
            }
        }
    }

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.save_file(filepath)
    }
}

pub(crate) struct TextWarp<T: Text> {
    lines: UnsafeCell<T>,                               // 文本
    cache_lines: UnsafeCell<RingVec<CacheStr>>,         // 缓存行
    cache_line_meta: UnsafeCell<RingVec<EditLineMeta>>, // 缓存行
    page_offset_list: UnsafeCell<Vec<PageOffset>>,      // 每页的偏移量
    height: usize,                                      //最大行数
    with: usize,
    text_warp_type: TextWarpType,
}

impl<T: Text> TextWarp<T> {
    pub(crate) fn new(
        lines: T,
        height: usize,
        with: usize,
        text_warp_type: TextWarpType,
    ) -> TextWarp<T> {
        TextWarp {
            lines: UnsafeCell::new(lines),
            cache_lines: UnsafeCell::new(RingVec::new(height)),
            cache_line_meta: UnsafeCell::new(RingVec::new(height)),
            page_offset_list: UnsafeCell::new(vec![PageOffset {
                line_index: 0,
                line_offset: 0,
                line_file_start: 0,
            }]),
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

    fn borrow_page_offset_list(&self) -> &Vec<PageOffset> {
        unsafe { &*self.page_offset_list.get() }
    }

    fn borrow_page_offset_list_mut(&self) -> &mut Vec<PageOffset> {
        unsafe { &mut *self.page_offset_list.get() }
    }

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

    //从当前行开始获取后面n行
    pub(crate) fn get_pre_line<'a>(
        &'a self,
        meta: &EditLineMeta,
        line_count: usize,
    ) -> (Option<CacheStr>, EditLineMeta) {
        // log::debug!("get_pre_line: {}", meta.get_line_num());
        assert!(meta.get_line_num() >= 1);
        if meta.get_line_num() == 1 {
            return (None, EditLineMeta::default());
        }
        let mut s = LineData::empty();
        let mut m = EditLineMeta::default();
        self.get_text(meta.get_line_num() - line_count, line_count, |txt, meta| {
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

        self.get_text(line_num, line_count, |txt, meta| {
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
        self.get_text(line_num, line_count, |txt, meta| {
            lines.push(CacheStr::from_data(txt));
            lines_meta.push(meta);
        });
        (lines, lines_meta)
    }

    fn get_text<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    where
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        assert!(line_num >= 1);
        // 计算页码
        let page_num = self.get_page_num(line_num);
        // 计算页码
        let mut index = (page_num - 1) / PAGE_GROUP;
        let page_offset_list = self.borrow_page_offset_list();
        let page_offset = if index >= page_offset_list.len() {
            index = page_offset_list.len() - 1;
            page_offset_list.last().unwrap()
        } else {
            &page_offset_list[index]
        };
        let start_page_num = index * PAGE_GROUP;
        assert!(line_num >= start_page_num * self.height);
        //跳过的行数
        let skip_line = line_num;
        // println!("skip_line:{}", skip_line);
        // println!("page_offset:{:?}", page_offset);
        self.get_char_text_fn(
            &page_offset,
            line_count,
            start_page_num * self.height,
            start_page_num,
            skip_line,
            &mut f,
        );
    }

    fn get_hex_text_fn<'a, F>(
        &'a self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(&'a [u8], EditLineMeta),
    {
        todo!()
    }

    fn get_char_text_fn<'a, F>(
        &'a self,
        page_offset: &PageOffset,
        line_count: usize,
        start_line_num: usize,
        start_page_num: usize,
        skip_line: usize,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        let mut cur_line_count = 0;
        let mut line_num = start_line_num;
        let mut page_num = start_page_num;

        let iter = self.borrow_lines_mut().iter(
            page_offset.line_index,
            page_offset.line_offset,
            page_offset.line_file_start,
        );

        for (i, v) in iter.enumerate() {
            let line_offset = if i == 0 { page_offset.line_offset } else { 0 };
            Self::set_line_char_txt(
                v,
                page_offset.line_index + i,
                line_offset,
                self.with,
                self.height,
                self.borrow_page_offset_list_mut(),
                &mut line_num,
                line_count,
                &mut page_num,
                &mut cur_line_count,
                skip_line,
                &self.text_warp_type,
                f,
            );
            if cur_line_count >= line_count {
                return;
            }
        }
    }

    fn set_line_char_txt<'a, F>(
        line_str: LineStr<'a>,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        page_offset_list: &mut Vec<PageOffset>,
        line_num: &mut usize,
        line_count: usize,
        page_num: &mut usize,
        cur_line_count: &mut usize,
        skip_line: usize,
        text_warp_type: &TextWarpType,
        f: &mut F,
    ) where
        // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
        F: FnMut(LineData<'a>, EditLineMeta),
    {
        //空行
        let line_txt = line_str.text(line_start..);

        if line_txt.len() == 0 {
            *line_num += 1; //行数加1
            if *line_num >= skip_line {
                *cur_line_count += 1;
                f(
                    LineData::empty(),
                    EditLineMeta::new(
                        0,
                        0,
                        *page_num + 1,
                        *line_num,
                        line_index,
                        0,
                        line_str.line_file_start,
                        line_str.line_file_end,
                    ),
                );
            }
            if *line_num % height == 0 {
                //到达一页
                *page_num += 1; //页数加1
                let m = *page_num / PAGE_GROUP;
                let n = *page_num % PAGE_GROUP;
                if n == 0 && m > page_offset_list.len() - 1 {
                    //保存页数的偏移量
                    page_offset_list.push(PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.line_file_end,
                    });
                }
            }

            if *cur_line_count >= line_count {
                return;
            }
            return;
        }

        match text_warp_type {
            TextWarpType::NoWrap => {
                //  todo!()
                Self::no_warp(
                    line_str,
                    line_index,
                    line_start,
                    with,
                    height,
                    page_offset_list,
                    line_num,
                    line_count,
                    page_num,
                    cur_line_count,
                    skip_line,
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
                    page_offset_list,
                    line_num,
                    line_count,
                    page_num,
                    cur_line_count,
                    skip_line,
                    f,
                );
            }
        }
    }

    fn no_warp<'a, F>(
        line_str: LineStr<'a>,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        page_offset_list: &mut Vec<PageOffset>,
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
        *line_num += 1; //行数加1
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
                    line_str.line_file_start,
                    line_str.line_file_end,
                ),
            );
        }
        if *line_num % height == 0 {
            //到达一页
            *page_num += 1; //页数加1
            let m = *page_num / PAGE_GROUP;
            let n = *page_num % PAGE_GROUP;
            if n == 0 && m > page_offset_list.len() - 1 {
                //保存页数的偏移量
                page_offset_list.push(PageOffset {
                    line_index,
                    line_offset: line_start + 0,
                    line_file_start: line_str.line_file_end,
                });
            }
        }
        if *cur_line_count >= line_count {
            return;
        }
    }

    fn sort_warp<'a, F>(
        line_str: LineStr<'a>,
        line_index: usize,
        line_start: usize,
        with: usize,
        height: usize,
        page_offset_list: &mut Vec<PageOffset>,
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
                                // let mut cur_byte_index = 0;

        for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
            let ch_width = ch.width().unwrap_or(0);
            // let byte_size = byte_index - cur_byte_index;
            // cur_byte_index = byte_index;
            log::debug!(
                "ch: {} ,ch1: {:?} ch_width: {},line_offset:{},current_bytes:{},current_width:{},with:{},byte_index:{},u8:{:?}",
                ch,
                ch,
                ch_width,
                line_offset,
                current_bytes,
                current_width,
                with,byte_index,
                ch.to_string().into_bytes()
            );
            //检查是否超过屏幕宽度
            if current_width + ch_width > with {
                let end = (line_offset + current_bytes).min(line_txt.len());
                *line_num += 1; //行数加1
                if *line_num >= skip_line {
                    *cur_line_count += 1;
                    let txt = line_txt.text(line_offset..end);
                    let len = txt.len();
                    f(
                        txt,
                        EditLineMeta::new(
                            len,
                            i - char_index,
                            *page_num + 1,
                            *line_num,
                            line_index,
                            line_start + line_offset,
                            line_str.line_file_start,
                            line_str.line_file_end,
                        ),
                    );
                }
                if *line_num % height == 0 {
                    //到达一页
                    *page_num += 1; //页数加1
                    let m = *page_num / PAGE_GROUP;
                    let n = *page_num % PAGE_GROUP;
                    if n == 0 && m > page_offset_list.len() - 1 {
                        //保存页数的偏移量
                        page_offset_list.push(PageOffset {
                            line_index,
                            line_offset: line_start + byte_index,
                            line_file_start: line_str.line_file_start,
                        });
                    }
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
            *line_num += 1;

            if *line_num >= skip_line {
                let txt = line_txt.text(line_offset..);
                *cur_line_count += 1;
                let len = txt.len();
                f(
                    txt,
                    EditLineMeta::new(
                        len,
                        char_count - char_index,
                        *page_num + 1,
                        *line_num,
                        line_index,
                        line_start + line_offset,
                        line_str.line_file_start,
                        line_str.line_file_end,
                    ),
                );
            }
            if *line_num % height == 0 {
                *page_num += 1; //页数加1
                let m = *page_num / PAGE_GROUP;
                let n = *page_num % PAGE_GROUP;
                if n == 0 && m > page_offset_list.len() - 1 {
                    //保存页数的偏移量
                    page_offset_list.push(PageOffset {
                        line_index: line_index + 1,
                        line_offset: 0,
                        line_file_start: line_str.line_file_end,
                    });
                }
            }
            if *cur_line_count >= line_count {
                return;
            }
        }
    }

    fn get_text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        self.borrow_lines().text_from_sel(sel)
    }
}

pub(crate) struct EditTextWarp<T: Text + EditText> {
    edit_text: TextWarp<T>,
}

#[inherit_methods(from = "self.edit_text")]
impl<T: Text + EditText> EditTextWarp<T> {
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

    pub(crate) fn get_text_len(&self, index: usize) -> usize;

    // 插入字符
    // 计算光标所在行
    // 计算光标所在列
    pub(crate) fn insert(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
        c: char,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .insert(cursor_y, cursor_x, line_meta, c);
        //切断page_offset_list 索引
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
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
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    // 删除光标前一个字符
    pub(crate) fn backspace(
        &self,
        cursor_y: usize,
        cursor_x: usize,
        line_meta: &EditLineMeta,
    ) -> ChapResult<()> {
        self.edit_text
            .borrow_lines_mut()
            .backspace(cursor_y, cursor_x, line_meta);
        let page_offset_list = self.edit_text.borrow_page_offset_list_mut();
        unsafe { page_offset_list.set_len(line_meta.get_page_num()) };
        self.edit_text.borrow_cache_lines_mut().clear();
        self.edit_text.borrow_cache_line_meta_mut().clear();
        Ok(())
    }

    pub(crate) fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        self.edit_text.borrow_lines_mut().save(filepath)
    }

    // //从当前行开始获取后面n行
    // pub(crate) fn get_pre_line<'a>(
    //     &'a self,
    //     meta: &EditLineMeta,
    //     line_count: usize,
    // ) -> (Option<CacheStr>, EditLineMeta) {
    //     if meta.get_line_num() == 1 {
    //         return (None, EditLineMeta::default());
    //     }
    //     let mut s = "";
    //     let mut m = EditLineMeta::default();
    //     self.get_text(meta.get_line_num() - line_count, line_count, |txt, meta| {
    //         s = txt;
    //         m = meta;
    //     });
    //     (Some(CacheStr::from_str(s)), m)
    // }

    // //从当前行开始获取后面n行
    // pub(crate) fn get_next_line<'a>(
    //     &'a self,
    //     meta: &EditLineMeta,
    //     line_count: usize,
    // ) -> (Option<CacheStr>, EditLineMeta) {
    //     let mut line_index = meta.get_line_index();
    //     let mut line_end = meta.get_line_end();
    //     if !self.borrow_lines().has_next_line(meta) {
    //         return (None, EditLineMeta::default());
    //     }

    //     let line = self.borrow_lines().get_line(line_index); //&self.borrow_lines()[line_index];
    //     if line_end == line.text_len() {
    //         line_end = 0;
    //         line_index += 1;
    //     }

    //     let p = PageOffset {
    //         line_index: line_index,
    //         line_start: line_end,
    //     };
    //     let mut s = "";
    //     let mut m = EditLineMeta::default();
    //     let start_page_num = meta.get_line_num() / self.height;
    //     self.get_text_fn(
    //         &p,
    //         line_count,
    //         meta.get_line_num(),
    //         start_page_num,
    //         0,
    //         &mut |x, m1| {
    //             s = x;
    //             m = m1;
    //         },
    //     );
    //     (Some(CacheStr::from_str(s)), m)
    // }

    // pub(crate) fn scroll_next_one_line(
    //     &self,
    //     meta: &EditLineMeta,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     let (s, l) = self.get_next_line(meta, 1);
    //     if let Some(s) = s {
    //         self.borrow_cache_lines_mut().push(s);
    //         self.borrow_cache_line_meta_mut().push(l);
    //     }
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn scroll_pre_one_line(
    //     &self,
    //     meta: &EditLineMeta,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     let (s, l) = self.get_pre_line(meta, 1);
    //     if let Some(s) = s {
    //         self.borrow_cache_lines_mut().push_front(s);
    //         self.borrow_cache_line_meta_mut().push_front(l);
    //     }
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn get_one_page(
    //     &self,
    //     line_num: usize,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     self.get_line_content(line_num, self.height)
    // }

    // pub(crate) fn get_current_page(&self) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // // 从第n行开始获取内容
    // pub(crate) fn get_line_content(
    //     &self,
    //     line_num: usize,
    //     line_count: usize,
    // ) -> (&RingVec<CacheStr>, &RingVec<EditLineMeta>) {
    //     self.borrow_cache_lines_mut().clear();
    //     self.borrow_cache_line_meta_mut().clear();
    //     self.get_text(line_num, line_count, |txt, meta| {
    //         self.borrow_cache_lines_mut().push(CacheStr::from_str(txt));
    //         self.borrow_cache_line_meta_mut().push(meta);
    //     });
    //     (self.borrow_cache_lines(), self.borrow_cache_line_meta())
    // }

    // pub(crate) fn get_line_content_with_count(
    //     &self,
    //     line_num: usize,
    //     line_count: usize,
    // ) -> (Vec<CacheStr>, Vec<EditLineMeta>) {
    //     let mut lines = Vec::new();
    //     let mut lines_meta = Vec::new();
    //     self.get_text(line_num, line_count, |txt, meta| {
    //         lines.push(CacheStr::from_str(txt));
    //         lines_meta.push(meta);
    //     });
    //     (lines, lines_meta)
    // }

    // fn get_text<'a, F>(&'a self, line_num: usize, line_count: usize, mut f: F)
    // where
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     assert!(line_num >= 1);
    //     // 计算页码
    //     let page_num = self.get_page_num(line_num);
    //     // 计算页码
    //     let mut index = (page_num - 1) / PAGE_GROUP;
    //     let page_offset_list = self.borrow_page_offset_list();
    //     let page_offset = if index >= page_offset_list.len() {
    //         index = page_offset_list.len() - 1;
    //         page_offset_list.last().unwrap()
    //     } else {
    //         &page_offset_list[index]
    //     };
    //     let start_page_num = index * PAGE_GROUP;
    //     assert!(line_num >= start_page_num * self.height);
    //     //跳过的行数
    //     let skip_line = line_num;
    //     self.get_text_fn(
    //         &page_offset,
    //         line_count,
    //         start_page_num * self.height,
    //         start_page_num,
    //         skip_line,
    //         &mut f,
    //     );
    // }

    // fn get_text_fn<'a, F>(
    //     &'a self,
    //     page_offset: &PageOffset,
    //     line_count: usize,
    //     start_line_num: usize,
    //     start_page_num: usize,
    //     skip_line: usize,
    //     f: &mut F,
    // ) where
    //     // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     // if page_offset.line_index >= self.borrow_lines().len() {
    //     //     return;
    //     // }
    //     // let (a, b) = self
    //     //     .borrow_lines_mut()
    //     //     .split_at_mut(page_offset.line_index + 1);
    //     let mut cur_line_count = 0;
    //     let mut line_num = start_line_num;
    //     let mut page_num = start_page_num;

    //     let iter = self
    //         .borrow_lines_mut()
    //         .iter(page_offset.line_index, page_offset.line_start);

    //     for (i, v) in iter.enumerate() {
    //         let line_txt = if i == 0 {
    //             v.text(page_offset.line_start..)
    //         } else {
    //             v.text(..)
    //         };
    //         Self::set_line_txt(
    //             line_txt,
    //             page_offset.line_index + i,
    //             page_offset.line_start,
    //             self.with,
    //             self.height,
    //             self.borrow_page_offset_list_mut(),
    //             &mut line_num,
    //             line_count,
    //             &mut page_num,
    //             &mut cur_line_count,
    //             skip_line,
    //             f,
    //         );
    //         if cur_line_count >= line_count {
    //             return;
    //         }
    //     }

    //     // let line_txt = &a[page_offset.line_index].text()[page_offset.line_start..];
    //     // Self::set_line_txt(
    //     //     line_txt,
    //     //     page_offset.line_index,
    //     //     page_offset.line_start,
    //     //     self.with,
    //     //     self.height,
    //     //     self.borrow_page_offset_list_mut(),
    //     //     &mut line_num,
    //     //     line_count,
    //     //     &mut page_num,
    //     //     &mut cur_line_count,
    //     //     skip_line,
    //     //     f,
    //     // );
    //     // if cur_line_count >= line_count {
    //     //     return;
    //     // }
    //     // // 使用 split_at_mut 获取后续行的可变子切片
    //     // for (i, line) in b.iter_mut().enumerate() {
    //     //     let line_txt = line.text();
    //     //     Self::set_line_txt(
    //     //         line_txt,
    //     //         page_offset.line_index + i + 1,
    //     //         0,
    //     //         self.with,
    //     //         self.height,
    //     //         self.borrow_page_offset_list_mut(),
    //     //         &mut line_num,
    //     //         line_count,
    //     //         &mut page_num,
    //     //         &mut cur_line_count,
    //     //         skip_line,
    //     //         f,
    //     //     );
    //     //     if cur_line_count >= line_count {
    //     //         return;
    //     //     }
    //     // }
    // }

    // fn set_line_txt<'a, F>(
    //     line_txt: &'a str,
    //     line_index: usize,
    //     line_start: usize,
    //     with: usize,
    //     height: usize,
    //     page_offset_list: &mut Vec<PageOffset>,
    //     line_num: &mut usize,
    //     line_count: usize,
    //     page_num: &mut usize,
    //     cur_line_count: &mut usize,
    //     skip_line: usize,
    //     f: &mut F,
    // ) where
    //     // 使用高阶 trait bound，允许闭包接受任意较短生命周期的 &str
    //     F: FnMut(&'a str, EditLineMeta),
    // {
    //     //空行
    //     if line_txt.len() == 0 {
    //         *line_num += 1; //行数加1
    //         if *line_num >= skip_line {
    //             *cur_line_count += 1;
    //             f(
    //                 "",
    //                 EditLineMeta::new(0, 0, *page_num + 1, *line_num, line_index, 0),
    //             );
    //         }
    //         if *line_num % height == 0 {
    //             //到达一页
    //             *page_num += 1; //页数加1
    //             let m = *page_num / PAGE_GROUP;
    //             let n = *page_num % PAGE_GROUP;
    //             if n == 0 && m > page_offset_list.len() - 1 {
    //                 //保存页数的偏移量
    //                 page_offset_list.push(PageOffset {
    //                     line_index,
    //                     line_start: 0,
    //                 });
    //             }
    //         }

    //         if *cur_line_count >= line_count {
    //             return;
    //         }
    //         return;
    //     }

    //     let mut current_width = 0; //
    //     let mut line_offset = 0; //
    //     let mut current_bytes = 0;
    //     let mut char_index = 0;
    //     let mut char_count = 0;

    //     for (i, (byte_index, ch)) in line_txt.char_indices().enumerate() {
    //         let ch_width = ch.width().unwrap_or(0);
    //         //检查是否超过屏幕宽度
    //         if current_width + ch_width > with {
    //             let end = (line_offset + current_bytes).min(line_txt.len());
    //             *line_num += 1; //行数加1
    //             if *line_num >= skip_line {
    //                 *cur_line_count += 1;
    //                 let txt = &line_txt[line_offset..end];
    //                 f(
    //                     txt,
    //                     EditLineMeta::new(
    //                         txt.len(),
    //                         i - char_index,
    //                         *page_num + 1,
    //                         *line_num,
    //                         line_index,
    //                         line_start + line_offset,
    //                     ),
    //                 );
    //             }
    //             if *line_num % height == 0 {
    //                 //到达一页
    //                 *page_num += 1; //页数加1
    //                 let m = *page_num / PAGE_GROUP;
    //                 let n = *page_num % PAGE_GROUP;
    //                 if n == 0 && m > page_offset_list.len() - 1 {
    //                     //保存页数的偏移量
    //                     page_offset_list.push(PageOffset {
    //                         line_index,
    //                         line_start: line_start + byte_index,
    //                     });
    //                 }
    //             }
    //             if *cur_line_count >= line_count {
    //                 return;
    //             }
    //             char_index = i;
    //             line_offset += current_bytes;
    //             current_width = 0;
    //             current_bytes = 0;
    //         }
    //         char_count += 1;
    //         current_width += ch_width;
    //         current_bytes += ch.len_utf8();
    //     }
    //     //当前行没有到达屏幕宽度 但还是一行
    //     if current_bytes > 0 {
    //         *line_num += 1;

    //         if *line_num >= skip_line {
    //             let txt = &line_txt[line_offset..];
    //             *cur_line_count += 1;
    //             f(
    //                 txt,
    //                 EditLineMeta::new(
    //                     txt.len(),
    //                     char_count - char_index,
    //                     *page_num + 1,
    //                     *line_num,
    //                     line_index,
    //                     line_start + line_offset,
    //                 ),
    //             );
    //         }
    //         if *line_num % height == 0 {
    //             *page_num += 1; //页数加1
    //             let m = *page_num / PAGE_GROUP;
    //             let n = *page_num % PAGE_GROUP;
    //             if n == 0 && m > page_offset_list.len() - 1 {
    //                 //保存页数的偏移量
    //                 page_offset_list.push(PageOffset {
    //                     line_index: line_index + 1,
    //                     line_start: 0,
    //                 });
    //             }
    //         }
    //         if *cur_line_count >= line_count {
    //             return;
    //         }
    //     }
    // }
}

pub(crate) struct EditTextBuffer {
    lines: UnsafeCell<Vec<GapBuffer>>, // 每行使用 GapBuffer 存储
    cache_lines: UnsafeCell<RingVec<CacheStr>>, // 缓存行
    cache_line_meta: UnsafeCell<RingVec<EditLineMeta>>, // 缓存行
    page_offset_list: UnsafeCell<Vec<PageOffset>>, // 每页的偏移量
    height: usize,                     //最大行数
    with: usize,                       //最大列数
}

impl EditTextBuffer {}

#[derive(Debug, Clone, Copy)]
struct PageOffset {
    line_index: usize,      //第多少行
    line_offset: usize,     //行在总行的起始位置
    line_file_start: usize, //这一行在整个文件的起始位置
}

#[cfg(test)]
mod tests {

    use ratatui::text;

    use super::*;
    use std::fs::File;

    #[test]
    fn text_hex_get() {
        let hex_text = TextWarp::new(
            HexText::from_file_path("/root/20250704120009_481.jpg").unwrap(),
            48,
            0,
            TextWarpType::NoWrap,
        );
        let line = hex_text.get_one_page(1).unwrap();
        let line = hex_text.get_one_page(93).unwrap();
        println!("=======================================");
        for s in line.0.iter() {
            let (a, b) = s.as_slice();
            for c in a.iter() {
                print!("{:02x} ", c);
            }
            print!("\n");
        }
    }

    #[test]
    fn text_hex_sel() {
        let mut hex = HexText::from_file_path("/root/20250704120009_481.jpg").unwrap();

        let sel = TextSelect::from_select(100, 1000);
        for s in hex.chunks.iter() {
            println!("chunk: {:?}", s.buffer.text(..).to_vec());
        }
        let text = hex.text_from_sel(&sel);
        println!("text1 len  {:?}", text.len());
        println!("text1  {:?}\n", text);

        let sel = TextSelect::from_select(0, 3);
        for s in hex.chunks.iter() {
            println!("chunk: {:?}", s.buffer.text(..).to_vec());
        }
        let text = hex.text_from_sel(&sel);
        println!("text2 len  {:?}", text.len());
        println!("text2  {:?}\n", text);

        let sel = TextSelect::from_select(2, 6);
        for s in hex.chunks.iter() {
            println!("chunk: {:?}", s.buffer.text(..).to_vec());
        }
        let text = hex.text_from_sel(&sel);
        println!("text3 len  {:?}", text.len());
        println!("text3  {:?}\n", text);

        let sel = TextSelect::from_select(8, 9);
        for s in hex.chunks.iter() {
            println!("chunk: {:?}", s.buffer.text(..).to_vec());
        }
        let text = hex.text_from_sel(&sel);
        println!("text4 len  {:?}", text.len());
        println!("text4  {:?}\n", text);

        let sel = TextSelect::from_select(0, 0);
        for s in hex.chunks.iter() {
            println!("chunk: {:?}", s.buffer.text(..).to_vec());
        }
        let text = hex.text_from_sel(&sel);
        println!("text5 len  {:?}", text.len());
        println!("text5  {:?}\n", text);
    }

    #[test]
    fn text_hex() {
        let mut hex = HexText::from_file_path("/root/aa.txt").unwrap();
        println!("hex len: {:?}", hex.chunks.get(0).unwrap().buffer.text(..));
        let iter = hex.iter(0, 10, 0);
        for i in iter {
            println!("i: {:?}", i.line_data.as_str());
        }
    }

    #[test]
    fn test_print() {
        let file = File::open("/root/aa.txt").unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        println!(" mmap len: {:?}", mmap.len());
        let mmap_text = MmapText::new(mmap);

        let text = TextWarp::new(mmap_text, 2, 5, TextWarpType::NoWrap);
        let (s, c) = text.get_one_page(1).unwrap();
        for (i, l) in s.iter().enumerate() {
            println!("l: {:?},{:?}", l.as_str(), c.get(i));
        }

        for p in text.borrow_page_offset_list().iter() {
            println!("p:{:?}", p)
        }

        let (s, c) = text.get_next_line(c.last().unwrap(), 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);

        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
        let (s, c) = text.get_next_line(&c, 1);
        println!("s:{:?},c:{:?}", s.unwrap().as_str(), c);
    }

    #[test]
    fn test_ringcache() {
        let mut ring_cache = RingVec::<usize>::new(8);
        for i in 0..11 {
            ring_cache.push(i);
        }

        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }

        println!("{:?}", ring_cache.cache);

        ring_cache.push_front(0);

        for i in ring_cache.iter() {
            println!("i1: {:?}", i);
        }

        ring_cache.push_front(11);
        ring_cache.push_front(12);
        ring_cache.push_front(13);
        ring_cache.push_front(14);
        ring_cache.push_front(15);
        ring_cache.push_front(16);
        ring_cache.push_front(17);
        ring_cache.push_front(18);
        ring_cache.push_front(19);

        for i in ring_cache.iter() {
            println!("i2: {:?}", i);
        }

        println!("{:?}", ring_cache.get(0));
        println!("{:?}", ring_cache.get(1));
        println!("{:?}", ring_cache.get(2));
        println!("{:?}", ring_cache.get(3));
        println!("{:?}", ring_cache.get(4));
        println!("{:?}", ring_cache.get(5));
        println!("{:?}", ring_cache.get(6));
        println!("{:?}", ring_cache.get(7));
        println!("{:?}", ring_cache.get(8));
        println!("{:?}", ring_cache.get(9));
        println!("{:?}", ring_cache.get(10));
        println!("{:?}", ring_cache.get(11));

        ring_cache.remove(0);

        for i in ring_cache.iter() {
            println!("i3: {:?}", i);
        }

        ring_cache.push(20);

        for i in ring_cache.iter() {
            println!("i4: {:?}", i);
        }

        // println!("{:?}", ring_cache.last());
    }
    #[test]
    fn test_remove() {
        let mut ring_cache = RingVec::<usize>::new(8);
        for i in 0.. {
            ring_cache.push(i);
        }

        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }

        println!("{:?}", ring_cache.cache);
        ring_cache.remove(0);

        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }

        println!("{:?}", ring_cache.cache);

        ring_cache.push(20);
        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }
        println!("{:?}", ring_cache.cache);

        ring_cache.remove(0);

        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }

        println!("{:?}", ring_cache.cache);

        ring_cache.push(30);
        for i in ring_cache.iter() {
            println!("i: {:?}", i);
        }
        println!("{:?}", ring_cache.cache);
    }

    #[test]
    fn test_remove2() {
        let mut ring_cache = RingVec::<usize>::new(3);
        for i in 0..3 {
            ring_cache.push(i);
        }

        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        println!("{:?},{}", ring_cache.cache, ring_cache.start);
        ring_cache.remove(7);

        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        println!("{:?},{}", ring_cache.cache, ring_cache.start);

        ring_cache.push_front(20);
        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }
        println!("push_front {:?}", ring_cache.cache);

        ring_cache.remove(7);

        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        println!("{:?},{}", ring_cache.cache, ring_cache.start);

        ring_cache.push_front(30);
        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        ring_cache.remove(7);
        ring_cache.push_front(40);

        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        println!("push_front{:?}", ring_cache.cache);

        ring_cache.remove(7);
        ring_cache.push_front(50);

        for i in ring_cache.iter().enumerate() {
            println!("i: {:?}", i);
        }

        println!("push_front{:?}", ring_cache.cache);
    }

    // #[test]
    // fn test_print3() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     let c = {
    //         let (s, c) = b.get_line_content_with_count(1, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         for p in b.borrow_page_offset_list().iter() {
    //             println!("p:{:?}", p)
    //         }
    //         c
    //     };
    //     let (s, m) = b.get_pre_line(&c.last().unwrap(), 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);
    //     let (s, m) = b.get_pre_line(&m, 1);
    //     println!("{:?},{:?}", s.unwrap().as_str(), m);

    //     // for p in b.borrow_page_offset_list().iter() {
    //     //     println!("p:{:?}", p)
    //     // }
    // }

    // #[test]
    // fn test_print4() {
    //     let mut b = EditTextBuffer::from_file_path("/root/aa.txt", 2, 5).unwrap();

    //     let c = {
    //         let (s, c) = b.get_line_content_with_count(1, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l0: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         let (s, c) = b.get_line_content_with_count(4, 11);
    //         for (i, l) in s.iter().enumerate() {
    //             println!("l1: {:?}{:?}", l.as_str(), c.get(i).unwrap());
    //         }

    //         for p in b.borrow_page_offset_list().iter() {
    //             println!("p:{:?}", p)
    //         }
    //         c
    //     };
    // }

    #[test]
    fn test_mmap() {
        let path = "/root/aa.txt";
        let file = std::fs::File::open(path).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        //let
    }
}
