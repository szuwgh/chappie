use crate::common::ring_vec::RingVec;
use crate::textwarp::BlockLineData;
use crate::textwarp::ChapResult;
use crate::textwarp::EditLineMeta;
use crate::textwarp::EditText;
use crate::textwarp::GapBuffer;
use crate::textwarp::GapBytes;
use crate::textwarp::Line;
use crate::textwarp::LineBlockStr;
use crate::textwarp::LineData;
use crate::textwarp::LineStr;
use crate::textwarp::PageOffset;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use crate::ChapError;
use crc::Crc;
use crc::CRC_32_ISO_HDLC;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Seek;
use std::io::SeekFrom;
use std::iter;
use std::path::Path;

const CHAR_GAP_SIZE: usize = 64;
const BLOCK_SIZE: usize = 4096;
const BLOKK_NUM: usize = 8;
const MAX_LINE_SIZE: usize = 4096; //最大行长度4KB

type BlockId = usize;

#[derive(Clone)]
//一行数据
struct LineIndex {
    block_start: usize, //行在块开始位置
    block_end: usize,   //行在块结束位置
    is_complete: bool,  //是否完整行
}

impl LineIndex {
    fn line_size(&self) -> usize {
        self.block_end - self.block_start
    }
}

//按块加载文件 每个块4KB大小
struct Block {
    data: GapBuffer,  //每一个块使用 GapBuffer 存储
    block_num: usize, //块编号
    //start_line_index: usize,     //块内的起始行号在整个文件中
    //line_count: usize,           //块内的行数
    //file_start: usize,           //块在文件开始位置
    //file_end: usize,             //块在文件结束的位置
    is_modified: bool, //块是否被修改
}

impl Block {
    fn text(&self, index: &LineIndex) -> GapBytes {
        self.data.text(index.block_start..index.block_end)
    }
    fn from_reader<T: io::Read + Seek>(
        reader: &mut T,
        buf: &mut [u8],
        file_start: usize,
        block_num: usize,
        check_sum: Option<u32>,
        //   mut last_line_size: usize, //如果上一行不是完整行要传line_offset 上一行的大小
    ) -> ChapResult<(Self, Option<BlockIndex>)> {
        // buf.clear();
        reader.seek(SeekFrom::Start(file_start as u64))?;
        let n = reader.read(buf)?;
        if n == 0 {
            return Err(ChapError::EOF);
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
        let sum = crc_checksum(valid_data);
        if let Some(sum1) = check_sum {
            if sum != sum1 {
                panic!("Checksum Mismatch Exception")
            }
        }
        let block_index = if check_sum.is_none() {
            // 把这一块拆成多个 128 字节 sub-chunk，并为每个 sub-chunk 创建一个 GapBuffer
            let mut lines_index = Vec::new();
            let mut line_buf = Vec::new();
            let mut block_reader = BufReader::new(&valid_data[..]);
            let mut block_offset: usize = 0;
            let mut line_count: usize = 0;
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
                    // line_offset: last_line_size,
                };

                if is_complete {
                    line_count += 1;
                    //last_line_size = 0;
                } else {
                    //last_line_size += bytes_read;
                }
                block_offset = end;
                lines_index.push(index);
            }
            Some(BlockIndex {
                file_start: file_start, //块在文件开始位置
                block_num: block_num,
                start_line_index: 0,      //块内的起始行号在整个文件中
                line_count: line_count,   //块内的行数
                block_size: actual_len,   //块的大小
                lines_index: lines_index, //块内的行索引
                check_sum: sum,
            })
        } else {
            None
        };

        let block = Block {
            data: GapBuffer::from_bytes(valid_data, CHAR_GAP_SIZE),
            block_num: block_num,
            is_modified: false,
        };

        Ok((block, block_index))
    }
}

