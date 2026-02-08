use crate::common::ring_vec::RingVec;
use crate::textwarp::block::Block;
use crate::textwarp::block::BlockIndex;
use crate::textwarp::block::LineIndex;
use crate::textwarp::block::BLOCK_SIZE;
use crate::textwarp::block::BLOKK_NUM;
use crate::textwarp::BlockLineData;
use crate::textwarp::ChapResult;
use crate::textwarp::EditText;
use crate::textwarp::Line;
use crate::textwarp::LineBlockStr;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::iter;
use std::mem;
use std::path::Path;

pub(crate) struct GapBlockText {
    reader: BufReader<File>,
    blocks: RingVec<Block>,        // 每个块4KB大小
    cache: HashMap<usize, Block>,  // 缓存已修改的块
    file_size: usize,              // 文件大小
    block_indexs: Vec<BlockIndex>, // 每一块的开始的行号 二分法快速检索
}

impl TextIndex for GapBlockText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        LineState::default()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
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
        for i in 0..BLOKK_NUM {
            block_num += i;
            if let Ok((block, Some(block_index))) =
                Block::from_reader(reader, &mut buf, block_start_offset, i, None)
            {
                blocks.push(block);
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
        if self.cache.contains_key(&file_start) {
            let o = self.cache.remove(&file_start).unwrap();
            return Ok((o, None));
        }
        let mut buf = [0u8; BLOCK_SIZE];
        Block::from_reader(&mut self.reader, &mut buf, file_start, block_num, check_sum)
    }

    //获取上一个block 并弹入列表中
    fn read_prev_block(
        &mut self,
        file_start: usize,
        block_num: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<()> {
        let (block, _) = self.read_one_block(file_start, block_num, check_sum)?;
        let old = self.blocks.push_front(block);
        if let Some(o) = old {
            if o.is_modified {
                self.cache.insert(file_start, o);
            }
        }
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
        let old = self.blocks.push(block);
        if let Some(index) = block_index {
            if no_index {
                self.block_indexs.push(index);
            }
        }
        if let Some(o) = old {
            if o.is_modified {
                self.cache.insert(file_start, o);
            }
        }
        Ok(())
    }

    fn get_iter_rev(
        &mut self,
        block_num: usize,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<GapBlockTextIterRev<'_>> {
        let block3: Block3<'_> = self.get_block3(block_num, block_line_index, block_offset)?;
        Ok(GapBlockTextIterRev {
            blocks: block3,
            cur_block_num: block_num,
            cur_block_line_index: block_line_index,
            cur_block_offset: block_offset,
        })
    }

    fn get_block3(
        &mut self,
        block_num: usize,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<Block3<'_>> {
        let mut j = None;
        //查找当前line_index是否在block列表中
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
                    if block.block_num == 0 {
                        return Ok(Block3 {
                            blocks: [self.blocks.get(0), self.blocks.get(1), self.blocks.get(2)],
                            block_indexs: &self.block_indexs,
                        });
                    } else {
                        //读取上一个块 把最后一个块弹出
                        let last_block_index = self.block_indexs.get(block.block_num - 1).unwrap();
                        self.read_prev_block(
                            last_block_index.file_start,
                            block.block_num - 1,
                            Some(last_block_index.check_sum),
                        )?;
                        return Ok(Block3 {
                            blocks: [self.blocks.get(1), self.blocks.get(2), self.blocks.get(3)],
                            block_indexs: &self.block_indexs,
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
                        return Ok(Block3 {
                            blocks: [
                                self.blocks.get(i - 1),
                                self.blocks.get(i),
                                self.blocks.get(i + 1),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    } else {
                        //从磁盘上取
                        let block_index = self.block_indexs.get(block.block_num).unwrap();
                        let next_block_file_start = block_index.file_end();
                        if next_block_file_start >= self.file_size {
                            return Ok(Block3 {
                                blocks: [self.blocks.get(i), None, None],
                                block_indexs: &self.block_indexs,
                            });
                        }
                        self.read_next_block(
                            next_block_file_start,
                            block.block_num + 1,
                            None,
                            true,
                        )?;
                        return Ok(Block3 {
                            blocks: [
                                self.blocks.get(i - 1),
                                self.blocks.get(i),
                                self.blocks.get(i + 1),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    }
                } else {
                    //不是最后一个块
                    return Ok(Block3 {
                        blocks: [
                            self.blocks.get(i),
                            self.blocks.get(i + 1),
                            self.blocks.get(i + 2),
                        ],
                        block_indexs: &self.block_indexs,
                    });
                }
            }
            //如果找不到行数 则要重置 block 列表
            self.reset_blocks(block_num)?;
        }
    }

    fn get_iter(
        &mut self,
        block_num: usize,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<GapBlockTextIter<'_>> {
        let block3 = self.get_block3(block_num, block_line_index, block_offset)?;
        Ok(GapBlockTextIter {
            blocks: block3,
            cur_block_num: block_num,
            cur_block_line_index: block_line_index,
            cur_block_offset: block_offset,
        })
    }

    fn find_block_index(&self, block_num: usize) -> Option<&BlockIndex> {
        // 未找到匹配块
        for block_index in self.block_indexs.iter() {
            if block_num == block_index.block_num {
                return Some(block_index);
            }
        }
        None
    }

    fn find_block(&self, file_start: usize) -> Option<&Block> {
        // 未找到匹配块
        for b in self.blocks.iter() {
            if file_start == b.file_start {
                return Some(b);
            }
        }
        None
    }

    fn reset_blocks(&mut self, block_num: usize) -> ChapResult<()> {
        let block_index = self.find_block_index(block_num);
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
                )?;
                self.blocks = blocks;
                self.block_indexs.extend(block_indexs);
                for (_, block) in self.blocks.iter().enumerate() {
                    if block_num == block.block_num {
                        break 'out;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Text for GapBlockText {
    type LineItem<'a> = LineBlockStr<'a>;
    fn get_file_size(&self) -> usize {
        self.file_size
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<Self::LineItem<'a>> {
        let block_num = state.block_num;
        let block_offset = state.block_offset;
        let block_line_index = state.block_line_index;
        let mut j = None;
        for (i, block) in self.blocks.iter().enumerate() {
            let block_index = self.block_indexs.get(block.block_num).unwrap();
            if block_num == block_index.block_num {
                j = Some(i);
                break;
            }
        }
        if let Some(i) = j {
            let blocks = Block3 {
                blocks: [
                    self.blocks.get(i),
                    self.blocks.get(i + 1),
                    self.blocks.get(i + 2),
                ],
                block_indexs: &self.block_indexs,
            };
            let (l, _) = blocks.get_line(block_num, block_offset)?;
            return Some(l);
        }
        None
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        let mut line_index = state.get_line_index();
        let mut line_end = state.get_line_end();
        let mut block_num = state.get_block_num();
        let mut block_line_index = state.get_block_line_index();
        let mut block_offset = state.get_block_offset();

        let block_index = self.block_indexs.get(block_num)?;

        let line = self.get_line(&state).unwrap();
        if state.get_line_end() >= line.text_len() {
            if state.get_block_line_end() >= block_index.block_size {
                block_num += 1;
                block_line_index = 0;
                block_offset = state.get_block_line_end() - block_index.block_size;
                line_end = 0;
            } else {
                line_end = 0;
                line_index += 1;
                block_line_index += 1;
                block_offset = state.get_block_line_end();
            }
        }

        let p = LineState::builder()
            .line_index(line_index)
            .line_offset(line_end)
            .block_num(block_num)
            .block_line_index(block_line_index)
            .block_offset(block_offset)
            .start_line_num(state.get_line_num())
            .build();
        Some(p)
    }

    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState> {
        if state.get_block_num() == 0
            && state.get_block_line_index() == 0
            && state.get_block_offset() == 0
        {
            return None;
        }
        let mut block_num = state.get_block_num();
        let mut block_line_index = state.get_block_line_index();
        let mut block_offset = state.get_block_offset();
        if state.get_line_offset() > 0 {
            //在行中间
            let p = LineState::builder()
                .start_line_num(state.get_line_num())
                .line_index(state.get_line_index())
                .line_offset(state.get_line_offset())
                .block_num(block_num)
                .block_line_index(block_line_index)
                .block_offset(block_offset)
                .build();
            return Some(p);
        }
        if block_line_index > 0 {
            let last_block_line_index = block_line_index.saturating_sub(1);
            let block = self.block_indexs.get(block_num)?;
            let last_line_info = &block.lines_index[last_block_line_index];
            if last_block_line_index == 0 {
                if block_num == 0 {
                    //已经是第一个块了
                    let p = LineState::builder()
                        .start_line_num(state.get_line_num())
                        .line_index(state.get_line_index().saturating_sub(1))
                        .line_offset(0)
                        .block_num(block_num)
                        .block_line_index(last_block_line_index)
                        .block_offset(0)
                        .build();
                    return Some(p);
                }
                // 如果是第一个块的第一行 要判断是否是一行的起始位置
                //取上一个块的最后一行
                let last_block_num = block_num.saturating_sub(1);
                let last_block_index = self.block_indexs.get(last_block_num)?;
                let last_last_line_info = last_block_index.lines_index.last().unwrap();
                if last_last_line_info.is_complete {
                    //上上一行是完整行
                    //如果是完整行
                    let p = LineState::builder()
                        .start_line_num(state.get_line_num())
                        .line_index(state.get_line_index().saturating_sub(1))
                        .line_offset(0)
                        .block_num(block_num)
                        .block_line_index(last_block_line_index)
                        .block_offset(last_line_info.block_start)
                        .build();
                    return Some(p);
                } else if last_line_info.is_complete {
                    //不完整行 从上一个块的最后一行开始
                    let p = LineState::builder()
                        .start_line_num(state.get_line_num())
                        .line_index(state.get_line_index().saturating_sub(1))
                        .line_offset(0)
                        .block_num(last_block_num)
                        .block_line_index(last_last_line_info.index_num)
                        .block_offset(last_last_line_info.block_start)
                        .build();
                    return Some(p);
                } else {
                    todo!()
                }
            } else {
                let p = LineState::builder()
                    .start_line_num(state.get_line_num())
                    .line_index(state.get_line_index().saturating_sub(1))
                    .line_offset(0)
                    .block_num(block_num)
                    .block_line_index(last_block_line_index)
                    .block_offset(last_line_info.block_start)
                    .build();
                return Some(p);
            }
        } else {
            if block_num == 0 {
                return None;
            }
            let last_block_num = block_num.saturating_sub(1);
            let last_block_index = self.block_indexs.get(last_block_num)?;
            let last_line_info = last_block_index.lines_index.last().unwrap();
            let p = LineState::builder()
                .start_line_num(state.get_line_num())
                .line_index(state.get_line_index().saturating_sub(1))
                .line_offset(0)
                .block_num(last_block_num)
                .block_line_index(last_line_info.index_num)
                .block_offset(last_line_info.block_start)
                .build();
            return Some(p);
        }
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        return vec![];
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        return 0;
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        if meta.get_block_num() == 0
            && meta.get_block_line_index() == 0
            && meta.get_block_offset() == 0
        {
            return false;
        }
        return true;
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        let last_block_index = self.block_indexs.last().unwrap();
        if meta.get_block_num() >= last_block_index.block_num
            && meta.get_block_line_end() >= last_block_index.block_size
        {
            return false;
        }
        return true;
    }

    fn iter<'a>(&'a mut self, state: &LineState) -> impl Iterator<Item = Self::LineItem<'a>> {
        self.get_iter(state.block_num, state.block_line_index, state.block_offset)
            .unwrap()
    }

    fn iter_rev<'a>(&'a mut self, state: &LineState) -> impl Iterator<Item = Self::LineItem<'a>> {
        self.get_iter_rev(state.block_num, state.block_line_index, state.block_offset)
            .unwrap()
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
    fn rollback() -> ChapResult<()> {
        Ok(())
    }
    fn backspace(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        count: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        //合并行
        if line_meta.line_offset == 0 && bytes_cursor == 0 {
            //在行首 需要合并上一行
            if block_offset > 0 {
                let block = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.block_num == block_num)
                    .unwrap();
                let insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
                //删除行首的换行符
                block.backspace(insert_offset, 1);
                let cur_block_index = &mut self.block_indexs[block_num];
                cur_block_index.lines_index[block_line_index - 1].block_end =
                    cur_block_index.lines_index[block_line_index].block_end - 1;
                cur_block_index.lines_index.remove(block_line_index);
                let len = cur_block_index.lines_index.len();
                if block_line_index < len {
                    for i in block_line_index..len {
                        let li = &mut cur_block_index.lines_index[i];
                        li.block_start = li.block_start.saturating_sub(1);
                        li.block_end = li.block_end.saturating_sub(1);
                        li.index_num = li.index_num.saturating_sub(1);
                    }
                }
            } else {
                //在块首 需要合并上一个块的最后一行
                if block_num > 0 {
                    let last_block = self
                        .blocks
                        .iter_mut()
                        .find(|b| b.block_num == block_num - 1)
                        .unwrap();
                    //删除行首的换行符
                    last_block.backspace_last(1);
                    let last_block_index = &mut self.block_indexs[block_num - 1];
                    last_block_index.lines_index.last_mut().unwrap().is_complete = false;
                }
            }
        } else {
            let mut cur_block_index = &mut self.block_indexs[block_num];
            let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
            if insert_offset < cur_block_index.block_size {
            } else {
                insert_offset = insert_offset - cur_block_index.block_size;
                block_num += 1;
                block_line_index = 0;
            }
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_num == block_num)
                .unwrap();
            block.backspace(insert_offset, count);
            cur_block_index = &mut self.block_indexs[block_num];
            cur_block_index.block_size -= count;
            cur_block_index.lines_index[block_line_index].block_end -= count;
            //更新索引
            let len = cur_block_index.lines_index.len();
            if block_line_index + 1 < len {
                for b in cur_block_index.lines_index[block_line_index + 1..].iter_mut() {
                    b.block_start = b.block_start.saturating_sub(count);
                    b.block_end = b.block_end.saturating_sub(count);
                }
            }
        }
        Ok(())
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        bytes: &[u8],
        _is_overwrite: bool,
    ) -> ChapResult<()> {
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut cur_block_index = &mut self.block_indexs[block_num];
        if insert_offset < cur_block_index.block_size {
        } else {
            insert_offset = insert_offset - cur_block_index.block_size;
            block_num += 1;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_num == block_num)
            .unwrap();
        block.insert(insert_offset, bytes);
        //重建索引
        cur_block_index = &mut self.block_indexs[block_num];

        let new_block_index = BlockIndex::from_block_bytes(
            block.data.as_continuous(),
            cur_block_index.file_start,
            cur_block_index.block_num,
            cur_block_index.block_size + bytes.len(),
            0,
        )?;
        let old_value = mem::replace(&mut self.block_indexs[block_num], new_block_index);
        drop(old_value);
        Ok(())
    }

    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut cur_block_index = &mut self.block_indexs[block_num];
        if insert_offset < cur_block_index.block_size {
        } else {
            insert_offset = insert_offset - cur_block_index.block_size;
            block_num += 1;
            block_line_index = 0;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_num == block_num)
            .unwrap();
        block.insert(insert_offset, c.encode_utf8(&mut [0; 4]).as_bytes());
        cur_block_index = &mut self.block_indexs[block_num];
        cur_block_index.block_size += c.len_utf8();
        cur_block_index.lines_index[block_line_index].block_end += c.len_utf8();
        //更新索引
        let len = cur_block_index.lines_index.len();
        if block_line_index + 1 < len {
            for b in cur_block_index.lines_index[block_line_index + 1..].iter_mut() {
                b.block_start += c.len_utf8();
                b.block_end += c.len_utf8();
            }
        }
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        let mut block_num = line_meta.get_block_num();
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut cur_block_index = &mut self.block_indexs[block_num];
        if insert_offset < cur_block_index.block_size {
        } else {
            insert_offset = insert_offset - cur_block_index.block_size;
            block_num += 1;
            block_line_index = 0;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_num == block_num)
            .unwrap();
        block.insert(insert_offset, b"\n");
        //更新索引
        cur_block_index = &mut self.block_indexs[block_num];
        cur_block_index.block_size += 1;
        let line_info = &mut cur_block_index.lines_index[block_line_index];
        let block_end = line_info.block_end;
        line_info.block_end = insert_offset + 1;
        line_info.is_complete = true;
        let new_line_info = LineIndex {
            index_num: line_info.index_num + 1,
            block_start: insert_offset + 1,
            block_end: block_end + 1,
            is_complete: line_info.is_complete,
        };
        //更新索引
        cur_block_index
            .lines_index
            .insert(block_line_index + 1, new_line_info);
        for b in cur_block_index.lines_index[block_line_index + 2..].iter_mut() {
            b.block_start += 1;
            b.block_end += 1;
            b.index_num += 1;
        }
        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        let mut buf = [0u8; BLOCK_SIZE];
        let mut file_start = 0;
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        loop {
            if self.cache.contains_key(&file_start) {
                let block = self.cache.get(&file_start).unwrap();
                let txt = block.data.text(..);
                let buf = txt.as_slice();
                w.write(buf.0)?;
                w.write(buf.1)?;
            } else {
                let block = self.find_block(file_start);
                if let Some(b) = block {
                    let txt = b.data.text(..);
                    let buf = txt.as_slice();
                    w.write(buf.0)?;
                    w.write(buf.1)?;
                } else {
                    self.reader.seek(SeekFrom::Start(file_start as u64))?;
                    let n = self.reader.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    w.write(&buf)?;
                }
            }
            file_start += BLOCK_SIZE;
        }
        w.flush()?;
        Ok(())
    }
}

struct Block3<'a> {
    blocks: [Option<&'a Block>; 3],
    block_indexs: &'a [BlockIndex],
}

impl<'a> Block3<'a> {
    fn get_line(
        &self,
        cur_block_num: usize,
        cur_block_offset: usize,
    ) -> Option<(LineBlockStr<'a>, &BlockIndex)> {
        for (i, block) in self.blocks.iter().enumerate() {
            if let Some(b) = block {
                let block_index = self.block_indexs.get(b.block_num).unwrap();
                if cur_block_num > block_index.block_num {
                    continue;
                }
                let option_line_info = block_index.get_line_index(cur_block_offset);
                if option_line_info.is_none() {
                    return None;
                }
                let line_info = option_line_info.unwrap();
                let line_str1 = BlockLineData {
                    data: LineData::GapBytes(
                        b.data.text(line_info.block_start..line_info.block_end),
                    ),
                    block_num: block_index.block_num,
                    block_line_index: line_info.index_num,
                    block_offset: line_info.block_start,
                };
                if line_info.is_complete {
                    //是完整的行
                    let ret: LineBlockStr<'_> = LineBlockStr(Some(line_str1), None);
                    // self.cur_block_offset += ret.text_len();
                    // //查看是否大于当前块
                    // if self.cur_block_offset >= block_index.block_size {
                    //     self.cur_block_num += 1;
                    //     self.cur_block_offset = 0;
                    // }
                    return Some((ret, block_index));
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
                                    block_line_index: next_line_info.index_num,
                                    block_offset: next_line_info.block_start,
                                };
                                let ret: LineBlockStr<'_> =
                                    LineBlockStr(Some(line_str1), Some(line_str2));
                                // self.cur_block_offset += ret.text_len();
                                // //查看是否大于当前块
                                // if self.cur_block_offset >= block_index.block_size {
                                //     self.cur_block_num += 1;
                                //     self.cur_block_offset = 0;
                                // }
                                if next_line_info.is_complete {
                                    //self.cur_line_index += 1;
                                }
                                return Some((ret, block_index));
                            }
                        }
                    }
                    let ret: LineBlockStr<'_> = LineBlockStr(Some(line_str1), None);
                    //self.cur_block_offset += ret.text_len();
                    return Some((ret, block_index));
                }
            }
        }
        None
    }
}

