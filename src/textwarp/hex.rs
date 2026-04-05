use ratatui::symbols::block;
use ratatui::symbols::line;

use crate::common::error::ChapError;
use crate::common::gap_buffer::GapBuffer;
use crate::textwarp::block::Block;
use crate::textwarp::ChapResult;
use crate::textwarp::EditText;
use crate::textwarp::Line;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStr;
use crate::textwarp::RingVec;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use crate::textwarp::CHUNK_NUM;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::path::Path;
//const PAGE_GROUP: usize = 1;
const HEX_CHUNK_SIZE: usize = 4 * 1024; // 每个块的大小
const HEX_GAP_SIZE: usize = 5;
pub(crate) const HEX_WITH: usize = 16; //16进制文本的宽度

// #[derive(Clone)]
// pub(crate) struct Block {
//     buffer: GapBuffer,
//     file_start: usize,
//     file_end: usize,
//     is_modified: bool,
// }

// impl Block {
//     fn text(&self, range: impl RangeBounds<usize>) -> GapBytes<'_> {
//         let start = match range.start_bound() {
//             std::ops::Bound::Included(&start) => start,
//             std::ops::Bound::Excluded(&start) => start + 1,
//             std::ops::Bound::Unbounded => self.file_start,
//         };
//         let end = match range.end_bound() {
//             std::ops::Bound::Included(&end) => end, //包含
//             std::ops::Bound::Excluded(&end) => end, //排除
//             std::ops::Bound::Unbounded => self.file_end,
//         };
//         assert!(start <= end && start >= self.file_start && end >= self.file_start);
//         self.buffer
//             .text((start - self.file_start)..(end - self.file_start))
//     }

//     fn text_len(&self) -> usize {
//         self.buffer.text_len()
//     }
// }

pub(crate) struct HexText {
    blocks: RingVec<Block>,       // 每个块4KB大小
    chk_iter: Block,              // 当前迭代的块 这个后续可以优化
    file: File,                   // 文件句柄
    cache: HashMap<usize, Block>, // 缓存已修改的块
    file_size: usize,             // 文件大小
    height: usize,                //显示高度
}