#[derive(Clone)]
struct BlockIndex {
    file_start: usize,           //块在文件开始位置
    block_num: usize,            //块编号
    start_line_index: usize,     //块内的起始行号在整个文件中
    line_count: usize,           //块内的行数
    block_size: usize,           //块的大小
    lines_index: Vec<LineIndex>, //块内的行索引
    check_sum: u32,              //保存的时候会更新 sum
}

impl BlockIndex {
    fn file_end(&self) -> usize {
        self.file_start + self.block_size
    }
}

pub(crate) struct GapBlockText {
    reader: BufReader<File>,
    blocks: RingVec<Block>,        // 每个块4KB大小
    cache: HashMap<usize, Block>,  // 缓存已修改的块
    file_size: usize,              // 文件大小
    block_indexs: Vec<BlockIndex>, // 每一块的开始的行号 二分法快速检索
}

impl TextIndex for GapBlockText {
    fn get_page_offset(&self, line_num: usize) -> PageOffset {
        PageOffset::new()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: PageOffset) {
        // GapText 不支持分页偏移
    }
}

impl GapBlockText {
    pub(crate) fn from_file_path<P: AsRef<Path>>(filename: P) -> ChapResult<GapBlockText> {
        let file_size = std::fs::metadata(&filename)?.len(); // 直接通过路径获取
        let file = File::open(filename)?;
        let mut reader = BufReader::new(file);
        let (blocks, block_indexs) = Self::read_blocks(&mut reader, 0, 0)?;
        Ok(GapBlockText {
            reader: reader,
            blocks: blocks,
            cache: HashMap::new(),
            file_size: file_size as usize,
            block_indexs: block_indexs,
        })
    }

    fn read_blocks<T: io::Read + Seek>(
        reader: &mut T,
        file_start: usize,
        mut block_num: usize,
    ) -> ChapResult<(RingVec<Block>, Vec<BlockIndex>)> {
        let mut blocks = RingVec::with_capacity(BLOKK_NUM);
        let mut buf = [0u8; BLOCK_SIZE];
        let mut block_start_offset: usize = 0;
        let mut start_line_index: usize = 0;
        let mut block_indexs = Vec::with_capacity(BLOKK_NUM * 2);
        // let mut line_offset: usize = 0;
        // let mut line_buf = Vec::new();
        for i in 0..BLOKK_NUM {
            block_num += i;
            //let block_start_offset = offset;
            // let n = reader.read(&mut buf)?;
            // if n == 0 {
            //     break;
            // }

            // // 调整到有效的UTF-8字符边界
            // let actual_len = match str::from_utf8(&buf[..n]) {
            //     Ok(_) => n,                // 整个块有效，直接使用
            //     Err(e) => e.valid_up_to(), // 使用最后一个有效字符的边界
            // };

            // // 如果有剩余字节（字符被分割），回退文件读取位置
            // if actual_len < n {
            //     let seek_back = n - actual_len;
            //     reader.seek(SeekFrom::Current(-(seek_back as i64)))?;
            // }

            // // 使用有效部分的数据
            // let valid_data = &buf[..actual_len];

            // // 把这一块拆成多个 128 字节 sub-chunk，并为每个 sub-chunk 创建一个 GapBuffer
            // let mut lines_index = Vec::new();
            // let mut line_buf = Vec::new();
            // let mut block_reader = BufReader::new(&valid_data[..]);
            // let mut block_offset: usize = 0;
            // let mut line_count: usize = 0;
            // //读取块内的行 构建行索引
            // loop {
            //     line_buf.clear();
            //     let bytes_read = block_reader.read_until(b'\n', &mut line_buf)?;
            //     if bytes_read == 0 {
            //         break;
            //     }
            //     let is_complete = line_buf.ends_with(&[b'\n']);
            //     if is_complete {
            //         line_count += 1;
            //         line_offset = 0;
            //     }

            //     let end = block_offset + bytes_read;
            //     let index = LineIndex {
            //         is_complete: is_complete,
            //         block_start: block_offset,
            //         line_offset: line_offset,
            //         block_end: end,
            //     };
            //     line_offset += bytes_read;
            //     block_offset = end;
            //     offset += bytes_read;
            //     lines_index.push(index);
            // }
            // let block = Block {
            //     data: GapBuffer::from_bytes(&valid_data, CHAR_GAP_SIZE),
            //     block_num: i,
            //     lines_index: lines_index,
            //     is_modified: false,
            // };
            if let Ok((block, Some(mut block_index))) =
                Block::from_reader(reader, &mut buf, block_start_offset, i, None)
            {
                blocks.push(block);
                block_index.start_line_index = start_line_index;
                let size = block_index.block_size;
                let line_count = block_index.line_count;
                let last_line_index = block_index.lines_index.last().unwrap();

                if last_line_index.is_complete {
                    //是完整行
                    start_line_index += line_count;
                } else {
                    //不是完整行
                    start_line_index += line_count - 1;
                }
                //  last_line_size = last_line_index.line_size();
                block_start_offset += size;
                block_indexs.push(block_index);
            } else {
                break;
            }
        }
        Ok((blocks, block_indexs))
    }