struct GapBlockTextIterRev<'a> {
    blocks: Block3<'a>,
    cur_block_num: usize,        //块编号
    cur_block_line_index: usize, //块内行索引
    cur_block_offset: usize,     //块内偏移
}

impl<'a> Iterator for GapBlockTextIterRev<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (ret, _) = self
            .blocks
            .get_line(self.cur_block_num, self.cur_block_offset)?;
        self.cur_block_line_index = self.cur_block_line_index.saturating_sub(1);
        let block_index = self.blocks.block_indexs.get(self.cur_block_num).unwrap();
        let line_info = &block_index.lines_index[self.cur_block_line_index];
        self.cur_block_offset = line_info.block_start;
        Some(ret)
    }
}

struct GapBlockTextIter<'a> {
    blocks: Block3<'a>,
    cur_block_num: usize,        //块编号
    cur_block_line_index: usize, //块内行索引
    cur_block_offset: usize,     //块内偏移
}

impl<'a> Iterator for GapBlockTextIter<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (ret, block_index) = self
            .blocks
            .get_line(self.cur_block_num, self.cur_block_offset)?;
        self.cur_block_line_index += 1;
        self.cur_block_offset += ret.text_len();
        //查看是否大于当前块
        if self.cur_block_offset >= block_index.block_size {
            self.cur_block_num += 1;
            self.cur_block_line_index = 0;
            self.cur_block_offset = self.cur_block_offset - block_index.block_size;
        }
        Some(ret)
    }
}

