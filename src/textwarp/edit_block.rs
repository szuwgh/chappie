use crate::common::ring_vec::RingVec;
use crate::textwarp::ChapResult;
use crate::textwarp::EditLineMeta;
use crate::textwarp::EditText;
use crate::textwarp::GapBuffer;
use crate::textwarp::GapBytes;
use crate::textwarp::LineBlockStr;
use crate::textwarp::LineData;
use crate::textwarp::LineStr;
use crate::textwarp::Text;
use crate::textwarp::TextSelect;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::iter;
use std::path::Path;

const CHAR_GAP_SIZE: usize = 64;
const BLOCK_SIZE: usize = 512;
const BLOKK_NUM: usize = 8;
const MAX_LINE_SIZE: usize = 4096; //最大行长度4KB

//一行数据
struct LineIndex {
    block_start: usize, //行在块开始位置
    block_end: usize,   //行在块结束位置
    is_complete: bool,  //是否完整行
}

//按块加载文件 每个块4KB大小
struct Block {
    data: GapBuffer,             //每一个块使用 GapBuffer 存储
    block_num: usize,            //块编号
    lines_index: Vec<LineIndex>, //块内的行索引
    start_line_index: usize,     //块内的起始行号在整个文件中
    line_count: usize,           //块内的行数
    file_start: usize,           //块在文件开始位置
    file_end: usize,             //块在文件结束的位置
    is_modified: bool,           //块是否被修改
}

impl Block {
    fn text(&self, index: &LineIndex) -> GapBytes {
        self.data.text(index.block_start..index.block_end)
    }
}

pub(crate) struct GapBlockText {
    blocks: RingVec<Block>,       // 每个块4KB大小
    cache: HashMap<usize, Block>, // 缓存已修改的块
    file_size: usize,             // 文件大小
}

impl GapBlockText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<GapBlockText> {
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);
        let mut blocks = RingVec::with_capacity(BLOKK_NUM);
        let mut buf = [0u8; BLOCK_SIZE];
        let mut offset: usize = 0;
        let mut start_line_index: usize = 0;
        let mut line_count: usize = 0;
        for i in 0..BLOKK_NUM {
            let block_start_offset = offset;
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }

            // 调整到有效的UTF-8字符边界
            let actual_len = match str::from_utf8(&buf[..n]) {
                Ok(_) => n,                // 整个块有效，直接使用
                Err(e) => e.valid_up_to(), // 使用最后一个有效字符的边界
            };

            // 如果有剩余字节（字符被分割），回退文件读取位置
            if actual_len < n {
                let seek_back = n - actual_len;
                reader.seek(SeekFrom::Current(-(seek_back as i64)))?;
            }

            // 使用有效部分的数据
            let valid_data = &buf[..actual_len];

            // 把这一块拆成多个 128 字节 sub-chunk，并为每个 sub-chunk 创建一个 GapBuffer
            let mut lines_index = Vec::new();
            let mut line_buf = Vec::new();
            let mut block_reader = BufReader::new(&valid_data[..]);
            let mut block_offset: usize = 0;
            //读取块内的行 构建行索引
            loop {
                line_buf.clear();
                let bytes_read = block_reader.read_until(b'\n', &mut line_buf)?;
                if bytes_read == 0 {
                    break;
                }
                let is_complete = line_buf.ends_with(&[b'\n']);
                let end = block_offset + bytes_read;
                let index = LineIndex {
                    is_complete: is_complete,
                    block_start: block_offset,
                    block_end: end,
                };
                block_offset = end;
                offset += bytes_read;
                lines_index.push(index);
                line_count += 1;
            }
            let block = Block {
                data: GapBuffer::from_bytes(&valid_data, CHAR_GAP_SIZE),
                block_num: i,
                lines_index: lines_index,
                start_line_index: start_line_index,
                line_count: line_count,
                is_modified: false,
                file_start: block_start_offset,
                file_end: offset,
            };
            blocks.push(block);

            // 如果读取不到一整块，但最后一块也要加上
            if n < BLOCK_SIZE {
                break;
            }
        }
        Ok(GapBlockText {
            blocks: blocks, // 每个块4KB大小
            cache: HashMap::new(),
            file_size: 0, // 文件大小
        })
    }

    fn read_next_block(&mut self, file_seek: usize) -> ChapResult<()> {
        Ok(())
    }

    fn get_block_from_line<'a>(&'a self, line_index: usize) -> Option<&'a Block> {
        for block in self.blocks.iter() {
            if line_index >= block.start_line_index
                && line_index < block.start_line_index + block.line_count
            {
                return Some(block);
            }
        }
        None
    }

    fn get_block_line<'a>(
        &'a self,
        line_index: usize,
        line_offset: usize,
    ) -> Option<LineBlockStr<'a>> {
        let block = self.get_block_from_line(line_index)?;
        let line_in_block_index = line_index - block.start_line_index;
        let line_info = &block.lines_index[line_in_block_index];
        //是完整的行
        let line_str1 = LineStr {
            line_data: LineData::GapBytes(
                block.data.text(line_info.block_start..line_info.block_end),
            ),
            line_file_start: block.file_start + line_info.block_start,
            line_file_end: block.file_start + line_info.block_end,
        };
        if line_info.is_complete {
            return Some(LineBlockStr([line_str1, LineStr::empty()]));
        } else {
            //不完整行 需要合并下一行
            //获取下一个block
            let next_block_num = block.block_num + 1;
            if let Some(next_block) = self.blocks.get(next_block_num) {
                if !next_block.lines_index.is_empty() {
                    let next_line_info = &next_block.lines_index[0];
                    let line_str2 = LineStr {
                        line_data: LineData::GapBytes(
                            next_block
                                .data
                                .text(next_line_info.block_start..next_line_info.block_end),
                        ),
                        line_file_start: next_block.file_start + next_line_info.block_start,
                        line_file_end: next_block.file_start + next_line_info.block_end,
                    };
                    return Some(LineBlockStr([line_str1, line_str2]));
                }
            }
        }
        None
    }

    fn get_iter(&mut self, line_index: usize, line_offset: usize) -> GapBlockTextIter {
        GapBlockTextIter {
            text: self,
            cur_line_index: line_index,
            cur_line_offset: line_offset,
            cur_file_start: 0,
        }
    }
}