    fn read_one_block(
        &mut self,
        file_start: usize,
        block_num: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<(Block, Option<BlockIndex>)> {
        let mut buf = [0u8; BLOCK_SIZE];
        Block::from_reader(&mut self.reader, &mut buf, file_start, block_num, check_sum)
    }

    //获取上一个block 并弹入列表中
    fn read_last_block(
        &mut self,
        file_start: usize,
        block_num: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<()> {
        let (block, block_index) = self.read_one_block(file_start, block_num, check_sum)?;
        self.blocks.push_front(block);
        Ok(())
    }

    fn read_next_block(
        &mut self,
        file_start: usize,
        block_num: usize,
        check_sum: Option<u32>,
        no_index: bool,
    ) -> ChapResult<()> {
        let (block, block_index) = self.read_one_block(file_start, block_num, check_sum)?;
        self.blocks.push(block);
        if let Some(index) = block_index {
            if no_index {
                self.block_indexs.push(index);
            }
        }
        Ok(())
    }

    fn get_block_from_line<'a>(&'a self, line_index: usize) -> Option<&'a Block> {
        for block in self.blocks.iter() {
            let block_index = self.block_indexs.get(block.block_num).unwrap();
            if line_index >= block_index.start_line_index
                && line_index < block_index.start_line_index + block_index.line_count
            {
                return Some(block);
            }
        }
        None
    }

    // fn get_block_line<'a>(
    //     &'a self,
    //     line_index: usize,
    //     line_offset: usize,
    // ) -> Option<LineBlockStr<'a>> {
    //     let block = self.get_block_from_line(line_index)?;
    //     let block_index = self.block_indexs.get(block.block_num).unwrap();
    //     let line_in_block_index = line_index - block_index.start_line_index;
    //     let line_info = &block_index.lines_index[line_in_block_index];

    //     let line_str1 = LineStr {
    //         line_data: LineData::GapBytes(
    //             block.data.text(line_info.block_start..line_info.block_end),
    //         ),
    //         line_file_start: block_index.file_start + line_info.block_start,
    //         line_file_end: block_index.file_start + line_info.block_end,
    //     };
    //     if line_info.is_complete {
    //         //是完整的行
    //         return Some(LineBlockStr([line_str1, LineStr::empty()]));
    //     } else {
    //         //不完整行 需要合并一行的下部分 一行的下一个部分在 下一个 block中
    //         //获取下一个block
    //         let next_block_num = block.block_num + 1;
    //         let next_block_index = self.block_indexs.get(next_block_num).unwrap();
    //         if let Some(next_block) = self.blocks.get(next_block_num) {
    //             if !next_block_index.lines_index.is_empty() {
    //                 let next_line_info = &next_block_index.lines_index[0];
    //                 let line_str2 = LineStr {
    //                     line_data: LineData::GapBytes(
    //                         next_block
    //                             .data
    //                             .text(next_line_info.block_start..next_line_info.block_end),
    //                     ),
    //                     line_file_start: next_block_index.file_start + next_line_info.block_start,
    //                     line_file_end: next_block_index.file_start + next_line_info.block_end,
    //                 };
    //                 return Some(LineBlockStr([line_str1, line_str2]));
    //             }
    //         }
    //     }
    //     None
    // }