mod tests {
    use super::*;
    use crate::textwarp::Line;
    use ratatui::symbols::block;
    #[test]
    fn test_gap_block_text() {
        let gap_block_text = GapBlockText::from_file_path("/home/postgres/a.txt").unwrap();
        for block in gap_block_text.blocks.iter() {
            let block_index = gap_block_text.block_indexs.get(block.block_num).unwrap();
            //打印块所有字段信息
            println!(
                "Block number: {},line_count:{},file_start:{},file_end:{},is_modified:{}",
                block.block_num,
                block_index.line_count,
                block_index.file_start,
                block_index.block_size,
                block.is_modified
            );
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
                    block.text_from_line(line_idx)
                );
            }
        }
        //assert!(gap_block_text.is_ok());
    }
    #[test]
    fn test_gap_block_iter() {
        let mut gap_block_text = GapBlockText::from_file_path("/home/postgres/a.txt").unwrap();
        let mut block_num = 0;
        let mut block_offset = 0;
        let mut block_line_index = 0;

        for _ in 0..122 {
            let mut iter = gap_block_text
                .get_iter(block_num, block_line_index, block_offset)
                .unwrap();
            let line_block_str = iter.next().unwrap();
            block_num = line_block_str.get_end_block_num();
            block_offset = line_block_str.get_end_block_offset();
            block_line_index = line_block_str.get_block_line_index();
            println!(
                "str:{},block_num:{},next_block_offset:{},cur_block_line_index:{}",
                line_block_str, block_num, block_offset, block_line_index
            );
        }
        println!("-----------------reverse-------------------");
        // let line_state = LineState::builder()
        //     .block_num(block_num)
        //     .block_offset(block_offset)
        //     .block_line_index(block_line_index + 1)
        //     .build();

        // let pre_line_state = gap_block_text.get_pre_line_state(&line_state).unwrap();
        // println!(
        //     "pre_block_num:{},pre_block_offset:{},pre_block_index:{}",
        //     pre_line_state.get_block_num(),
        //     pre_line_state.get_block_offset(),
        //     pre_line_state.get_block_line_index()
        // );
        // block_num = pre_line_state.get_block_num();
        // block_offset = pre_line_state.get_block_offset();
        // block_line_index = pre_line_state.get_block_line_index();
        // for _ in 0..125 {
        //     let line_state = {
        //         let mut iter_rev = gap_block_text
        //             .get_iter_rev(block_num, block_line_index, block_offset)
        //             .unwrap();
        //         let line_block_str = iter_rev.next().unwrap();
        //         println!(
        //             "str:{},block_num:{},block_offset:{}",
        //             line_block_str, block_num, block_offset
        //         );
        //         let line_state = LineState::builder()
        //             .block_num(block_num)
        //             .block_offset(block_offset)
        //             .block_line_index(block_line_index)
        //             .build();
        //         line_state
        //     };

        //     let pre_line_state = gap_block_text.get_pre_line_state(&line_state).unwrap();
        //     block_num = pre_line_state.get_block_num();
        //     block_offset = pre_line_state.get_block_offset();
        //     block_line_index = pre_line_state.get_block_line_index();
        // }
    }
}