impl Text for GapBlockText {
    type LineItem<'a> = LineBlockStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_start: usize,
        line_end: usize,
    ) -> Self::LineItem<'a> {
        todo!()
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        return vec![];
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        return 0;
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        return false;
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,  //行索引
        line_offset: usize, //行内偏移
        line_file_start: usize,
    ) -> impl Iterator<Item = Self::LineItem<'a>> {
        iter::empty()
    }

    fn iter_rev<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = Self::LineItem<'a>> {
        iter::empty()
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        iter::empty()
    }
}

impl EditText for GapBlockText {
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &EditLineMeta,
    ) {
    }

    fn insert(&mut self, cursor_y: usize, bytes_cursor: usize, line_meta: &EditLineMeta, c: char) {}

    fn insert_newline(&mut self, cursor_y: usize, cursor_x: usize, line_meta: &EditLineMeta) {}

    fn save<P: AsRef<Path>>(&mut self, filepath: P) -> ChapResult<()> {
        todo!()
    }
}

struct GapBlockTextIter<'a> {
    text: &'a GapBlockText,
    cur_line_index: usize,
    cur_line_offset: usize,
    cur_file_start: usize,
}

impl<'a> Iterator for GapBlockTextIter<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.text
            .get_block_line(self.cur_line_index, self.cur_line_offset)
    }
}

mod tests {

    use super::*;
    #[test]
    fn test_gap_block_text() {
        let gap_block_text = GapBlockText::from_file_path("/home/postgres/a.txt").unwrap();
        for block in gap_block_text.blocks.iter() {
            //打印块所有字段信息
            println!("Block number: {},start_line_index:{},line_count:{},file_start:{},file_end:{},is_modified:{}",
             block.block_num,block.start_line_index,block.line_count,block.file_start,block.file_end,block.is_modified);
            for (i, line_idx) in block.lines_index.iter().enumerate() {
                println!(
                    "  line {}: block_start: {}, block_end: {}, is_complete: {},content:{}",
                    i,
                    line_idx.block_start,
                    line_idx.block_end,
                    line_idx.is_complete,
                    block.text(line_idx)
                );
            }
        }
        //assert!(gap_block_text.is_ok());
    }
}