    fn get_iter(
        &mut self,
        line_index: usize,
        // line_offset: usize,
        block_num: usize,
        block_offst: usize,
        // line_file_start: usize,
    ) -> ChapResult<GapBlockTextIter<'_>> {
        let mut j = None;
        //查找当前line_index是否在block列表中
        //log::debug!("edit_block get file start:{}", line_file_start);
        loop {
            for (i, block) in self.blocks.iter().enumerate() {
                let block_index = self.block_indexs.get(block.block_num).unwrap();
                if block_num == block_index.block_num {
                    j = Some(i);
                    break;
                }
            }
            if let Some(i) = j {
                let block = self.blocks.get(i).unwrap();
                if i == 0 {
                    ////读取上一个块 把最后一个块弹出
                    if block.block_num == 0 {
                        return Ok(GapBlockTextIter {
                            blocks: [self.blocks.get(0), self.blocks.get(1)],
                            block_indexs: &self.block_indexs,
                            cur_block_num: block_num,
                            cur_block_offset: block_offst,
                            cur_line_index: line_index,
                        });
                    } else {
                        //读取上一个块 把最后一个块弹出
                        let last_block_index = self.block_indexs.get(block.block_num - 1).unwrap();
                        self.read_last_block(
                            last_block_index.file_start,
                            block.block_num - 1,
                            Some(last_block_index.check_sum),
                        )?;
                        return Ok(GapBlockTextIter {
                            blocks: [self.blocks.get(1), self.blocks.get(2)],
                            block_indexs: &self.block_indexs,
                            cur_block_num: block_num,
                            cur_block_offset: block_offst,
                            cur_line_index: line_index,
                        });
                    }
                } else if i == self.blocks.len() - 1 {
                    //最后一个块 弹出第一个快 取下一个块
                    //之前已经取过了
                    if let Some(next_block_index) = self.block_indexs.get(block.block_num + 1) {
                        self.read_next_block(
                            next_block_index.file_start,
                            block.block_num + 1,
                            Some(next_block_index.check_sum),
                            false,
                        )?;
                        return Ok(GapBlockTextIter {
                            blocks: [self.blocks.get(i - 1), self.blocks.get(i)],
                            block_indexs: &self.block_indexs,
                            cur_block_num: block_num,
                            cur_block_offset: block_offst,
                            cur_line_index: line_index,
                        });
                    } else {
                        //从磁盘上取
                        let block_index = self.block_indexs.get(block.block_num).unwrap();
                        let next_block_file_start = block_index.file_end();
                        if next_block_file_start >= self.file_size {
                            return Ok(GapBlockTextIter {
                                blocks: [self.blocks.get(i), None],
                                block_indexs: &self.block_indexs,
                                cur_block_num: block_num,
                                cur_block_offset: block_offst,
                                cur_line_index: line_index,
                            });
                        }
                        self.read_next_block(
                            next_block_file_start,
                            block.block_num + 1,
                            None,
                            true,
                        )?;
                        return Ok(GapBlockTextIter {
                            blocks: [self.blocks.get(i - 1), self.blocks.get(i)],
                            block_indexs: &self.block_indexs,
                            cur_block_num: block_num,
                            cur_block_offset: block_offst,
                            cur_line_index: line_index,
                        });
                    }
                } else {
                    //不是最后一个块
                    return Ok(GapBlockTextIter {
                        blocks: [self.blocks.get(i), self.blocks.get(i + 1)],
                        block_indexs: &self.block_indexs,
                        cur_block_num: block_num,
                        cur_block_offset: block_offst,
                        cur_line_index: line_index,
                    });
                }
            }
            //如果找不到行数 则要重置 block 列表
            self.reset_blocks(line_index)?;
        }
    }

    fn find_block_index(&self, line_file_start: usize) -> Option<&BlockIndex> {
        // 未找到匹配块
        for block_index in self.block_indexs.iter() {
            if line_file_start >= block_index.file_start && line_file_start < block_index.file_end()
            {
                return Some(block_index);
            }
        }
        None
    }

    fn reset_blocks(&mut self, line_file_start: usize) -> ChapResult<()> {
        let block_index = self.find_block_index(line_file_start);
        //之前就获取过这个block
        if let Some(index) = block_index {
            let file_start = index.file_start;
            let block_num = index.block_num;
            let (blocks, block_indexs) =
                Self::read_blocks(&mut self.reader, file_start, block_num)?;
            let last_block_index = self.block_indexs.last().unwrap();
            let diff = last_block_index
                .block_num
                .saturating_sub(block_indexs.last().unwrap().block_num);
            if diff > 0 {
                self.block_indexs
                    .extend_from_slice(&block_indexs[(block_indexs.len() - diff)..]);
            }
            self.blocks = blocks;
        } else {
            'out: loop {
                //不在索引表中
                let last_block_index = self.block_indexs.last().unwrap();
                let (blocks, block_indexs) = Self::read_blocks(
                    &mut self.reader,
                    last_block_index.file_end(),
                    last_block_index.block_num + 1,
                    //  last_line_size,
                )?;
                self.blocks = blocks;
                self.block_indexs.extend(block_indexs);
                for (_, block) in self.blocks.iter().enumerate() {
                    let block_index = self.block_indexs.get(block.block_num).unwrap();
                    if line_file_start >= block_index.file_start
                        && line_file_start < block_index.file_end()
                    {
                        break 'out;
                    }
                }
            }
        }
        Ok(())
        //查找 block_index
        //  let mut blocks = RingVec::with_capacity(BLOKK_NUM);
        //  loop {}
    }
    // /*滚动block*/
    // fn scroll_block(&mut self, line_index: usize) {
    //    let is_in_block =if let Some(last_block) = self.blocks.last() {
    //         if line_index >= last_block.start_line_index
    //             && line_index < last_block.start_line_index + last_block.line_count
    //         {
    //             //return Some(block);
    //         }
    //     }
    // }
}