impl HexText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(
        filename: P,
        height: usize,
    ) -> ChapResult<HexText> {
        let mut file = File::open(filename)?;
        let file_size = file.metadata()?.len() as usize;
        let mut blocks = RingVec::with_capacity(CHUNK_NUM);

        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let mut bytes_start = 0;
        for _ in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < HEX_CHUNK_SIZE {
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
            blocks.push(Block {
                data: buffer,
                file_start: bytes_start,
                file_end: bytes_start + bytes_read,
                block_id: blocks.len(),
                is_modified: false,
            });
            bytes_start += bytes_read;
        }
        let chk_iter = blocks
            .get(0)
            .unwrap_or(&Block {
                data: GapBuffer::new(0),
                file_start: 0,
                file_end: 0,
                block_id: 0,
                is_modified: false,
            })
            .clone();
        Ok(HexText {
            blocks: blocks,
            chk_iter: chk_iter,
            file,
            cache: HashMap::new(),
            file_size,
            height: height, // 初始高度为0，可以根据需要设置
        })
    }

    pub(crate) fn get_file_size(&self) -> usize {
        self.file_size
    }

    pub(crate) fn reset_chunks(&mut self, line_file_start: usize) {
        let n = (self.file_size + HEX_CHUNK_SIZE - 1) / HEX_CHUNK_SIZE;
        let last_chunk_address = (n - CHUNK_NUM) * HEX_CHUNK_SIZE;
        //如果没有找到块 从新重读chunks
        //通过line_file_start 计算在哪一个块 每个块的大小是 HEX_CHUNK_SIZE
        let chunk_start =
            (line_file_start / HEX_CHUNK_SIZE * HEX_CHUNK_SIZE).min(last_chunk_address);
        let block_num = chunk_start / HEX_CHUNK_SIZE;
        self.read_chunks(block_num, chunk_start).unwrap();
    }

    pub(crate) fn read_one_chunk(
        &mut self,
        block_num: usize,
        chunk_seek: usize,
    ) -> ChapResult<Block> {
        self.file
            .seek(std::io::SeekFrom::Start(chunk_seek as u64))?;
        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let bytes_start = chunk_seek;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                //跳出 for 循环
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }
        if bytes_read == 0 {
            return Err(ChapError::Unexpected("No data read from file".to_string()).into());
        }
        return Ok(Block {
            data: buffer,
            file_start: bytes_start,
            file_end: bytes_start + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
    }

    pub(crate) fn read_chunks(&mut self, block_num: usize, chunk_seek: usize) -> ChapResult<()> {
        self.file
            .seek(std::io::SeekFrom::Start(chunk_seek as u64))?;
        let mut blocks = RingVec::with_capacity(CHUNK_NUM);

        let mut buf = [0u8; HEX_CHUNK_SIZE];
        let mut bytes_start = chunk_seek;
        for i in 0..CHUNK_NUM {
            let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
            let mut bytes_read = 0;
            while bytes_read < HEX_CHUNK_SIZE {
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
            blocks.push(Block {
                data: buffer,
                file_start: bytes_start,
                file_end: bytes_start + bytes_read,
                block_id: block_num + i,
                is_modified: false,
            });
            bytes_start += bytes_read;
        }
        self.blocks = blocks;
        return Ok(());
    }

    pub(crate) fn read_last_chunk(&mut self, block_num: usize, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出最后一个块
        let block2 = self.blocks.remove_last();
        if let Some(c) = block2 {
            if c.is_modified {
                self.cache.insert(c.file_start, c);
            }
        }
        self.blocks.push_front(Block {
            data: buffer,
            file_start: file_seek,
            file_end: file_seek + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
        Ok(())
    }

    pub(crate) fn read_next_chunk(&mut self, block_num: usize, file_seek: usize) -> ChapResult<()> {
        if file_seek >= self.file_size {
            return Ok(());
        }
        self.file.seek(std::io::SeekFrom::Start(file_seek as u64))?;
        let mut buffer = GapBuffer::new(HEX_CHUNK_SIZE + HEX_GAP_SIZE);
        let mut bytes_read = 0;
        let mut buf = [0u8; 1024];
        while bytes_read < HEX_CHUNK_SIZE {
            let n = self.file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            bytes_read += n;
            buffer.insert(buffer.text_len(), &buf[..n]);
        }

        //弹出第一个块
        let block0 = self.blocks.remove(0);
        if let Some(c) = block0 {
            if c.is_modified {
                self.cache.insert(c.file_start, c);
            }
        }
        self.blocks.push(Block {
            data: buffer,
            file_start: file_seek,
            file_end: file_seek + bytes_read,
            block_id: block_num,
            is_modified: false,
        });
        Ok(())
    }
}

impl TextIndex for HexText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        let start_page_num = line_num / self.height;
        let start_line_num = (start_page_num * self.height).saturating_sub(1);
        let line_file_start = start_line_num * HEX_WITH;
        let state = LineState::builder()
            .line_index(0)
            .line_offset(0)
            .block_num(0)
            .block_line_index(0)
            .block_offset(0)
            .line_file_start(line_file_start)
            .start_line_num(start_line_num)
            .start_page_num(start_page_num)
            .build();
        state
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // 这里可以实现设置页偏移的逻辑
        // 目前没有具体实现
    }
}

impl EditText for HexText {
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        mut count: usize,
        line_meta: &LineState,
    ) -> ChapResult<Vec<u8>> {
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;

        for _ in 0..2 {
            let cur_block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_num)
                .unwrap();
            let cur_block_size = cur_block.block_size();
            if insert_offset < cur_block.block_size() {
                if count < insert_offset {
                    cur_block.backspace(insert_offset, count);
                    break;
                } else {
                    cur_block.backspace(insert_offset, insert_offset);
                    block_num = block_num.saturating_sub(1);
                    count = count - insert_offset;
                    insert_offset = cur_block_size - 1;
                }
            } else {
                insert_offset = insert_offset - cur_block.block_size();
                block_num += 1;
                let next_block = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.block_id == block_num)
                    .unwrap();
                if count > insert_offset {
                    next_block.backspace(insert_offset, insert_offset);
                    block_num = block_num.saturating_sub(1);
                    count = count - insert_offset;
                    insert_offset = cur_block_size - 1;
                } else {
                    next_block.backspace(insert_offset, count);
                    break;
                }
            }
        }

        Ok(vec![])
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: &[u8],
        is_overwrite: bool,
    ) -> ChapResult<()> {
        if is_overwrite {
            //覆盖模式 先删除后插入
            self.backspace(cursor_y, bytes_cursor + c.len(), c.len(), line_meta)?;
        }
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;

        let cur_block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_num)
            .unwrap();
        if insert_offset < cur_block.block_size() {
            cur_block.insert(insert_offset, c);
        } else {
            insert_offset = insert_offset - cur_block.block_size();
            block_num += 1;
            let next_block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_num)
                .unwrap();
            next_block.insert(insert_offset, c);
        }

        Ok(())
    }

    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        //判断c是否是十六进制字符
        // if !c.is_ascii_hexdigit() {
        //     return Err(ChapError::InvalidHexChar(c));
        // }
        // let mut block_num = line_meta.get_block_num();
        // let block_offset = line_meta.get_block_offset();
        // let mut block_line_index = line_meta.get_block_line_index();
        // let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        Ok(())
    }

    fn rollback() -> ChapResult<()> {
        Ok(())
    }

    fn ensure_block_loaded(&mut self, _block_num: usize) -> ChapResult<()> {
        Ok(())
    }
}

