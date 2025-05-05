use std::fmt::Display;
use std::fmt::{Debug, Write};

use std::borrow::Cow;
use utf8_iter::Utf8CharIndices;
use utf8_iter::Utf8CharsEx;

use crate::editor::{Line, LineData, LineStr};

pub(crate) struct GapBytes<'a>(&'a [u8], &'a [u8]);

impl<'a> GapBytes<'a> {
    pub(crate) fn new(left: &'a [u8], right: &'a [u8]) -> GapBytes<'a> {
        GapBytes(left, right)
    }

    pub(crate) fn empty() -> GapBytes<'a> {
        GapBytes(&[], &[])
    }

    pub(crate) fn as_str(&self) -> (Cow<str>, Cow<str>) {
        let str1 = String::from_utf8_lossy(self.left());
        let str2 = String::from_utf8_lossy(self.right());

        (str1, str2)
    }

    pub(crate) fn text(&self, range: impl std::ops::RangeBounds<usize>) -> GapBytes<'a> {
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
        assert!(start <= end && end <= self.len());

        if start < self.left().len() {
            if end <= self.left().len() {
                GapBytes::new(&self.0[start..end], &[])
            } else {
                GapBytes(self.0, &self.1[..end - self.left().len()])
            }
        } else if self.right().len() > 0 {
            GapBytes(
                &[],
                &self.1[start - self.left().len()..end - self.left().len()],
            )
        } else {
            return GapBytes(&[], &[]);
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len() + self.1.len()
    }

    pub(crate) fn left(&self) -> &[u8] {
        self.0
    }

    pub(crate) fn right(&self) -> &[u8] {
        self.1
    }

    pub(crate) fn iter(&self) -> GapBytesIter<'_> {
        GapBytesIter {
            left: self.left().iter(),
            right: self.right().iter(),
        }
    }

    pub(crate) fn char_indices(&self) -> GapBytesCharIter<'_> {
        GapBytesCharIter {
            left: self.left().char_indices(),
            right: self.right().char_indices(),
            left_bytes: self.left().len(),
        }
    }
}

pub(crate) struct GapBytesCharIter<'a> {
    left: Utf8CharIndices<'a>,
    right: Utf8CharIndices<'a>,
    left_bytes: usize,
}

impl<'a> Iterator for GapBytesCharIter<'a> {
    type Item = (usize, char);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((index, byte)) = self.left.next() {
            Some((index, byte))
        } else {
            self.right
                .next()
                .map(|(index, byte)| (index + self.left_bytes, byte))
        }
    }
}

pub(crate) struct GapBytesIter<'a> {
    left: std::slice::Iter<'a, u8>,
    right: std::slice::Iter<'a, u8>,
}

impl Iterator for GapBytesIter<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(&byte) = self.left.next() {
            Some(byte)
        } else {
            self.right.next().copied()
        }
    }
}

impl<'a> Debug for GapBytes<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{}{}",
            std::str::from_utf8(self.left()).unwrap(),
            std::str::from_utf8(self.right()).unwrap()
        ))
    }
}

impl<'a> Display for GapBytes<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{:?}{:?}", self.left(), self.right()))
    }
}

pub(crate) struct GapBuffer {
    buffer: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
}

impl Line for GapBuffer {
    fn text_len(&self) -> usize {
        self.buffer.len() - (self.gap_end - self.gap_start)
    }

    fn text(&self, range: impl std::ops::RangeBounds<usize>) -> GapBytes<'_> {
        self.get_text(range)
    }
}

impl GapBuffer {
    pub(crate) fn new(size: usize) -> GapBuffer {
        GapBuffer {
            buffer: vec![0u8; size],
            gap_start: 0,
            gap_end: size,
        }
    }

    fn gap_size(&self) -> usize {
        self.gap_end - self.gap_start
    }