impl Text for GapBlockText {
    type LineItem<'a> = LineBlockStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn get_line<'a>(
        &'a mut self,
        line_index: usize,
        line_file_start: usize,
        line_file_end: usize,
    ) -> Self::LineItem<'a> {
        todo!()
        // LineStr {
        //     line_data: LineData::GapBytes(self.borrow_lines_mut()[line_index].text(..)),
        //     line_file_start: line_file_start,
        //     line_file_end: line_file_end,
        // }
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        return vec![];
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        return 0;
    }

    fn has_next_line(&self, meta: &EditLineMeta) -> bool {
        if meta.line_file_end >= self.file_size {
            return false;
        }
        return true;
    }

    fn iter<'a>(
        &'a mut self,
        line_index: usize,   //行索引
        block_num: usize,    //块编号
        block_offset: usize, //块内偏移
        line_offset: usize,  //行内偏移
        line_file_start: usize,
    ) -> impl Iterator<Item = Self::LineItem<'a>> {
        self.get_iter(line_index, block_num, block_offset).unwrap()
    }

    fn iter_rev<'a>(
        &'a mut self,
        line_index: usize,
        block_num: usize,    //块编号
        block_offset: usize, //块内偏移
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
    blocks: [Option<&'a Block>; 2],
    block_indexs: &'a [BlockIndex],
    cur_block_num: usize,
    cur_block_offset: usize,
    cur_line_index: usize,
}