impl Text for HexText {
    type LineItem<'a> = LineStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        let mut line_index = state.get_line_index();
        let mut line_end = state.get_line_end();
        let mut line_file_start = state.get_line_file_start();

        let line = self.get_line(&state).unwrap();
        if state.get_line_end() == line.text_len() {
            line_file_start = state.get_line_file_end();
            line_end = 0;
            line_index += 1;
        }

        let p = LineState::builder()
            .line_index(line_index)
            .line_offset(line_end)
            .line_file_start(line_file_start)
            .start_line_num(state.get_line_num())
            // .start_page_num(state.get_line_num() / self.height)
            .build();
        Some(p)
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        if meta.get_line_index() == 0 && meta.get_line_end() == 0 {
            return false;
        }
        true
    }

    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState> {
        None
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut start = sel.get_start();
        let end = sel.get_end();
        for s in self.blocks.iter() {
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
            buf.extend_from_slice(&s.data.text(from..=to).to_vec());
            // 如果选区在此块内完全结束，则跳出循环
            if end <= s.file_end {
                break;
            }
            // 更新起点为当前块末尾，继续下一块
            start = s.file_end;
        }
        buf
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<LineStr<'a>> {
        let with = state.line_file_end - state.line_file_start;
        for (i, b) in self.blocks.iter().enumerate() {
            if state.line_file_start > b.file_end || state.line_file_start < b.file_start {
                continue;
            }
            let buffer = b.text_from_file_seek(state.line_file_start..);
            let line_start = state.line_file_start;
            // 如果不足以填充 with 宽度 说明是跨块了
            if with > buffer.len() {
                let len = buffer.len();
                // 一行跨两个块数据
                if i < self.blocks.len() - 1 {
                    if let Some(c1) = self.blocks.get(i + 1) {
                        let mut v: Vec<u8> = Vec::with_capacity(with);
                        v.extend_from_slice(buffer.left());
                        v.extend_from_slice(buffer.right());
                        let remaining = with - buffer.len();
                        let buf1 = c1.text_from_file_seek(state.line_file_start + len..);
                        if remaining >= buf1.len() {
                            v.extend_from_slice(buf1.left());
                            v.extend_from_slice(buf1.right());
                            return Some(LineStr {
                                data: LineData::Own(v),
                                block_num: b.block_id,
                                block_offset: state.line_file_start - b.file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + len + buf1.len(),
                            });
                        } else {
                            let buf2 = buf1.text(..remaining);
                            v.extend_from_slice(buf2.left());
                            v.extend_from_slice(buf2.right());
                            return Some(LineStr {
                                data: LineData::Own(v),
                                block_num: b.block_id,
                                block_offset: state.line_file_start - b.file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + with,
                            });
                        }
                    } else {
                        return Some(LineStr {
                            data: LineData::GapBytes(buffer),
                            block_num: b.block_id,
                            block_offset: state.line_file_start - b.file_start,
                            line_file_start: line_start,
                            line_file_end: state.line_file_end,
                        });
                    }
                } else {
                    return Some(LineStr {
                        data: LineData::GapBytes(buffer),
                        block_num: b.block_id,
                        block_offset: state.line_file_start - b.file_start,
                        line_file_start: line_start,
                        line_file_end: line_start + len,
                    });
                }
            } else {
                return Some(LineStr {
                    data: LineData::GapBytes(buffer.text(..with)),
                    block_num: b.block_id,
                    block_offset: state.line_file_start - b.file_start,
                    line_file_start: line_start,
                    line_file_end: line_start + with,
                });
            }
        }
        return Some(LineStr {
            // line: buffer,
            data: LineData::Bytes(&[]),
            block_num: 0,
            block_offset: 0,
            line_file_start: 0,
            line_file_end: 0,
        });
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        if meta.line_file_end >= self.file_size {
            return false;
        }
        return true;
    }

    fn iter_rev<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.iter(line_state)
    }

    fn iter<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        let mut j = None;
        loop {
            if line_state.line_file_start >= self.file_size {
                return HexTextIter::new([None, None], HEX_WITH, line_state.line_file_start);
            }
            for (i, block) in self.blocks.iter().enumerate() {
                if line_state.line_file_start >= block.file_start
                    && line_state.line_file_start < block.file_end
                {
                    j = Some(i);
                    break;
                }
            }
            if let Some(j) = j {
                if j == 0 {
                    let b = self.blocks.get(0).unwrap();
                    let block_seek = b.file_start;
                    let block_num = b.block_id;
                    if block_seek == 0 {
                        //已经是第一个块无需弹出
                        return HexTextIter::new(
                            [self.blocks.get(0), self.blocks.get(1)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    } else {
                        //读取上一个块 把最后一个块弹出
                        let last_block_seek = block_seek.saturating_sub(HEX_CHUNK_SIZE);
                        self.read_last_chunk(block_num - 1, last_block_seek)
                            .unwrap();
                        // for c in self.blocks.iter() {}
                        return HexTextIter::new(
                            [self.blocks.get(1), self.blocks.get(2)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    }
                } else if j == self.blocks.len() - 1 {
                    //最后一个块
                    //读取下一个块 把第一个块弹出
                    let next_file_seek = self.blocks.get(j).unwrap().file_end;
                    let block_num = self.blocks.get(j).unwrap().block_id;
                    if next_file_seek >= self.file_size {
                        return HexTextIter::new(
                            [self.blocks.get(j), None],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    } else {
                        self.read_next_chunk(block_num + 1, next_file_seek).unwrap();
                        return HexTextIter::new(
                            [self.blocks.get(j - 1), self.blocks.get(j)],
                            HEX_WITH,
                            line_state.line_file_start,
                        );
                    }
                } else {
                    //不是最后一个块
                    return HexTextIter::new(
                        [self.blocks.get(j), self.blocks.get(j + 1)],
                        HEX_WITH,
                        line_state.line_file_start,
                    );
                }
            }
            self.reset_chunks(line_state.line_file_start);
        }
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        let mut j = None;
        for (i, block) in self.blocks.iter().enumerate() {
            if line_file_start >= block.file_start && line_file_start < block.file_end {
                j = Some(i);
                break;
            }
        }
        if let Some(j) = j {
            self.chk_iter = self.blocks.get(j).unwrap().clone();
            return HexTextU8Iter::new(
                self,
                line_file_start - self.blocks.get(j).unwrap().file_start,
            );
        }
        // 如果没有找到块 从新重读chunks
        // 通过line_file_start 计算在哪一个块 每个块的大小是 HEX_CHUNK_SIZE
        let chunk_start = line_file_start / HEX_CHUNK_SIZE * HEX_CHUNK_SIZE;
        let block_num = chunk_start / HEX_CHUNK_SIZE;
        let chk_iter = self.read_one_chunk(block_num, chunk_start).unwrap();
        self.chk_iter = chk_iter;
        return HexTextU8Iter::new(self, line_file_start - self.chk_iter.file_start);
    }
}

struct HexTextU8Iter<'a> {
    hex_text: &'a mut HexText,
    i: usize,
}

impl<'a> HexTextU8Iter<'a> {
    fn new(hex_text: &'a mut HexText, i: usize) -> HexTextU8Iter<'a> {
        HexTextU8Iter { hex_text, i: i }
    }
}

impl<'a> Iterator for HexTextU8Iter<'a> {
    type Item = u8;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            // 获取当前 chunk 并从中读取一字节
            let chunk = &self.hex_text.chk_iter;
            let buffer = chunk.data.text(..);
            let (a, b) = buffer.as_slice();
            let total = a.len() + b.len();

            if self.i < total {
                let byte = if self.i < a.len() {
                    a[self.i]
                } else {
                    b[self.i - a.len()]
                };
                self.i += 1;
                return Some(byte);
            }
            let next_block_seek = self.hex_text.chk_iter.file_end;
            let next_block_num = self.hex_text.chk_iter.block_id + 1;
            if let Ok(chk) = self
                .hex_text
                .read_one_chunk(next_block_num, next_block_seek)
            {
                self.hex_text.chk_iter = chk;
            } else {
                return None; // 如果没有更多数据，返回 None
            }
            self.i = 0;
        }
    }
}

struct HexTextEmptyIter;

impl Iterator for HexTextEmptyIter {
    type Item = LineStr<'static>;

    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

struct HexTextIter<'a> {
    hex_chunk: [Option<&'a Block>; 2],
    with: usize,
    line_file_start: usize,
}

impl<'a> HexTextIter<'a> {
    fn new(
        hex_chunk: [Option<&'a Block>; 2],
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
        for (i, block) in self.hex_chunk.iter().enumerate() {
            if let Some(b) = block {
                if self.line_file_start >= b.file_end || self.line_file_start < b.file_start {
                    continue;
                }
                let line_start = self.line_file_start;
                let buffer = b.text_from_file_seek(line_start..);
                // 长度大于 buffer 说明当前chunk 不足以显示一行
                if self.with > buffer.len() {
                    let len = buffer.len();
                    if i == 0 {
                        // 从第一个块读取完毕
                        if let Some(b1) = self.hex_chunk[1] {
                            let mut v: Vec<u8> = Vec::with_capacity(self.with);
                            v.extend_from_slice(buffer.left());
                            v.extend_from_slice(buffer.right());
                            let remaining = self.with - buffer.len();
                            //从下一个块读取
                            let buf1 = b1.text_from_file_seek(line_start + len..);
                            //继续读取
                            if remaining >= buf1.len() {
                                // let buf2 = buf1.text(..need);
                                v.extend_from_slice(buf1.left());
                                v.extend_from_slice(buf1.right());
                                self.line_file_start += len + buf1.len();
                                return Some(LineStr {
                                    data: LineData::Own(v),
                                    block_num: b.block_id,
                                    block_offset: line_start - b.file_start,
                                    line_file_start: line_start,
                                    line_file_end: line_start + len + buf1.len(),
                                });
                            } else {
                                self.line_file_start += self.with;
                                let buf2 = buf1.text(..remaining);
                                v.extend_from_slice(buf2.left());
                                v.extend_from_slice(buf2.right());
                                return Some(LineStr {
                                    data: LineData::Own(v),
                                    block_num: b.block_id,
                                    block_offset: line_start - b.file_start,
                                    line_file_start: line_start,
                                    line_file_end: line_start + self.with,
                                });
                            }
                        } else {
                            self.line_file_start += len;
                            return Some(LineStr {
                                data: LineData::GapBytes(buffer),
                                block_num: b.block_id,
                                block_offset: line_start - b.file_start,
                                line_file_start: line_start,
                                line_file_end: line_start + len,
                            });
                        }
                    } else {
                        self.line_file_start += len;
                        return Some(LineStr {
                            data: LineData::GapBytes(buffer),
                            block_num: b.block_id,
                            block_offset: line_start - b.file_start,
                            line_file_start: line_start,
                            line_file_end: line_start + len,
                        });
                    }
                } else {
                    self.line_file_start += self.with;
                    return Some(LineStr {
                        // line: buffer.text(..self.with),
                        data: LineData::GapBytes(buffer.text(..self.with)),
                        block_num: b.block_id,
                        block_offset: line_start - b.file_start,
                        line_file_start: line_start,
                        line_file_end: line_start + self.with,
                    });
                }
            }
        }
        return None;
    }
}
