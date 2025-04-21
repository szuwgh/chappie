use crate::editor::{Line, LineStr};
pub(crate) struct GapBuffer {
    buffer: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
}

impl Line for GapBuffer {
    fn text_len(&self) -> usize {
        self.buffer.len() - (self.gap_end - self.gap_start)
    }

    fn text(&mut self, range: impl std::ops::RangeBounds<usize>) -> &str {
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
        self.move_gap_to(end);
        std::str::from_utf8(&self.buffer[start..end]).unwrap()
    }
}

impl GapBuffer {
    pub(crate) fn new(size: usize) -> GapBuffer {
        GapBuffer {
            buffer: vec![0; size],
            gap_start: 0,
            gap_end: size,
        }
    }

    pub(crate) fn get_buffer(&self) -> &[u8] {
        &self.buffer
    }

    pub(crate) fn get_line_str<'a>(&'a mut self) -> LineStr<'a> {
        LineStr {
            line: self.text(..),
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

    pub(crate) fn expandGap(&mut self, n: usize) {
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
    pub(crate) fn insert(&mut self, index: usize, text: &str) {
        if text.len() == 0 {
            return;
        }
        self.move_gap_to(index);
        let text_bytes = text.as_bytes();
        let text_len = text_bytes.len();
        if self.gap_end - self.gap_start < text_len {
            self.expandGap(text_len);
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
        let mut gb = GapBuffer::new(10);
        gb.insert(0, "1234我是");

        println!("{}", gb.text(4..));
    }

    #[test]
    fn test_delete() {
        let mut gb = GapBuffer::new(10);
        gb.insert(0, "Hello");
        println!("{}", gb.text(..));
        gb.delete(1, 1);
        println!("{}", gb.text(..));
    }
}