impl<'a> Iterator for GapBlockTextIter<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        for (i, block) in self.blocks.iter().enumerate() {
            if let Some(b) = block {
                let block_index = self.block_indexs.get(b.block_num).unwrap();
                if self.cur_block_num > block_index.block_num {
                    continue;
                }
                let line_in_block_index = self.cur_line_index - block_index.start_line_index;
                if line_in_block_index >= block_index.lines_index.len() {
                    return None;
                }
                let line_info = &block_index.lines_index[line_in_block_index];
                let line_str1 = BlockLineData {
                    data: LineData::GapBytes(
                        b.data.text(line_info.block_start..line_info.block_end),
                    ),
                    block_num: block_index.block_num,
                    block_offset: line_info.block_start,
                };
                if line_info.is_complete {
                    //是完整的行
                    let ret: LineBlockStr<'_> =
                        LineBlockStr([line_str1, BlockLineData::empty_gap_bytes()]);
                    self.cur_block_offset += ret.text_len();
                    //查看是否大于当前块
                    if self.cur_block_offset >= block_index.block_size {
                        self.cur_block_num += 1;
                        self.cur_block_offset = 0;
                    }
                    self.cur_line_index += 1;
                    return Some(ret);
                } else {
                    //不完整行 需要合并一行的下部分 一行的下一个部分在 下一个 block中
                    //获取下一个block
                    if i == 0 {
                        let next_block_num = b.block_num + 1;
                        let next_block_index = self.block_indexs.get(next_block_num).unwrap();
                        if let Some(next_block) = self.blocks[1] {
                            if !next_block_index.lines_index.is_empty() {
                                let next_line_info = &next_block_index.lines_index[0];
                                let line_str2 = BlockLineData {
                                    data: LineData::GapBytes(next_block.data.text(
                                        next_line_info.block_start..next_line_info.block_end,
                                    )),
                                    block_num: next_block_index.block_num,
                                    block_offset: next_line_info.block_start,
                                };
                                let ret: LineBlockStr<'_> = LineBlockStr([line_str1, line_str2]);
                                self.cur_block_offset += ret.text_len();
                                //查看是否大于当前块
                                if self.cur_block_offset >= block_index.block_size {
                                    self.cur_block_num += 1;
                                    self.cur_block_offset = 0;
                                }
                                if next_line_info.is_complete {
                                    self.cur_line_index += 1;
                                }
                                return Some(ret);
                            }
                        }
                    }
                    let ret: LineBlockStr<'_> =
                        LineBlockStr([line_str1, BlockLineData::empty_gap_bytes()]);
                    self.cur_block_offset += ret.text_len();
                    return Some(ret);
                }
            }
        }
        None
        // let line_block = self
        //     .text
        //     .get_block_line(self.cur_line_index, self.cur_line_offset);
        // self.cur_line_index += 1;
        // line_block
    }
}

fn crc_checksum(data: &[u8]) -> u32 {
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    crc.checksum(data)
}

mod tests {

    use super::*;
    #[test]
    fn test_gap_block_text() {
        let gap_block_text = GapBlockText::from_file_path("/home/postgres/a.txt").unwrap();
        for block in gap_block_text.blocks.iter() {
            let block_index = gap_block_text.block_indexs.get(block.block_num).unwrap();
            //打印块所有字段信息
            println!("Block number: {},start_line_index:{},line_count:{},file_start:{},file_end:{},is_modified:{}",
             block.block_num,block_index.start_line_index,block_index.line_count,block_index.file_start,block_index.block_size,block.is_modified);
            for (i, line_idx) in gap_block_text.block_indexs[block.block_num]
                .lines_index
                .iter()
                .enumerate()
            {
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
    #[test]
    fn test_gap_block_iter() {
        let mut gap_block_text = GapBlockText::from_file_path("/home/postgres/a.txt").unwrap();
        let mut iter = gap_block_text.get_iter(0, 0, 0).unwrap();
        while let Some(line_block_str) = iter.next() {
            println!("Line: {}", line_block_str);
        }
    }
}
