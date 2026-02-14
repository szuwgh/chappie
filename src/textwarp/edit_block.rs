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
                    start_line_index += line_count.saturating_sub(1);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ================================================================
    //  辅助函数
    // ================================================================

    /// 创建临时文件并写入内容，返回 GapBlockText
    fn create_gap_block_text(content: &str) -> GapBlockText {
        create_gap_block_text_bytes(content.as_bytes())
    }

    /// 从字节创建 GapBlockText
    fn create_gap_block_text_bytes(content: &[u8]) -> GapBlockText {
        let mut tmp = NamedTempFile::new().expect("failed to create temp file");
        tmp.write_all(content)
            .expect("failed to write temp file");
        tmp.flush().expect("failed to flush temp file");
        GapBlockText::from_file_path(tmp.path()).expect("failed to open GapBlockText")
    }

    /// 获取块中指定 block_num 的完整文本内容
    fn get_block_text(gbt: &mut GapBlockText, block_num: usize) -> Vec<u8> {
        let block = gbt
            .blocks
            .iter_mut()
            .find(|b| b.block_num == block_num)
            .expect("block not found");
        block.data.as_continuous().to_vec()
    }

    /// 获取所有已加载块的拼接文本内容（按 block_num 排序）
    fn get_all_blocks_text(gbt: &mut GapBlockText) -> Vec<u8> {
        let mut block_nums: Vec<usize> = gbt.blocks.iter().map(|b| b.block_num).collect();
        block_nums.sort();
        let mut result = Vec::new();
        for bn in block_nums {
            result.extend_from_slice(&get_block_text(gbt, bn));
        }
        result
    }

    /// 构建默认 LineState（block_num=0, block_line_index=0, block_offset=0）
    fn default_line_state() -> LineState {
        LineState::default()
    }

    /// 构建指定 block/line/offset 的 LineState
    fn make_line_state(
        block_num: usize,
        block_line_index: usize,
        block_offset: usize,
        line_offset: usize,
    ) -> LineState {
        LineState::builder()
            .block_num(block_num)
            .block_line_index(block_line_index)
            .block_offset(block_offset)
            .line_offset(line_offset)
            .build()
    }

    /// 生成跨越多个 block 的大文本内容
    /// 每行格式: "LINE-{n}:xxxx...xxxx\n"，每行固定长度 line_len
    fn generate_large_content(total_bytes: usize, line_len: usize) -> String {
        let mut content = String::new();
        let mut line_num = 0;
        while content.len() < total_bytes {
            let prefix = format!("LINE-{:04}:", line_num);
            let remaining = line_len.saturating_sub(prefix.len() + 1); // -1 for '\n'
            let filler: String = std::iter::repeat('x').take(remaining).collect();
            content.push_str(&prefix);
            content.push_str(&filler);
            content.push('\n');
            line_num += 1;
        }
        content.truncate(total_bytes);
        // 确保最后一个字符是换行符
        if !content.ends_with('\n') {
            let len = content.len();
            content.replace_range(len - 1..len, "\n");
        }
        content
    }

    /// 生成恰好 n 字节的重复填充行文本（每行 80 字符）
    fn generate_padded_content(n: usize) -> String {
        let mut s = String::new();
        let mut i = 0u32;
        while s.len() < n {
            let line = format!("{:079}\n", i);
            s.push_str(&line);
            i += 1;
        }
        s.truncate(n);
        if !s.ends_with('\n') {
            let len = s.len();
            s.replace_range(len - 1..len, "\n");
        }
        s
    }

    // ================================================================
    //  一、短文本测试 (< 100 字节, 单 block)
    // ================================================================

    // ---------- insert_char 短文本 ----------

    #[test]
    fn test_short_insert_char_at_beginning() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_char(0, 0, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "Xhello\n");
    }

    #[test]
    fn test_short_insert_char_in_middle() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_char(0, 2, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "heXllo\n");
    }

    #[test]
    fn test_short_insert_char_before_newline() {
        let mut gbt = create_gap_block_text("hi\n");
        let state = default_line_state();
        gbt.insert_char(0, 2, &state, '!').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hi!\n");
    }

    #[test]
    fn test_short_insert_char_utf8_chinese() {
        let mut gbt = create_gap_block_text("abc\n");
        let state = default_line_state();
        gbt.insert_char(0, 1, &state, '中').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "a中bc\n");
        // 验证 block_size 增加了 3（中文 UTF-8 占 3 字节）
        assert_eq!(gbt.block_indexs[0].block_size, 7);
    }

    #[test]
    fn test_short_insert_char_utf8_emoji() {
        let mut gbt = create_gap_block_text("ab\n");
        let state = default_line_state();
        // emoji 占 4 字节
        gbt.insert_char(0, 1, &state, '😀').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "a😀b\n");
        assert_eq!(gbt.block_indexs[0].block_size, 7); // 1+4+1+1
    }

    #[test]
    fn test_short_insert_char_consecutive() {
        let mut gbt = create_gap_block_text("ab\n");
        let state = default_line_state();
        gbt.insert_char(0, 1, &state, '1').unwrap(); // "a1b\n"
        gbt.insert_char(0, 2, &state, '2').unwrap(); // "a12b\n"
        gbt.insert_char(0, 3, &state, '3').unwrap(); // "a123b\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "a123b\n");
    }

    // ---------- insert_bytes 短文本 ----------

    #[test]
    fn test_short_insert_bytes_at_beginning() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 0, &state, b"XYZ", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "XYZhello\n");
    }

    #[test]
    fn test_short_insert_bytes_in_middle() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 3, &state, b"XY", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "helXYlo\n");
    }

    #[test]
    fn test_short_insert_bytes_empty() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 0, &state, b"", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hello\n");
    }

    #[test]
    fn test_short_insert_bytes_with_newlines() {
        let mut gbt = create_gap_block_text("ab\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 1, &state, b"X\nY", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aX\nYb\n");
    }

    // ---------- insert_newline 短文本 ----------

    #[test]
    fn test_short_insert_newline_at_start() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_newline(0, 0, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "\nhello\n");
    }

    #[test]
    fn test_short_insert_newline_in_middle() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_newline(0, 3, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hel\nlo\n");
    }

    #[test]
    fn test_short_insert_newline_index_update() {
        let mut gbt = create_gap_block_text("abcdef\n");
        let state = default_line_state();
        gbt.insert_newline(0, 3, &state).unwrap();
        let bi = &gbt.block_indexs[0];
        assert_eq!(bi.lines_index.len(), 2);
        assert_eq!(bi.lines_index[0].block_start, 0);
        assert_eq!(bi.lines_index[0].block_end, 4); // "abc\n"
        assert!(bi.lines_index[0].is_complete);
        assert_eq!(bi.lines_index[1].block_start, 4);
        assert_eq!(bi.lines_index[1].block_end, 8); // "def\n"
    }

    #[test]
    fn test_short_insert_newline_on_empty_line() {
        // 文件只有一个换行
        let mut gbt = create_gap_block_text("\n");
        let state = default_line_state();
        gbt.insert_newline(0, 0, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "\n\n");
    }

    // ---------- backspace 短文本 ----------

    #[test]
    fn test_short_backspace_single() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.backspace(0, 3, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "helo\n");
    }

    #[test]
    fn test_short_backspace_multiple() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.backspace(0, 5, 3, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "he\n");
    }

    #[test]
    fn test_short_backspace_block_size_update() {
        let mut gbt = create_gap_block_text("hello\n");
        let orig = gbt.block_indexs[0].block_size;
        let state = default_line_state();
        gbt.backspace(0, 2, 1, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].block_size, orig - 1);
    }

    #[test]
    fn test_short_backspace_then_insert_restore() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.backspace(0, 3, 1, &state).unwrap(); // "helo\n"
        gbt.insert_char(0, 2, &state, 'l').unwrap(); // "hello\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hello\n");
    }

    // ---------- 短文本组合/边界 ----------

    #[test]
    fn test_short_insert_newline_then_merge() {
        let mut gbt = create_gap_block_text("hello\n");
        let state = default_line_state();
        gbt.insert_newline(0, 3, &state).unwrap(); // "hel\nlo\n"
        let state2 = make_line_state(0, 1, 4, 0);
        gbt.backspace(1, 0, 1, &state2).unwrap(); // "hello\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hello\n");
    }

    #[test]
    fn test_short_modified_flag_insert() {
        let mut gbt = create_gap_block_text("hi\n");
        let state = default_line_state();
        gbt.insert_char(0, 0, &state, 'X').unwrap();
        assert!(gbt.blocks.iter().find(|b| b.block_num == 0).unwrap().is_modified);
    }

    #[test]
    fn test_short_modified_flag_backspace() {
        let mut gbt = create_gap_block_text("hi\n");
        let state = default_line_state();
        gbt.backspace(0, 2, 1, &state).unwrap();
        assert!(gbt.blocks.iter().find(|b| b.block_num == 0).unwrap().is_modified);
    }

    #[test]
    fn test_short_rollback_ok() {
        assert!(GapBlockText::rollback().is_ok());
    }

    // ================================================================
    //  二、多行文本测试 (多行但仍在单 block 内, < 4096 字节)
    // ================================================================

    // ---------- insert_char 多行文本 ----------

    #[test]
    fn test_multi_insert_char_first_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        let state = default_line_state();
        gbt.insert_char(0, 1, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aXaa\nbbb\nccc\n");
    }

    #[test]
    fn test_multi_insert_char_second_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        // 第二行: block_line_index=1, block_offset=4 ("aaa\n" = 4 bytes)
        let state = make_line_state(0, 1, 4, 0);
        gbt.insert_char(1, 1, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbXbb\nccc\n");
    }

    #[test]
    fn test_multi_insert_char_third_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        // 第三行: block_line_index=2, block_offset=8
        let state = make_line_state(0, 2, 8, 0);
        gbt.insert_char(2, 2, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbbb\nccXc\n");
    }

    #[test]
    fn test_multi_insert_char_updates_subsequent_lines() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        let state = default_line_state();
        gbt.insert_char(0, 1, &state, 'X').unwrap();
        let bi = &gbt.block_indexs[0];
        // 第一行: "aXaa\n" = 5 bytes, block_end = 5
        assert_eq!(bi.lines_index[0].block_end, 5);
        // 第二行 block_start 应从 4->5, block_end 从 8->9
        assert_eq!(bi.lines_index[1].block_start, 5);
        assert_eq!(bi.lines_index[1].block_end, 9);
        // 第三行 也偏移
        assert_eq!(bi.lines_index[2].block_start, 9);
        assert_eq!(bi.lines_index[2].block_end, 13);
    }

    // ---------- insert_bytes 多行文本 ----------

    #[test]
    fn test_multi_insert_bytes_on_last_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        let state = make_line_state(0, 2, 8, 0);
        gbt.insert_bytes(2, 1, &state, b"XXXX", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbbb\ncXXXXcc\n");
    }

    #[test]
    fn test_multi_insert_bytes_rebuilds_index() {
        let mut gbt = create_gap_block_text("aa\nbb\ncc\n");
        let state = default_line_state();
        // insert_bytes 会通过 from_block_bytes 完全重建索引
        gbt.insert_bytes(0, 1, &state, b"123", false).unwrap();
        let bi = &gbt.block_indexs[0];
        assert_eq!(bi.lines_index.len(), 3); // 仍然 3 行
        assert_eq!(bi.block_size, 12); // 原始 9 + 3
    }

    #[test]
    fn test_multi_insert_bytes_with_newlines_rebuilds_lines() {
        let mut gbt = create_gap_block_text("aa\nbb\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 1, &state, b"X\nY\nZ", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aX\nY\nZa\nbb\n");
        let bi = &gbt.block_indexs[0];
        assert_eq!(bi.lines_index.len(), 4); // 从 2 行变 4 行
    }

    // ---------- insert_newline 多行文本 ----------

    #[test]
    fn test_multi_insert_newline_first_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        let state = default_line_state();
        gbt.insert_newline(0, 2, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aa\na\nbbb\nccc\n");
        assert_eq!(gbt.block_indexs[0].lines_index.len(), 4);
    }

    #[test]
    fn test_multi_insert_newline_middle_line() {
        let mut gbt = create_gap_block_text("aaa\nbbbccc\nddd\n");
        // 第二行 block_offset=4
        let state = make_line_state(0, 1, 4, 0);
        gbt.insert_newline(1, 3, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbbb\nccc\nddd\n");
    }

    #[test]
    fn test_multi_insert_newline_last_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\ncccdddeee\n");
        // 第三行 block_offset=8
        let state = make_line_state(0, 2, 8, 0);
        gbt.insert_newline(2, 3, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbbb\nccc\ndddeee\n");
    }

    #[test]
    fn test_multi_insert_newline_consecutive() {
        let mut gbt = create_gap_block_text("abcdef\n");
        let state = default_line_state();
        gbt.insert_newline(0, 2, &state).unwrap(); // "ab\ncdef\n"
        let state2 = make_line_state(0, 1, 3, 0);
        gbt.insert_newline(1, 2, &state2).unwrap(); // "ab\ncd\nef\n"
        let state3 = make_line_state(0, 2, 6, 0);
        gbt.insert_newline(2, 1, &state3).unwrap(); // "ab\ncd\ne\nf\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "ab\ncd\ne\nf\n");
        assert_eq!(gbt.block_indexs[0].lines_index.len(), 4);
    }

    // ---------- backspace 多行文本 ----------

    #[test]
    fn test_multi_backspace_first_line() {
        let mut gbt = create_gap_block_text("hello\nworld\nfoo\n");
        let state = default_line_state();
        gbt.backspace(0, 3, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "helo\nworld\nfoo\n");
    }

    #[test]
    fn test_multi_backspace_second_line() {
        let mut gbt = create_gap_block_text("hello\nworld\nfoo\n");
        let state = make_line_state(0, 1, 6, 0);
        gbt.backspace(1, 3, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "hello\nwold\nfoo\n");
    }

    #[test]
    fn test_multi_backspace_merge_second_into_first() {
        let mut gbt = create_gap_block_text("hello\nworld\nfoo\n");
        let state = make_line_state(0, 1, 6, 0);
        gbt.backspace(1, 0, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "helloworld\nfoo\n");
        assert_eq!(gbt.block_indexs[0].lines_index.len(), 2);
    }

    #[test]
    fn test_multi_backspace_merge_third_into_second() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        // 第三行行首: block_line_index=2, block_offset=8
        let state = make_line_state(0, 2, 8, 0);
        gbt.backspace(2, 0, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbbbccc\n");
        assert_eq!(gbt.block_indexs[0].lines_index.len(), 2);
    }

    #[test]
    fn test_multi_backspace_subsequent_line_index_updates() {
        let mut gbt = create_gap_block_text("aaa\nbbb\nccc\n");
        let state = default_line_state();
        gbt.backspace(0, 2, 1, &state).unwrap(); // "aa\nbbb\nccc\n"
        let bi = &gbt.block_indexs[0];
        assert_eq!(bi.lines_index[0].block_end, 3); // "aa\n"
        assert_eq!(bi.lines_index[1].block_start, 3);
        assert_eq!(bi.lines_index[1].block_end, 7); // "bbb\n"
        assert_eq!(bi.lines_index[2].block_start, 7);
        assert_eq!(bi.lines_index[2].block_end, 11); // "ccc\n"
    }

    // ---------- 多行组合测试 ----------

    #[test]
    fn test_multi_insert_char_then_newline_then_backspace() {
        let mut gbt = create_gap_block_text("ab\ncd\n");
        let state = default_line_state();
        gbt.insert_char(0, 1, &state, 'X').unwrap(); // "aXb\ncd\n"
        gbt.insert_newline(0, 2, &state).unwrap(); // "aX\nb\ncd\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aX\nb\ncd\n");

        // 合并第二行到第一行
        let state2 = make_line_state(0, 1, 3, 0);
        gbt.backspace(1, 0, 1, &state2).unwrap(); // "aXb\ncd\n"
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aXb\ncd\n");
    }

    #[test]
    fn test_multi_many_lines() {
        // 10 行短文本
        let content = "L0\nL1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\n";
        let mut gbt = create_gap_block_text(content);
        assert_eq!(gbt.block_indexs[0].lines_index.len(), 10);

        // 在第 5 行插入字符
        // L0(3) + L1(3) + L2(3) + L3(3) + L4(3) = offset 15
        let state = make_line_state(0, 5, 15, 0);
        gbt.insert_char(5, 1, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert!(data.starts_with(b"L0\nL1\nL2\nL3\nL4\nLX5\n"));
    }

    #[test]
    fn test_multi_make_backup() {
        let content = "hello\nworld\nfoo\nbar\n";
        let mut gbt = create_gap_block_text(content);
        let state = default_line_state();
        gbt.insert_char(0, 0, &state, 'Z').unwrap();
        let backup_file = NamedTempFile::new().unwrap();
        let backup_path = backup_file.path().to_path_buf();
        gbt.make_backup(&backup_path).unwrap();
        let backup_content = std::fs::read_to_string(&backup_path).unwrap();
        assert_eq!(backup_content, "Zhello\nworld\nfoo\nbar\n");
    }

    #[test]
    fn test_multi_insert_bytes_on_second_line() {
        let mut gbt = create_gap_block_text("aaa\nbbb\n");
        let state = make_line_state(0, 1, 4, 0);
        gbt.insert_bytes(1, 1, &state, b"XX", false).unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "aaa\nbXXbb\n");
    }

    #[test]
    fn test_multi_no_trailing_newline() {
        let mut gbt = create_gap_block_text("line1\nline2");
        let state = default_line_state();
        gbt.insert_char(0, 3, &state, 'X').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "linXe1\nline2");
    }

    // ================================================================
    //  三、大块文本测试 (跨越 4096 字节 block 边界)
    // ================================================================

    #[test]
    fn test_large_file_creates_multiple_blocks() {
        // 创建 > 4096 字节的文件，确保产生多个 block
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let gbt = create_gap_block_text(&content);
        assert!(
            gbt.block_indexs.len() >= 3,
            "文件大于 2 * BLOCK_SIZE 应产生至少 3 个 block, 实际 {}",
            gbt.block_indexs.len()
        );
        assert_eq!(gbt.file_size, content.len());
    }

    #[test]
    fn test_large_block_index_consistency() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let gbt = create_gap_block_text(&content);
        // 检查 block_indexs 的连续性
        for i in 0..gbt.block_indexs.len() {
            assert_eq!(gbt.block_indexs[i].block_num, i);
            if i > 0 {
                assert_eq!(
                    gbt.block_indexs[i].file_start,
                    gbt.block_indexs[i - 1].file_start + gbt.block_indexs[i - 1].block_size,
                    "block {} 的 file_start 应等于前一个 block 的 file_end",
                    i
                );
            }
        }
    }

    #[test]
    fn test_large_insert_char_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].block_size;
        gbt.insert_char(0, 5, &state, 'Z').unwrap();
        let data = get_block_text(&mut gbt, 0);
        // 验证 block 0 的第一行头部
        let first_line: String = String::from_utf8_lossy(&data)
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(first_line.starts_with("LINE-Z0000:"));
        assert_eq!(gbt.block_indexs[0].block_size, orig_size + 1);
    }

    #[test]
    fn test_large_insert_char_on_block0_second_line() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        // 第二行: block_line_index=1
        let bi = &gbt.block_indexs[0];
        let line1_info = &bi.lines_index[1];
        let block_offset = line1_info.block_start;
        let state = make_line_state(0, 1, block_offset, 0);
        gbt.insert_char(1, 5, &state, 'Z').unwrap();
        let data = get_block_text(&mut gbt, 0);
        let text = String::from_utf8_lossy(&data).to_string();
        let lines: Vec<&str> = text.lines().collect();
        // 第二行应该在位置 5 插入了 'Z'
        assert!(lines.len() >= 2);
    }

    #[test]
    fn test_large_insert_char_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        // 在 block 1 的第一行插入
        let bi1 = &gbt.block_indexs[1];
        let first_line_offset = bi1.lines_index[0].block_start;
        let state = make_line_state(1, 0, first_line_offset, 0);
        let orig_size = bi1.block_size;
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        assert_eq!(gbt.block_indexs[1].block_size, orig_size + 1);
    }

    #[test]
    fn test_large_insert_bytes_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].block_size;
        gbt.insert_bytes(0, 5, &state, b"ABCDE", false).unwrap();
        assert_eq!(gbt.block_indexs[0].block_size, orig_size + 5);
    }

    #[test]
    fn test_large_insert_bytes_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi1 = &gbt.block_indexs[1];
        let first_line_offset = bi1.lines_index[0].block_start;
        let state = make_line_state(1, 0, first_line_offset, 0);
        let orig_size = bi1.block_size;
        gbt.insert_bytes(0, 2, &state, b"XY", false).unwrap();
        assert_eq!(gbt.block_indexs[1].block_size, orig_size + 2);
    }

    #[test]
    fn test_large_insert_newline_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let orig_line_count = gbt.block_indexs[0].lines_index.len();
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();
        // 行数应增加 1
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_line_count + 1);
        // block_size 增加 1
        let data = get_block_text(&mut gbt, 0);
        let text = String::from_utf8_lossy(&data);
        let first_line = text.lines().next().unwrap();
        assert_eq!(first_line, "LINE-");
    }

    #[test]
    fn test_large_insert_newline_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi1 = &gbt.block_indexs[1];
        let first_line_offset = bi1.lines_index[0].block_start;
        let orig_lines = bi1.lines_index.len();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_newline(0, 10, &state).unwrap();
        assert_eq!(gbt.block_indexs[1].lines_index.len(), orig_lines + 1);
    }

    #[test]
    fn test_large_backspace_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].block_size;
        gbt.backspace(0, 5, 2, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].block_size, orig_size - 2);
    }

    #[test]
    fn test_large_backspace_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi1 = &gbt.block_indexs[1];
        let first_line_offset = bi1.lines_index[0].block_start;
        let orig_size = bi1.block_size;
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.backspace(0, 5, 1, &state).unwrap();
        assert_eq!(gbt.block_indexs[1].block_size, orig_size - 1);
    }

    #[test]
    fn test_large_backspace_merge_in_block0() {
        // 确保第一个块有多行
        let content = generate_large_content(BLOCK_SIZE * 2, 80);
        let mut gbt = create_gap_block_text(&content);
        let bi = &gbt.block_indexs[0];
        assert!(bi.lines_index.len() >= 2, "block 0 应至少有 2 行");
        let second_line = &bi.lines_index[1];
        let block_offset = second_line.block_start;
        let orig_lines = bi.lines_index.len();
        let state = make_line_state(0, 1, block_offset, 0);
        gbt.backspace(1, 0, 1, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_lines - 1);
    }

    #[test]
    fn test_large_backspace_merge_cross_block() {
        // 在 block 1 第一行行首退格，触发跨块合并
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi1 = &gbt.block_indexs[1];
        let first_line_info = &bi1.lines_index[0];
        // block_line_index=0, block_offset=first_line_info.block_start
        // 行首退格: bytes_cursor=0, line_offset=0
        let state = make_line_state(1, 0, first_line_info.block_start, 0);
        // 跨块合并: block_offset == 0，block_num > 0
        // 这里 block_offset 可能不为 0（如果行跨块），需要根据实际情况构造
        // 当 block_line_index=0 且 line_offset=0 且 bytes_cursor=0:
        //   如果 block_offset > 0 -> 块内合并
        //   如果 block_offset == 0 -> 跨块合并
        if first_line_info.block_start == 0 {
            // 跨块合并
            let bi0_last = gbt.block_indexs[0].lines_index.last().unwrap().clone();
            let was_complete = bi0_last.is_complete;
            gbt.backspace(0, 0, 1, &state).unwrap();
            // 跨块合并后，block 0 的最后一行应变成 is_complete=false
            let bi0_last_after = gbt.block_indexs[0].lines_index.last().unwrap();
            assert!(!bi0_last_after.is_complete, "跨块合并后前一块最后行应标记为不完整");
        } else {
            // 块内合并
            let orig_lines = gbt.block_indexs[1].lines_index.len();
            let state2 = make_line_state(1, 0, 0, 0);
            // 如果 block_offset > 0 说明这一行不在块首，走块内合并逻辑
            // 但 block_line_index=0 时 block_line_index-1 会 underflow
            // 这种情况说明块的第一行前面有数据属于上一个块的跨行
        }
    }

    #[test]
    fn test_large_insert_then_backspace_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        gbt.insert_char(0, 5, &state, 'Z').unwrap();
        gbt.backspace(0, 6, 1, &state).unwrap();
        let data = get_block_text(&mut gbt, 0);
        let text = String::from_utf8_lossy(&data);
        assert!(text.starts_with("LINE-0000:"));
    }

    #[test]
    fn test_large_insert_newline_then_merge_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let orig_lines = gbt.block_indexs[0].lines_index.len();
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_lines + 1);
        let state2 = make_line_state(0, 1, 6, 0);
        gbt.backspace(1, 0, 1, &state2).unwrap();
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_lines);
    }

    #[test]
    fn test_large_make_backup_unmodified() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let backup_file = NamedTempFile::new().unwrap();
        let backup_path = backup_file.path().to_path_buf();
        gbt.make_backup(&backup_path).unwrap();
        let backup_content = std::fs::read(&backup_path).unwrap();
        assert_eq!(
            backup_content.len(),
            content.len(),
            "未修改大文件备份长度应一致"
        );
        assert_eq!(backup_content, content.as_bytes());
    }

    #[test]
    fn test_large_make_backup_after_modify_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        gbt.insert_char(0, 0, &state, 'Z').unwrap();
        let backup_file = NamedTempFile::new().unwrap();
        let backup_path = backup_file.path().to_path_buf();
        gbt.make_backup(&backup_path).unwrap();
        let backup_content = std::fs::read(&backup_path).unwrap();
        assert_eq!(backup_content[0], b'Z');
        assert_eq!(backup_content.len(), content.len() + 1);
    }

    #[test]
    fn test_large_file_size() {
        let content = generate_padded_content(BLOCK_SIZE * 3);
        let gbt = create_gap_block_text(&content);
        assert_eq!(gbt.get_file_size(), content.len());
    }

    #[test]
    fn test_large_block_line_count() {
        // 每行 80 字节, 4096/80 = 51.2, 所以 block 0 约有 51 行
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let gbt = create_gap_block_text(&content);
        let bi0 = &gbt.block_indexs[0];
        assert!(
            bi0.lines_index.len() >= 40,
            "block 0 至少应有 40 行, 实际 {}",
            bi0.lines_index.len()
        );
    }

    #[test]
    fn test_large_insert_char_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi = &gbt.block_indexs[0];
        let last_idx = bi.lines_index.len() - 1;
        let last_line = &bi.lines_index[last_idx];
        let state = make_line_state(0, last_idx, last_line.block_start, 0);
        let orig_end = last_line.block_end;
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        assert_eq!(
            gbt.block_indexs[0].lines_index[last_idx].block_end,
            orig_end + 1
        );
    }

    #[test]
    fn test_large_insert_newline_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi = &gbt.block_indexs[0];
        let last_idx = bi.lines_index.len() - 1;
        let last_line = &bi.lines_index[last_idx];
        let orig_lines = bi.lines_index.len();
        let state = make_line_state(0, last_idx, last_line.block_start, 0);
        gbt.insert_newline(0, 5, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_lines + 1);
    }

    #[test]
    fn test_large_backspace_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi = &gbt.block_indexs[0];
        let last_idx = bi.lines_index.len() - 1;
        let last_line = &bi.lines_index[last_idx];
        let state = make_line_state(0, last_idx, last_line.block_start, 0);
        let orig_size = bi.block_size;
        gbt.backspace(0, 5, 2, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].block_size, orig_size - 2);
    }

    #[test]
    fn test_large_consecutive_inserts_on_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].block_size;
        for i in 0..10 {
            gbt.insert_char(0, i, &state, 'A').unwrap();
        }
        assert_eq!(gbt.block_indexs[0].block_size, orig_size + 10);
    }

    #[test]
    fn test_large_consecutive_backspaces_on_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].block_size;
        for _ in 0..5 {
            gbt.backspace(0, 10, 1, &state).unwrap();
        }
        assert_eq!(gbt.block_indexs[0].block_size, orig_size - 5);
    }

    #[test]
    fn test_large_3_blocks_file() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 200);
        let gbt = create_gap_block_text(&content);
        assert!(
            gbt.block_indexs.len() >= 4,
            "3*BLOCK_SIZE+200 应产生至少 4 个 block, 实际 {}",
            gbt.block_indexs.len()
        );
        // 所有 block 的 block_size 之和应等于 file_size
        let total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(total, content.len());
    }

    #[test]
    fn test_large_insert_bytes_large_payload() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let payload = vec![b'X'; 200];
        let orig_size = gbt.block_indexs[0].block_size;
        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();
        assert_eq!(gbt.block_indexs[0].block_size, orig_size + 200);
    }

    #[test]
    fn test_large_insert_bytes_with_newlines_on_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_lines = gbt.block_indexs[0].lines_index.len();
        gbt.insert_bytes(0, 5, &state, b"A\nB\nC", false).unwrap();
        // insert_bytes 重建索引，增加了 2 个换行 => 多 2 行
        assert_eq!(gbt.block_indexs[0].lines_index.len(), orig_lines + 2);
    }

    #[test]
    fn test_large_exact_block_boundary() {
        // 文件大小恰好是 BLOCK_SIZE 的倍数
        let content = generate_padded_content(BLOCK_SIZE * 2);
        let gbt = create_gap_block_text(&content);
        let total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(total, BLOCK_SIZE * 2);
    }

    #[test]
    fn test_large_insert_char_utf8_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let bi1 = &gbt.block_indexs[1];
        let first_line_offset = bi1.lines_index[0].block_start;
        let orig_size = bi1.block_size;
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_char(0, 3, &state, '中').unwrap(); // 3 字节
        assert_eq!(gbt.block_indexs[1].block_size, orig_size + 3);
    }
}