    fn get_text(&self, range: impl std::ops::RangeBounds<usize>) -> GapBytes<'_> {
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
        assert!(start <= end && end <= self.text_len());
        if start < self.gap_start {
            if end <= self.gap_start {
                GapBytes(&self.buffer[start..end], &[])
            } else {
                GapBytes(
                    &self.buffer[start..self.gap_start],
                    &self.buffer[self.gap_end..end + self.gap_size()],
                )
            }
        } else {
            GapBytes(
                &[],
                &self.buffer[start + self.gap_size()..end + self.gap_size()],
            )
        }
    }

    pub(crate) fn get_buffer(&self) -> &[u8] {
        &self.buffer
    }

    pub(crate) fn get_line_str<'a>(&'a mut self) -> LineStr<'a> {
        LineStr {
            // line: self.text(..),
            line_data: LineData::GapBytes(self.text(..)),
            line_file_start: 0,
            line_file_end: 0,
        }
    }

    // pub(crate) fn text_len(&self) -> usize {
    //     self.buffer.len() - (self.gap_end - self.gap_start)
    // }

    /// Move the gap to the specified index
    /// [H][e][l][l][o][ ][ ][ ][ ][ ][W][o][r][l][d]
    /// [H][e][l][ ][ ][ ][ ][ ][l][o][W][o][r][l][d]
    pub(crate) fn move_gap_to(&mut self, index: usize) {
        if index > self.text_len() {
            return;
        }
        // Move the gap to the left
        if index < self.gap_start {
            let shift = self.gap_start - index;
            for i in 0..shift {
                self.buffer[self.gap_end - 1 - i] = self.buffer[self.gap_start - 1 - i];
            }
            self.gap_start = index;
            self.gap_end -= shift;
        } else if index > self.gap_start {
            // Move the gap to the right
            let shift = index - self.gap_start;
            for i in 0..shift {
                self.buffer[self.gap_start + i] = self.buffer[self.gap_end + i];
            }
            self.gap_start += shift;
            self.gap_end += shift;
        }
    }

    /// Move the gap to the end of the buffer
    /// [H][e][l][l][o][ ][ ][ ][ ][ ][W][o][r][l][d]
    /// [H][e][l][l][o][W][o][r][l][d][ ][ ][ ][ ][ ]
    pub(crate) fn move_gap_to_last(&mut self) {
        self.move_gap_to(self.text_len());
    }

    pub(crate) fn expand_gap(&mut self, n: usize) {
        if self.gap_end - self.gap_start >= n {
            return;
        }
        let mut new_cap = self.buffer.len() * 2;
        if new_cap < self.buffer.len() + n {
            new_cap = self.buffer.len() + n;
        }
        let mut new_buffer = vec![0u8; new_cap];
        //复制 gap 前部分
        new_buffer[0..self.gap_start].copy_from_slice(&self.buffer[0..self.gap_start]);
        //复制 gap 后部分
        let new_gap_end = new_cap - (self.buffer.len() - self.gap_end);
        new_buffer[new_gap_end..new_cap].copy_from_slice(&self.buffer[self.gap_end..]);
        //更新 gap 的位置
        self.buffer = new_buffer;
        self.gap_end = new_gap_end;
    }

    /// 插入文本到指定位置
    /// [H][e][l][l][o][ ][ ][ ][ ][ ][W][o][r][l][d]
    pub(crate) fn insert(&mut self, index: usize, text: &[u8]) {
        if text.len() == 0 {
            return;
        }
        self.move_gap_to(index);
        let text_bytes = text;
        let text_len = text_bytes.len();
        if self.gap_end - self.gap_start < text_len {
            self.expand_gap(text_len);
        }
        // Move the gap to the right
        for i in 0..text_len {
            self.buffer[self.gap_start + i] = text_bytes[i];
        }
        self.gap_start += text_len;
    }

    // Backspace 删除光标前一个字符
    /// [H][e][l][l][o][ ][ ][ ][ ][ ][W][o][r][l][d]
    pub(crate) fn backspace(&mut self, index: usize) {
        self.delete(index, 1);
    }

    /// 删除index处前len个字符
    pub(crate) fn delete(&mut self, mut index: usize, len: usize) {
        if index == 0 {
            return;
        }
        if index < len {
            index = len
        }
        self.move_gap_to(index);
        self.gap_start = self.gap_start.saturating_sub(len)
    }

    // Move the gap to the end of the buffer
    // pub(crate) fn text(&mut self) -> &str {
    //     self.move_gap_to_last();
    //     std::str::from_utf8(&self.buffer[..self.gap_start]).unwrap()
    // }
}

#[cfg(test)]
mod tests {

    use super::*;
    #[test]
    fn test_insert() {
        let mut gb = GapBuffer::new(15);
        gb.insert(0, "helloworld".as_bytes());
        gb.insert(2, "w".as_bytes());
        println!("{:?},{},{}", gb.get_buffer(), gb.gap_start, gb.gap_end);
        println!("{:?}", gb.get_text(..));
        println!("{:?}", gb.get_text(0..2));
        println!("{:?}", gb.get_text(4..=6));
        println!("{:?}", gb.get_text(4..=9));
        println!("{:?}", gb.get_text(3..=7));
        println!("{:?}", gb.get_text(9..11));
    }
    use super::*;
    use utf8_iter::Utf8CharsEx;
    #[test]
    fn test_iter() {
        let mut gb = GapBuffer::new(15);
        gb.insert(0, "helloworld".as_bytes());
        gb.insert(2, "我们".as_bytes());
        println!("{:?}", gb.get_text(..));
        let s = gb.get_text(..);
        for (i, byte) in s.char_indices().enumerate() {
            println!("{}: {:?}", i, byte);
        }
    }

    #[test]
    fn test_delete() {
        // let mut gb = GapBuffer::new(10);
        // gb.insert(0, "Hello".as_bytes());
        // println!("{}", gb.text_str(..));
        // gb.delete(1, 1);
        // println!("{}", gb.text_str(..));
    }
}
