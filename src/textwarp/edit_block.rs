use crate::common::error::ChapError;
use crate::common::gap_buffer::GapBuffer;
use crate::common::ring_vec::RingVec;
use crate::textwarp::block::Block;
use crate::textwarp::block::BlockId;
use crate::textwarp::block::BlockIndex;
use crate::textwarp::block::LineIndex;
use crate::textwarp::block::BLOCK_SIZE;
use crate::textwarp::block::BLOKK_NUM;
use crate::textwarp::block::CHAR_GAP_SIZE;
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
    blocks: RingVec<Block>,         // 每个块4KB大小（内存窗口）
    cache: HashMap<BlockId, Block>, // 缓存已修改的块（key = stable block_id）
    file_size: usize,               // 文件大小
    block_indexs: Vec<BlockIndex>,  // 每一块的索引（Vec位置是位置索引，block_id是稳定标识）
    next_id: BlockId,               // 下一个分配的 block_id
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
        let next_id = block_indexs.len(); // 初始 block_id 从 0..len-1，下一个从 len 开始
        Ok(GapBlockText {
            reader: reader,
            blocks: blocks,
            cache: HashMap::new(),
            file_size: file_size as usize,
            block_indexs: block_indexs,
            next_id: next_id,
        })
    }

    fn read_blocks<T: io::Read + Seek>(
        reader: &mut T,
        file_start: usize,
        start_block_id: BlockId,
    ) -> ChapResult<(RingVec<Block>, Vec<BlockIndex>)> {
        let mut blocks = RingVec::with_capacity(BLOKK_NUM);
        let mut buf = [0u8; BLOCK_SIZE];
        let mut block_start_offset: usize = file_start;
        // let mut start_line_index: usize = 0;
        let mut block_indexs = Vec::with_capacity(BLOKK_NUM * 2);
        for i in 0..BLOKK_NUM {
            let block_id = start_block_id + i;
            if let Ok((block, Some(block_index))) =
                Block::from_reader(reader, &mut buf, block_start_offset, block_id, None)
            {
                blocks.push(block);
                let size = block_index.block_size;
                //  let line_count = block_index.line_count;
                //let last_line_index = block_index.lines_index.last().unwrap();

                //if last_line_index.is_complete {
                //是完整行
                // start_line_index += line_count;
                // } else {
                //不是完整行
                //  start_line_index += line_count.saturating_sub(1);
                //}
                block_start_offset += size;
                block_indexs.push(block_index);
            } else {
                break;
            }
        }
        Ok((blocks, block_indexs))
    }

    /// 分配新的 block_id（单调递增，分裂时使用）
    fn alloc_id(&mut self) -> BlockId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 根据 block_id 查找其在 block_indexs 中的位置（O(n)，n 通常很小）
    fn block_pos(&self, block_id: BlockId) -> Option<usize> {
        self.block_indexs
            .iter()
            .position(|bi| bi.block_id == block_id)
    }

    fn read_one_block(
        &mut self,
        block_id: BlockId,
        file_start: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<(Block, Option<BlockIndex>)> {
        // 优先从 cache 取（修改过的块存在 cache 中，key = stable block_id）
        if let Some(o) = self.cache.remove(&block_id) {
            return Ok((o, None));
        }
        let mut buf = [0u8; BLOCK_SIZE];
        Block::from_reader(&mut self.reader, &mut buf, file_start, block_id, check_sum)
    }

    //获取上一个block 并弹入列表中
    fn read_prev_block(
        &mut self,
        block_id: BlockId,
        file_start: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<()> {
        let (block, _) = self.read_one_block(block_id, file_start, check_sum)?;
        let old = self.blocks.push_front(block);
        if let Some(o) = old {
            // 修复：用被驱逐块自身的 block_id 作为 key，而不是新块的 file_start
            if o.is_modified {
                self.cache.insert(o.block_id, o);
            }
        }
        Ok(())
    }

    fn read_next_block(
        &mut self,
        block_id: BlockId,
        file_start: usize,
        check_sum: Option<u32>,
        no_index: bool,
    ) -> ChapResult<()> {
        let (block, block_index) = self.read_one_block(block_id, file_start, check_sum)?;
        let old = self.blocks.push(block);
        if let Some(index) = block_index {
            if no_index {
                self.block_indexs.push(index);
            }
        }
        if let Some(o) = old {
            // 修复：用被驱逐块自身的 block_id 作为 key
            if o.is_modified {
                self.cache.insert(o.block_id, o);
            }
        }
        Ok(())
    }

    fn get_iter_rev(
        &mut self,
        block_id: BlockId,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<GapBlockTextIterRev<'_>> {
        let block3: Block3<'_> = self.get_block3(block_id, block_line_index, block_offset)?;
        Ok(GapBlockTextIterRev {
            blocks: block3,
            cur_block_id: block_id,
            cur_block_line_index: block_line_index,
            cur_block_offset: block_offset,
            exhausted: false,
        })
    }

    fn get_block3(
        &mut self,
        block_id: BlockId,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<Block3<'_>> {
        //查找当前 block_id 是否在内存 blocks 列表中
        loop {
            let j = self
                .blocks
                .iter()
                .enumerate()
                .find(|(_, b)| b.block_id == block_id)
                .map(|(i, _)| i);

            if let Some(i) = j {
                // 找到了；查该块在 block_indexs 中的位置
                let cur_block_id = self.blocks.get(i).unwrap().block_id;
                let pos_in_idx = self.block_pos(cur_block_id).unwrap_or(0);

                if i == 0 {
                    if pos_in_idx == 0 {
                        // 已经是第一个块，没有前驱
                        return Ok(Block3 {
                            blocks: [self.blocks.get(0), self.blocks.get(1), self.blocks.get(2)],
                            block_indexs: &self.block_indexs,
                        });
                    } else {
                        // 读取上一个块，把最后一个块弹出
                        let prev_bi = self.block_indexs[pos_in_idx - 1].clone();
                        self.read_prev_block(
                            prev_bi.block_id,
                            prev_bi.file_start,
                            Some(prev_bi.check_sum),
                        )?;
                        return Ok(Block3 {
                            blocks: [self.blocks.get(1), self.blocks.get(2), self.blocks.get(3)],
                            block_indexs: &self.block_indexs,
                        });
                    }
                } else if i == self.blocks.len() - 1 {
                    // 最后一个块，尝试加载下一个块
                    if let Some(next_bi) = self.block_indexs.get(pos_in_idx + 1).cloned() {
                        self.read_next_block(
                            next_bi.block_id,
                            next_bi.file_start,
                            Some(next_bi.check_sum),
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
                        // 从磁盘上取未索引的块
                        let cur_bi = self.block_indexs[pos_in_idx].clone();
                        let next_block_file_start = cur_bi.file_end();
                        if next_block_file_start >= self.file_size {
                            return Ok(Block3 {
                                blocks: [self.blocks.get(i), None, None],
                                block_indexs: &self.block_indexs,
                            });
                        }
                        let new_block_id = self.alloc_id();
                        self.read_next_block(new_block_id, next_block_file_start, None, true)?;
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
                    // 不是第一个也不是最后一个块
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
            self.reset_blocks(block_id)?;
        }
    }

    fn get_iter(
        &mut self,
        block_id: BlockId,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<GapBlockTextIter<'_>> {
        let block3 = self.get_block3(block_id, block_line_index, block_offset)?;
        Ok(GapBlockTextIter {
            blocks: block3,
            cur_block_id: block_id,
            cur_block_line_index: block_line_index,
            cur_block_offset: block_offset,
        })
    }

    fn find_block(&self, file_start: usize) -> Option<&Block> {
        for b in self.blocks.iter() {
            if file_start == b.file_start {
                return Some(b);
            }
        }
        None
    }

    /// 确保 block_id 对应的块在内存（self.blocks）中。
    ///
    /// 用于 undo/redo：操作发生时的块可能因滚动已被挤出 RingVec，
    /// 调用此函数后可安全执行 `self.blocks.iter_mut().find(|b| b.block_id == id)`。
    pub(crate) fn ensure_block_loaded_impl(&mut self, block_id: BlockId) -> ChapResult<()> {
        // 1. 已在内存中，直接返回
        if self.blocks.iter().any(|b| b.block_id == block_id) {
            return Ok(());
        }

        // 2. 把当前窗口中修改过的块存入 cache，防止 push 时被丢弃
        for block in self.blocks.iter() {
            if block.is_modified && !self.cache.contains_key(&block.block_id) {
                self.cache.insert(block.block_id, block.clone());
            }
        }

        // 3. 找到目标块的 BlockIndex（需要 file_start 和 check_sum）
        let target = self
            .block_indexs
            .iter()
            .find(|bi| bi.block_id == block_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("block_id {} not found in block_indexs", block_id),
                )
            })?
            .clone();

        // 4. read_one_block 优先从 cache 取（key = block_id）
        let (block, _) =
            self.read_one_block(block_id, target.file_start, Some(target.check_sum))?;

        // 5. 推入 RingVec；满时挤出最老的块，若已修改则存入 cache（key = block_id）
        let evicted = self.blocks.push(block);
        if let Some(old) = evicted {
            if old.is_modified {
                self.cache.insert(old.block_id, old);
            }
        }

        Ok(())
    }

    /// 给定 (block_id, abs_byte)，返回该块内包含该字节位置的 block_line_index。
    /// abs_byte 是操作完成后的位置（即操作字节之后），传入前需 - data.len() 定位到被操作字节。
    /// 若 abs_byte >= block_size（跨块边界），退回最后一行索引。
    pub(crate) fn find_block_line_for_offset(
        &self,
        block_id: BlockId,
        abs_byte: usize,
    ) -> Option<usize> {
        let bi = self
            .block_indexs
            .iter()
            .find(|bi| bi.block_id == block_id)?;
        bi.lines_index
            .iter()
            .enumerate()
            .find(|(_, li)| abs_byte >= li.block_start && abs_byte < li.block_end)
            .or_else(|| bi.lines_index.iter().enumerate().last())
            .map(|(i, _)| i)
    }

    fn reset_blocks(&mut self, block_id: BlockId) -> ChapResult<()> {
        // 把当前窗口中修改过的块存入 cache，防止丢失
        for block in self.blocks.iter() {
            if block.is_modified {
                self.cache
                    .entry(block.block_id)
                    .or_insert_with(|| block.clone());
            }
        }

        if let Some(start_pos) = self.block_pos(block_id) {
            // 已在索引中，重新加载以 start_pos 为起始的 BLOKK_NUM 个块
            let mut new_blocks = RingVec::with_capacity(BLOKK_NUM);
            let mut buf = [0u8; BLOCK_SIZE];
            for i in 0..BLOKK_NUM {
                if let Some(bi) = self.block_indexs.get(start_pos + i).cloned() {
                    if let Some(cached) = self.cache.remove(&bi.block_id) {
                        new_blocks.push(cached);
                    } else {
                        match Block::from_reader(
                            &mut self.reader,
                            &mut buf,
                            bi.file_start,
                            bi.block_id,
                            Some(bi.check_sum),
                        ) {
                            Ok((block, _)) => {
                                new_blocks.push(block);
                            }
                            Err(_) => break,
                        }
                    }
                } else {
                    // 需要从磁盘加载新块并扩展索引
                    let last_bi = self.block_indexs.last().unwrap().clone();
                    let next_file_start = last_bi.file_end();
                    if next_file_start >= self.file_size {
                        break;
                    }
                    let new_id = self.alloc_id();
                    match Block::from_reader(
                        &mut self.reader,
                        &mut buf,
                        next_file_start,
                        new_id,
                        None,
                    ) {
                        Ok((block, Some(bi))) => {
                            self.block_indexs.push(bi);
                            new_blocks.push(block);
                        }
                        _ => break,
                    }
                }
            }
            self.blocks = new_blocks;
        } else {
            // block_id 不在索引中：从文件末尾继续加载，直到找到为止
            let mut buf = [0u8; BLOCK_SIZE];
            'out: loop {
                let last_bi = self.block_indexs.last().unwrap().clone();
                let next_file_start = last_bi.file_end();
                if next_file_start >= self.file_size {
                    break;
                }
                let new_id = self.alloc_id();
                match Block::from_reader(&mut self.reader, &mut buf, next_file_start, new_id, None)
                {
                    Ok((block, Some(bi))) => {
                        self.block_indexs.push(bi);
                        let found = block.block_id == block_id;
                        if let Some(evicted) = self.blocks.push(block) {
                            if evicted.is_modified {
                                self.cache.insert(evicted.block_id, evicted);
                            }
                        }
                        if found {
                            break 'out;
                        }
                    }
                    _ => break,
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
        let block_id = state.block_num; // LineState.block_num 语义上是 block_id
        let block_offset = state.block_offset;
        // 在内存 blocks 中找到目标 block_id 的位置
        let j = self
            .blocks
            .iter()
            .enumerate()
            .find(|(_, b)| b.block_id == block_id)
            .map(|(i, _)| i);
        if let Some(i) = j {
            let blocks = Block3 {
                blocks: [
                    self.blocks.get(i),
                    self.blocks.get(i + 1),
                    self.blocks.get(i + 2),
                ],
                block_indexs: &self.block_indexs,
            };
            let (l, _) = blocks.get_line(block_id, block_offset)?;
            return Some(l);
        }
        None
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        let mut line_index = state.get_line_index();
        let mut line_end = state.get_line_end();
        let block_id = state.get_block_num(); // block_num 字段存储 block_id
        let mut block_line_index = state.get_block_line_index();
        let mut block_offset = state.get_block_offset();

        // 通过 block_id 查找位置，再访问 block_indexs
        let pos = self.block_pos(block_id)?;
        let block_index = &self.block_indexs[pos];

        let line = self.get_line(&state).unwrap();
        let mut next_block_id = block_id;
        if state.get_line_end() >= line.text_len() {
            if state.get_block_line_end() >= block_index.block_size - 1 {
                // 越过当前块，进入下一块
                next_block_id = self.block_indexs.get(pos + 1)?.block_id;
                block_line_index = 0;
                block_offset = state
                    .get_block_line_end()
                    .saturating_sub(block_index.block_size);
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
            .block_num(next_block_id)
            .block_line_index(block_line_index)
            .block_offset(block_offset)
            .start_line_num(state.get_line_num())
            .build();
        Some(p)
    }

    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState> {
        let block_id = state.get_block_num(); // block_num 字段存储 block_id
        let block_line_index = state.get_block_line_index();
        let block_offset = state.get_block_offset();

        // 通过 block_id 找位置
        let pos = self.block_pos(block_id)?;

        // 判断是否在文件最开头
        if pos == 0 && block_line_index == 0 && block_offset == 0 {
            return None;
        }

        if state.get_line_offset() > 0 {
            //在行中间
            let p = LineState::builder()
                .start_line_num(state.get_line_num())
                .line_index(state.get_line_index())
                .line_offset(state.get_line_offset())
                .block_num(block_id)
                .block_line_index(block_line_index)
                .block_offset(block_offset)
                .build();
            return Some(p);
        }
        if block_line_index > 0 {
            let last_block_line_index = block_line_index.saturating_sub(1);
            let block = &self.block_indexs[pos];
            let last_line_info = &block.lines_index[last_block_line_index];
            if last_block_line_index == 0 {
                if pos == 0 {
                    //已经是第一个块了
                    let p = LineState::builder()
                        .start_line_num(state.get_line_num())
                        .line_index(state.get_line_index().saturating_sub(1))
                        .line_offset(0)
                        .block_num(block_id)
                        .block_line_index(last_block_line_index)
                        .block_offset(0)
                        .build();
                    return Some(p);
                }
                // 如果是第一个块的第一行 要判断是否是一行的起始位置
                //取上一个块的最后一行
                let last_block_index = &self.block_indexs[pos - 1];
                let last_block_id = last_block_index.block_id;
                let last_last_line_info = last_block_index.lines_index.last().unwrap();
                if last_last_line_info.is_complete {
                    //上上一行是完整行
                    let p = LineState::builder()
                        .start_line_num(state.get_line_num())
                        .line_index(state.get_line_index().saturating_sub(1))
                        .line_offset(0)
                        .block_num(block_id)
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
                        .block_num(last_block_id)
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
                    .block_num(block_id)
                    .block_line_index(last_block_line_index)
                    .block_offset(last_line_info.block_start)
                    .build();
                return Some(p);
            }
        } else {
            if pos == 0 {
                return None;
            }
            let last_block_index = &self.block_indexs[pos - 1];
            let last_block_id = last_block_index.block_id;
            let last_line_info = last_block_index.lines_index.last().unwrap();
            let p = LineState::builder()
                .start_line_num(state.get_line_num())
                .line_index(state.get_line_index().saturating_sub(1))
                .line_offset(0)
                .block_num(last_block_id)
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
        if self.block_pos(meta.get_block_num()) == Some(0)
            && meta.get_block_line_index() == 0
            && meta.get_block_offset() == 0
        {
            return false;
        }
        return true;
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        let last_block_index = self.block_indexs.last().unwrap();
        if meta.get_block_num() == last_block_index.block_id
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

impl GapBlockText {
    fn split_block(&mut self, block_id: BlockId) -> ChapResult<()> {
        let pos = self.block_pos(block_id).ok_or_else(|| {
            ChapError::Unexpected(format!("split_block: block_id {} not found", block_id))
        })?;
        let block_size = self.block_indexs[pos].block_size;
        let file_start = self.block_indexs[pos].file_start;

        // ① 找切分点：is_complete 行中 block_end 最接近 block_size/2 的那条
        let half = block_size / 2;
        let split_byte = self.block_indexs[pos]
            .lines_index
            .iter()
            .filter(|l| l.is_complete)
            .min_by_key(|l| (l.block_end as isize - half as isize).abs())
            .map(|l| l.block_end)
            .unwrap_or(half);

        if split_byte == 0 || split_byte >= block_size {
            return Ok(()); // 无法切分（整块只有一行且超大）
        }

        // ② 直接从 as_continuous() 切片创建两个新 GapBuffer，消除 to_vec() 中间拷贝
        let (data_a, data_b) = {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "split_block: block_id {} not in memory",
                        block_id
                    ))
                })?;
            let bytes = block.as_continuous(); // 仅移动 gap，不分配
            let data_b = GapBuffer::from_bytes(&bytes[split_byte..], CHAR_GAP_SIZE);
            let data_a = GapBuffer::from_bytes(&bytes[..split_byte], CHAR_GAP_SIZE);
            (data_a, data_b)
        }; // self.blocks 借用在此结束

        // ③ 拆分 lines_index：mem::take + drain 避免 clone lines_a 的 Vec
        //    partition_point 利用 block_end 单调递增做二分，O(log n)
        let split_line_count = self.block_indexs[pos]
            .lines_index
            .partition_point(|l| l.block_end <= split_byte);

        let mut all_lines = std::mem::take(&mut self.block_indexs[pos].lines_index);
        let mut lines_b: Vec<LineIndex> = all_lines.drain(split_line_count..).collect();
        for (i, l) in lines_b.iter_mut().enumerate() {
            l.block_start -= split_byte;
            l.block_end -= split_byte;
            l.index_num = i;
        }
        let lines_a = all_lines; // drain 后剩余前半段，复用原 Vec 内存

        let count_a = lines_a.iter().filter(|l| l.is_complete).count();
        let count_b = lines_b.iter().filter(|l| l.is_complete).count();

        // ④ 分配 block B 的新 block_id（单调递增，不影响其他块的 block_id）
        let new_block_id = self.alloc_id();

        // 构造 Block B
        let block_b = Block {
            data: data_b,
            file_start: file_start + split_byte,
            file_end: file_start + block_size,
            block_id: new_block_id,
            is_modified: true,
        };
        // 更新 Block A
        {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .unwrap();
            block.data = data_a;
            block.file_end = file_start + split_byte;
            block.is_modified = true;
        }

        // ⑤ 更新 block_indexs：替换 A，插入 B（不需要重编号后续块！）
        self.block_indexs[pos] = BlockIndex {
            file_start,
            block_id, // 保持原 block_id 不变
            line_count: count_a,
            block_size: split_byte,
            lines_index: lines_a,
            check_sum: 0,
        };
        self.block_indexs.insert(
            pos + 1,
            BlockIndex {
                file_start: file_start + split_byte,
                block_id: new_block_id, // 新 block_id，稳定
                line_count: count_b,
                block_size: block_size - split_byte,
                lines_index: lines_b,
                check_sum: 0,
            },
        );
        // ⑥ cache 以 block_id 为 key，其他块 block_id 未变，无需更新

        // ⑦ push block_b；被挤出时存入 cache（key = block_id）
        if let Some(evicted) = self.blocks.push(block_b) {
            if evicted.is_modified {
                self.cache.entry(evicted.block_id).or_insert(evicted);
            }
        }
        // 按 file_start 排序，维持物理顺序供 Block3 窗口使用
        self.blocks.sort_by_key(|b| b.file_start);

        Ok(())
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
    ) -> ChapResult<Vec<u8>> {
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        //合并行

        if line_meta.line_offset == 0 && bytes_cursor == 0 {
            let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
            let mut pos = self.block_pos(block_id).unwrap();
            while insert_offset >= self.block_indexs[pos].block_size {
                insert_offset -= self.block_indexs[pos].block_size;
                pos += 1;
                block_id = self.block_indexs[pos].block_id;
                block_line_index = 0;
            }
            //在行首 需要合并上一行
            if block_offset > 0 {
                // 先计算 pos（&self 方法），再取 blocks 的可变借用
                //  let pos = self.block_pos(block_id).unwrap();
                //  let insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
                log::debug!("backspace at block_id {}, line_offset {}, insert_offset {}, block_line_index {}",
                    block_id, line_meta.line_offset, insert_offset, block_line_index);
                let deleted_bytes = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.block_id == block_id)
                    .unwrap()
                    .backspace(insert_offset, 1);
                // blocks 的可变借用在上一语句结束后由 NLL 释放
                let cur_block_index = &mut self.block_indexs[pos];
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
                return Ok(deleted_bytes);
            } else {
                //在块首 需要合并上一个块的最后一行
                let pos = self.block_pos(block_id).unwrap();
                if pos > 0 {
                    let prev_block_id = self.block_indexs[pos - 1].block_id;
                    self.blocks
                        .iter_mut()
                        .find(|b| b.block_id == prev_block_id)
                        .unwrap()
                        .backspace_last(1);
                    // blocks 借用释放
                    self.block_indexs[pos - 1]
                        .lines_index
                        .last_mut()
                        .unwrap()
                        .is_complete = false;
                    return Ok(vec![b'\n']);
                }
            }
        } else {
            let mut pos = self.block_pos(block_id).unwrap();
            let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
            while insert_offset >= self.block_indexs[pos].block_size {
                insert_offset -= self.block_indexs[pos].block_size;
                pos += 1;
                block_id = self.block_indexs[pos].block_id;
                block_line_index = 0;
            }
            let deleted_bytes = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .unwrap()
                .backspace(insert_offset, count);
            // blocks 借用释放
            let cur_block_index = &mut self.block_indexs[pos];
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
            return Ok(deleted_bytes);
        }
        Ok(vec![])
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        bytes: &[u8],
        _is_overwrite: bool,
    ) -> ChapResult<()> {
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut pos = self.block_pos(block_id).unwrap();
        while insert_offset >= self.block_indexs[pos].block_size {
            insert_offset -= self.block_indexs[pos].block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        block.insert(insert_offset, bytes);
        //重建索引
        let new_block_index = BlockIndex::from_block_bytes(
            block.data.as_continuous(),
            self.block_indexs[pos].file_start,
            self.block_indexs[pos].block_id,
            self.block_indexs[pos].block_size + bytes.len(),
            0,
        )?;
        let old_value = mem::replace(&mut self.block_indexs[pos], new_block_index);
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
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut pos = self.block_pos(block_id).unwrap();
        while insert_offset >= self.block_indexs[pos].block_size {
            insert_offset -= self.block_indexs[pos].block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
            block_line_index = 0;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        block.insert(insert_offset, c.encode_utf8(&mut [0; 4]).as_bytes());
        let cur_block_index = &mut self.block_indexs[pos];
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
        if self.block_indexs[pos].block_size > BLOCK_SIZE {
            self.split_block(block_id)?;
        }
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut pos = self.block_pos(block_id).unwrap();
        while insert_offset >= self.block_indexs[pos].block_size {
            insert_offset -= self.block_indexs[pos].block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
            block_line_index = 0;
        }
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        block.insert(insert_offset, b"\n");
        //更新索引
        let cur_block_index = &mut self.block_indexs[pos];
        cur_block_index.block_size += 1;
        let line_info = &mut cur_block_index.lines_index[block_line_index];
        let block_end = line_info.block_end;
        let was_complete = line_info.is_complete;
        line_info.block_end = insert_offset + 1;
        line_info.is_complete = true;
        let new_line_info = LineIndex {
            index_num: line_info.index_num + 1,
            block_start: insert_offset + 1,
            block_end: block_end + 1,
            is_complete: was_complete,
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
        if self.block_indexs[pos].block_size > BLOCK_SIZE {
            self.split_block(block_id)?;
        }
        Ok(())
    }

    fn delete_line(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut pos = self.block_pos(block_id).unwrap();
        while insert_offset >= self.block_indexs[pos].block_size {
            insert_offset -= self.block_indexs[pos].block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
            block_line_index = 0;
        }
        //在行首 需要合并上一行
        if insert_offset > 0 {
            // 先计算 pos（&self 方法），再取 blocks 的可变借用
            // let pos = self.block_pos(block_id).unwrap();
            log::debug!(
                "backspace at block_id {}, line_offset {}, insert_offset {}, block_line_index {}",
                block_id,
                line_meta.line_offset,
                insert_offset,
                block_line_index
            );
            self.blocks
                .iter_mut()
                .find(|b| b.block_id == block_id)
                .unwrap()
                .backspace(insert_offset, 1);
            // blocks 的可变借用在上一语句结束后由 NLL 释放
            let cur_block_index = &mut self.block_indexs[pos];
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
            return Ok(());
        } else {
            //在块首 需要合并上一个块的最后一行
            let pos = self.block_pos(block_id).unwrap();
            if pos > 0 {
                let prev_block_id = self.block_indexs[pos - 1].block_id;
                self.blocks
                    .iter_mut()
                    .find(|b| b.block_id == prev_block_id)
                    .unwrap()
                    .backspace_last(1);
                // blocks 借用释放
                self.block_indexs[pos - 1]
                    .lines_index
                    .last_mut()
                    .unwrap()
                    .is_complete = false;
                return Ok(());
            }
        }

        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        let mut buf = [0u8; BLOCK_SIZE];
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        // 按 block_indexs 迭代，确保 split 后的碎块也被写入
        let block_infos: Vec<(BlockId, usize, usize)> = self
            .block_indexs
            .iter()
            .map(|bi| (bi.block_id, bi.file_start, bi.block_size))
            .collect();
        for (block_id, file_start, block_size) in block_infos {
            if self.cache.contains_key(&block_id) {
                // cache 以 block_id 为 key
                let block = self.cache.get(&block_id).unwrap();
                let txt = block.data.text(..);
                let s = txt.as_slice();
                w.write(s.0)?;
                w.write(s.1)?;
            } else {
                let block = self.find_block(file_start);
                if let Some(b) = block {
                    let txt = b.data.text(..);
                    let s = txt.as_slice();
                    w.write(s.0)?;
                    w.write(s.1)?;
                } else {
                    self.reader.seek(SeekFrom::Start(file_start as u64))?;
                    let n = self.reader.read(&mut buf[..block_size.min(BLOCK_SIZE)])?;
                    if n == 0 {
                        break;
                    }
                    w.write(&buf[..n])?;
                }
            }
        }
        w.flush()?;
        Ok(())
    }

    fn ensure_block_loaded(&mut self, block_num: usize) -> ChapResult<()> {
        self.ensure_block_loaded_impl(block_num)
    }
}

struct Block3<'a> {
    blocks: [Option<&'a Block>; 3],
    block_indexs: &'a [BlockIndex],
}

impl<'a> Block3<'a> {
    fn get_line(
        &self,
        cur_block_id: BlockId,
        cur_block_offset: usize,
    ) -> Option<(LineBlockStr<'a>, &BlockIndex)> {
        for (i, block) in self.blocks.iter().enumerate() {
            if let Some(b) = block {
                // 通过 block_id 找对应的 BlockIndex
                let block_index = self
                    .block_indexs
                    .iter()
                    .find(|bi| bi.block_id == b.block_id)?;
                // 跳过不是目标块的块
                if b.block_id != cur_block_id {
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
                    block_num: block_index.block_id, // block_num 字段存储 block_id
                    block_line_index: line_info.index_num,
                    block_offset: line_info.block_start,
                };
                if line_info.is_complete {
                    //是完整的行
                    let ret: LineBlockStr<'_> = LineBlockStr(Some(line_str1), None);
                    return Some((ret, block_index));
                } else {
                    //不完整行 需要合并一行的下部分 一行的下一个部分在 下一个 block中
                    //获取下一个block
                    if i == 0 {
                        // 找当前块在 block_indexs 中的位置，取下一个
                        let cur_pos = self
                            .block_indexs
                            .iter()
                            .position(|bi| bi.block_id == b.block_id)?;
                        let next_block_index = self.block_indexs.get(cur_pos + 1)?;
                        if let Some(next_block) = self.blocks[1] {
                            if !next_block_index.lines_index.is_empty() {
                                let next_line_info = &next_block_index.lines_index[0];
                                let line_str2 = BlockLineData {
                                    data: LineData::GapBytes(next_block.data.text(
                                        next_line_info.block_start..next_line_info.block_end,
                                    )),
                                    block_num: next_block_index.block_id, // block_id
                                    block_line_index: next_line_info.index_num,
                                    block_offset: next_line_info.block_start,
                                };
                                let ret: LineBlockStr<'_> =
                                    LineBlockStr(Some(line_str1), Some(line_str2));
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
    cur_block_id: BlockId,       //当前块的稳定 block_id
    cur_block_line_index: usize, //块内行索引
    cur_block_offset: usize,     //块内偏移
    exhausted: bool,
}

impl<'a> Iterator for GapBlockTextIterRev<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        let (ret, _) = self
            .blocks
            .get_line(self.cur_block_id, self.cur_block_offset)?;
        let block_index = self
            .blocks
            .block_indexs
            .iter()
            .find(|bi| bi.block_id == self.cur_block_id)?;
        let current_line_info = block_index.get_line_index(self.cur_block_offset)?;

        if current_line_info.index_num > 0 {
            let prev_index = current_line_info.index_num.saturating_sub(1);
            let line_info = block_index.lines_index.get(prev_index)?;
            self.cur_block_line_index = prev_index;
            self.cur_block_offset = line_info.block_start;
        } else {
            let cur_pos = self
                .blocks
                .block_indexs
                .iter()
                .position(|bi| bi.block_id == self.cur_block_id)?;
            if cur_pos == 0 {
                self.exhausted = true;
            } else {
                let prev_block_index = self.blocks.block_indexs.get(cur_pos - 1)?;
                let prev_line_info = prev_block_index.lines_index.last()?;
                self.cur_block_id = prev_block_index.block_id;
                self.cur_block_line_index = prev_line_info.index_num;
                self.cur_block_offset = prev_line_info.block_start;
            }
        }
        Some(ret)
    }
}

struct GapBlockTextIter<'a> {
    blocks: Block3<'a>,
    cur_block_id: BlockId,       //当前块的稳定 block_id
    cur_block_line_index: usize, //块内行索引
    cur_block_offset: usize,     //块内偏移
}

impl<'a> Iterator for GapBlockTextIter<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (ret, block_index) = self
            .blocks
            .get_line(self.cur_block_id, self.cur_block_offset)?;
        self.cur_block_line_index += 1;
        self.cur_block_offset += ret.text_len();
        //查看是否大于当前块
        if self.cur_block_offset >= block_index.block_size {
            // 通过 block_indexs 找下一个块的 block_id
            if let Some(cur_pos) = self
                .blocks
                .block_indexs
                .iter()
                .position(|bi| bi.block_id == self.cur_block_id)
            {
                if let Some(next_bi) = self.blocks.block_indexs.get(cur_pos + 1) {
                    self.cur_block_id = next_bi.block_id;
                }
            }
            self.cur_block_line_index = 0;
            self.cur_block_offset = self.cur_block_offset.saturating_sub(block_index.block_size);
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
        tmp.write_all(content).expect("failed to write temp file");
        tmp.flush().expect("failed to flush temp file");
        GapBlockText::from_file_path(tmp.path()).expect("failed to open GapBlockText")
    }

    /// 获取块中指定 block_id 的完整文本内容
    fn get_block_text(gbt: &mut GapBlockText, block_id: BlockId) -> Vec<u8> {
        let block = gbt
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .expect("block not found");
        block.data.as_continuous().to_vec()
    }

    /// 获取所有已加载块的拼接文本内容（按 file_start 排序，维持物理顺序）
    fn get_all_blocks_text(gbt: &mut GapBlockText) -> Vec<u8> {
        let mut block_ids: Vec<(usize, BlockId)> = gbt
            .blocks
            .iter()
            .map(|b| (b.file_start, b.block_id))
            .collect();
        block_ids.sort_by_key(|(fs, _)| *fs);
        let mut result = Vec::new();
        for (_, bid) in block_ids {
            result.extend_from_slice(&get_block_text(gbt, bid));
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

    #[test]
    fn test_short_insert_newline_preserves_incomplete_tail() {
        let mut gbt = create_gap_block_text("abcdef");
        let state = default_line_state();
        gbt.insert_newline(0, 3, &state).unwrap();
        let bi = &gbt.block_indexs[0];
        assert!(bi.lines_index[0].is_complete, "前半行应变为完整行");
        assert!(
            !bi.lines_index[1].is_complete,
            "原始尾行无换行时，后半行应保持不完整"
        );
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
        assert!(
            gbt.blocks
                .iter()
                .find(|b| b.block_id == 0)
                .unwrap()
                .is_modified
        );
    }

    #[test]
    fn test_short_modified_flag_backspace() {
        let mut gbt = create_gap_block_text("hi\n");
        let state = default_line_state();
        gbt.backspace(0, 2, 1, &state).unwrap();
        assert!(
            gbt.blocks
                .iter()
                .find(|b| b.block_id == 0)
                .unwrap()
                .is_modified
        );
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
            assert_eq!(gbt.block_indexs[i].block_id, i);
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
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        gbt.insert_char(0, 5, &state, 'Z').unwrap();
        let data = get_block_text(&mut gbt, 0);
        // 验证 block 0 的第一行头部
        let first_line: String = String::from_utf8_lossy(&data)
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(first_line.starts_with("LINE-Z0000:"));
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(new_total, orig_total + 1);
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
        let first_line_offset = gbt.block_indexs[1].lines_index[0].block_start;
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(new_total, orig_total + 1);
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
        let orig_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();
        let new_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        // 行数应增加 1
        assert_eq!(new_total_lines, orig_total_lines + 1);
        // block0 第一行应为 "LINE-"
        let data = get_block_text(&mut gbt, 0);
        let text = String::from_utf8_lossy(&data);
        let first_line = text.lines().next().unwrap();
        assert_eq!(first_line, "LINE-");
    }

    #[test]
    fn test_large_insert_newline_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let first_line_offset = gbt.block_indexs[1].lines_index[0].block_start;
        let orig_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_newline(0, 10, &state).unwrap();
        let new_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        assert_eq!(new_total_lines, orig_total_lines + 1);
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
            assert!(
                !bi0_last_after.is_complete,
                "跨块合并后前一块最后行应标记为不完整"
            );
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
        let orig_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();
        let after_insert: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        assert_eq!(after_insert, orig_total_lines + 1);
        let state2 = make_line_state(0, 1, 6, 0);
        gbt.backspace(1, 0, 1, &state2).unwrap();
        let after_backspace: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        assert_eq!(after_backspace, orig_total_lines);
    }

    #[test]
    fn test_iter_rev_does_not_panic_on_stale_block_line_index() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();

        let block_id = gbt.block_indexs[0].block_id;
        let valid_offset = gbt.block_indexs[0].lines_index[0].block_start;
        let mut iter = gbt.get_iter_rev(block_id, 69, valid_offset).unwrap();

        assert!(iter.next().is_some(), "当前行应可被反向迭代返回");
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
        let last_idx = gbt.block_indexs[0].lines_index.len() - 1;
        let last_block_start = gbt.block_indexs[0].lines_index[last_idx].block_start;
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        let state = make_line_state(0, last_idx, last_block_start, 0);
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(new_total, orig_total + 1);
    }

    #[test]
    fn test_large_insert_newline_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let last_idx = gbt.block_indexs[0].lines_index.len() - 1;
        let last_block_start = gbt.block_indexs[0].lines_index[last_idx].block_start;
        let orig_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        let state = make_line_state(0, last_idx, last_block_start, 0);
        gbt.insert_newline(0, 5, &state).unwrap();
        let new_total_lines: usize = gbt.block_indexs.iter().map(|bi| bi.lines_index.len()).sum();
        assert_eq!(new_total_lines, orig_total_lines + 1);
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
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        for i in 0..10 {
            gbt.insert_char(0, i, &state, 'A').unwrap();
        }
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(new_total, orig_total + 10);
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
        let first_line_offset = gbt.block_indexs[1].lines_index[0].block_start;
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_char(0, 3, &state, '中').unwrap(); // 3 字节
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.block_size).sum();
        assert_eq!(new_total, orig_total + 3);
    }

    // ================================================================
    //  split_block 测试
    //  使用 generate_padded_content(BLOCK_SIZE*2+100) 构造文件：
    //    block 0 = 4096 字节，51 条完整行(各 80B) + 1 条不完整行
    //    split_byte ≈ 2080（第 26 条完整行的 block_end）
    // ================================================================

    // ---------- 1. 数据完整性 ----------

    /// A 半 + B 半字节拼接 == 分裂前原始字节
    #[test]
    fn test_split_block_data_integrity() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let id_a = gbt.block_indexs[0].block_id;
        let orig = get_block_text(&mut gbt, id_a);

        gbt.split_block(id_a).unwrap();

        // block B 在 block_indexs[1]，拿它的 block_id
        let id_b = gbt.block_indexs[1].block_id;
        let mut combined = get_block_text(&mut gbt, id_a);
        combined.extend_from_slice(&get_block_text(&mut gbt, id_b));
        assert_eq!(combined, orig, "A+B 必须等于原始字节");
    }

    /// 三次 split 后，所有已加载块拼接 == 原始文件全部内容
    #[test]
    fn test_split_block_twice_data_integrity() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig = get_all_blocks_text(&mut gbt);

        gbt.split_block(0).unwrap();
        gbt.split_block(0).unwrap(); // 再次对已缩小的 block 0 分裂

        assert_eq!(get_all_blocks_text(&mut gbt), orig);
    }

    // ---------- 2. block_size / block_indexs ----------

    /// 分裂后 block_indexs 条目增加 1
    #[test]
    fn test_split_block_indexs_count_increases() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let before = gbt.block_indexs.len();

        gbt.split_block(0).unwrap();

        assert_eq!(gbt.block_indexs.len(), before + 1);
    }

    /// A.block_size + B.block_size == 原始 block_size，且两半均非零
    #[test]
    fn test_split_block_size_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_size = gbt.block_indexs[0].block_size;

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].block_size;
        let size_b = gbt.block_indexs[1].block_size;
        assert_eq!(size_a + size_b, orig_size);
        assert!(size_a > 0, "A 不应为空");
        assert!(size_b > 0, "B 不应为空");
    }

    /// 分裂点在原块 25%～75% 范围内（接近中点）
    #[test]
    fn test_split_block_near_midpoint() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_size = gbt.block_indexs[0].block_size;

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].block_size;
        assert!(size_a > orig_size / 4, "分裂点不应太靠前");
        assert!(size_a < orig_size * 3 / 4, "分裂点不应太靠后");
    }

    /// 分裂后 block A 的 block_id 保持不变，block B 获得新分配的 block_id
    #[test]
    fn test_split_block_nums_updated() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_id_a = gbt.block_indexs[0].block_id; // 初始 = 0

        gbt.split_block(orig_id_a).unwrap();

        // A 的 block_id 保持不变
        assert_eq!(gbt.block_indexs[0].block_id, orig_id_a);
        // B 获得新分配的 block_id（不同于 A）
        assert_ne!(
            gbt.block_indexs[1].block_id, orig_id_a,
            "B 应有新分配的 block_id"
        );
    }

    /// B.file_start == A.file_start + A.block_size（物理地址连续）
    #[test]
    fn test_split_block_file_start_continuity() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_file_start = gbt.block_indexs[0].file_start;

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].block_size;
        assert_eq!(
            gbt.block_indexs[1].file_start,
            orig_file_start + size_a,
            "B 的 file_start 应紧跟 A"
        );
    }

    /// split_block(0) 后，后续块的 block_id 保持不变（不重新编号），file_start 也不变
    #[test]
    fn test_split_block_renumbers_subsequent_blocks() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_b1_file_start = gbt.block_indexs[1].file_start;
        let orig_b2_file_start = gbt.block_indexs[2].file_start;
        let orig_b1_id = gbt.block_indexs[1].block_id;
        let orig_b2_id = gbt.block_indexs[2].block_id;

        gbt.split_block(0).unwrap();

        // block_indexs: [A, B(new), orig_block_1, orig_block_2]
        // 后续块 block_id 保持稳定，不重新编号
        assert_eq!(
            gbt.block_indexs[2].block_id, orig_b1_id,
            "后续块 block_id 不变"
        );
        assert_eq!(gbt.block_indexs[2].file_start, orig_b1_file_start);
        assert_eq!(
            gbt.block_indexs[3].block_id, orig_b2_id,
            "后续块 block_id 不变"
        );
        assert_eq!(gbt.block_indexs[3].file_start, orig_b2_file_start);
    }

    /// split_block(1)：block 0 完全不受影响，后续块 block_id 保持稳定
    #[test]
    fn test_split_non_first_block_does_not_affect_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);
        let b0_id = gbt.block_indexs[0].block_id;
        let b0_size = gbt.block_indexs[0].block_size;
        let b0_file_start = gbt.block_indexs[0].file_start;
        let b0_line_count = gbt.block_indexs[0].line_count;
        let b1_id = gbt.block_indexs[1].block_id; // = 1
        let b2_id = gbt.block_indexs[2].block_id; // = 2

        gbt.split_block(b1_id).unwrap(); // split block with block_id=1

        // block 0 完全不受影响
        assert_eq!(gbt.block_indexs[0].block_id, b0_id);
        assert_eq!(gbt.block_indexs[0].block_size, b0_size);
        assert_eq!(gbt.block_indexs[0].file_start, b0_file_start);
        assert_eq!(gbt.block_indexs[0].line_count, b0_line_count);
        // 分裂结果在 [1] 和 [2]：A 保持 block_id=b1_id，B 获得新 id
        assert_eq!(gbt.block_indexs[1].block_id, b1_id, "A 的 block_id 保持");
        assert_ne!(gbt.block_indexs[2].block_id, b1_id, "B 有新 block_id");
        // 原 block 2 现在在 [3]，block_id 保持稳定
        assert_eq!(
            gbt.block_indexs[3].block_id, b2_id,
            "原 block_2 block_id 不变"
        );
    }

    // ---------- 3. lines_index ----------

    /// A 的所有行 block_end <= A.block_size；B 的所有行 block_end <= B.block_size
    #[test]
    fn test_split_block_lines_index_within_bounds() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].block_size;
        for l in &gbt.block_indexs[0].lines_index {
            assert!(
                l.block_end <= size_a,
                "A 行 block_end={} > size_a={}",
                l.block_end,
                size_a
            );
        }
        let size_b = gbt.block_indexs[1].block_size;
        for l in &gbt.block_indexs[1].lines_index {
            assert!(
                l.block_end <= size_b,
                "B 行 block_end={} > size_b={}",
                l.block_end,
                size_b
            );
        }
    }

    /// B 的第一行 block_start == 0（偏移已正确调整）
    #[test]
    fn test_split_block_b_lines_start_at_zero() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let first = gbt.block_indexs[1].lines_index.first().unwrap();
        assert_eq!(first.block_start, 0, "B 第一行 block_start 必须为 0");
    }

    /// A/B 各自的 lines_index 内相邻行连续（每行 block_start == 前一行 block_end）
    #[test]
    fn test_split_block_lines_index_contiguous() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        for (label, idx) in [("A", &gbt.block_indexs[0]), ("B", &gbt.block_indexs[1])] {
            for w in idx.lines_index.windows(2) {
                assert_eq!(
                    w[0].block_end, w[1].block_start,
                    "block {} lines_index 不连续: [{}.block_end={} != {}.block_start={}]",
                    label, w[0].index_num, w[0].block_end, w[1].index_num, w[1].block_start
                );
            }
        }
    }

    #[test]
    fn test_split_block_lines_b_index_num_restarts_from_zero() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        for (i, line) in gbt.block_indexs[1].lines_index.iter().enumerate() {
            assert_eq!(
                line.index_num, i,
                "分裂后的 B 块 index_num 应按块内顺序从 0 递增"
            );
        }
    }

    /// lines_index 条目总数之和 == 原始 lines_index.len()
    #[test]
    fn test_split_block_lines_index_count_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_count = gbt.block_indexs[0].lines_index.len();

        gbt.split_block(0).unwrap();

        let count_a = gbt.block_indexs[0].lines_index.len();
        let count_b = gbt.block_indexs[1].lines_index.len();
        assert_eq!(count_a + count_b, orig_count);
    }

    /// line_count 之和 == 原始 line_count（完整行数）
    #[test]
    fn test_split_block_line_count_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig = gbt.block_indexs[0].line_count;

        gbt.split_block(0).unwrap();

        assert_eq!(
            gbt.block_indexs[0].line_count + gbt.block_indexs[1].line_count,
            orig
        );
    }

    /// 分裂在完整行边界：A 最后一行 is_complete=true，且 A 块末尾字节是 '\n'
    #[test]
    fn test_split_block_split_at_newline_boundary() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let last_a = gbt.block_indexs[0].lines_index.last().unwrap();
        assert!(last_a.is_complete, "A 最后一行应为完整行");
        let id_a = gbt.block_indexs[0].block_id;
        let bytes_a = get_block_text(&mut gbt, id_a);
        assert_eq!(bytes_a.last(), Some(&b'\n'), "A 最后一字节应是 \\n");
    }

    /// lines_index 中每条 is_complete 行在块字节中确实以 '\n' 结尾
    #[test]
    fn test_split_block_lines_match_content() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let id_a = gbt.block_indexs[0].block_id;
        let id_b = gbt.block_indexs[1].block_id;
        let bytes_a = get_block_text(&mut gbt, id_a);
        for l in &gbt.block_indexs[0].lines_index {
            if l.is_complete {
                assert_eq!(
                    bytes_a[l.block_start..l.block_end].last(),
                    Some(&b'\n'),
                    "A block 完整行未以 \\n 结尾"
                );
            }
        }
        let bytes_b = get_block_text(&mut gbt, id_b);
        for l in &gbt.block_indexs[1].lines_index {
            if l.is_complete {
                assert_eq!(
                    bytes_b[l.block_start..l.block_end].last(),
                    Some(&b'\n'),
                    "B block 完整行未以 \\n 结尾"
                );
            }
        }
    }

    // ---------- 4. RingVec / cache / is_modified ----------

    /// 分裂后 RingVec 同时包含 block A（block_id=0）和 block B（新 block_id）
    #[test]
    fn test_split_block_ringvec_contains_both_halves() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let id_a = gbt.block_indexs[0].block_id;

        gbt.split_block(id_a).unwrap();

        let id_b = gbt.block_indexs[1].block_id;
        let ids: Vec<BlockId> = gbt.blocks.iter().map(|b| b.block_id).collect();
        assert!(
            ids.contains(&id_a),
            "RingVec 应包含 block A（block_id={}）",
            id_a
        );
        assert!(
            ids.contains(&id_b),
            "RingVec 应包含 block B（block_id={}）",
            id_b
        );
    }

    /// 分裂后 A 和 B 均标记 is_modified=true
    #[test]
    fn test_split_block_both_marked_modified() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let id_a = gbt.block_indexs[0].block_id;

        gbt.split_block(id_a).unwrap();

        let id_b = gbt.block_indexs[1].block_id;
        assert!(
            gbt.blocks
                .iter()
                .find(|b| b.block_id == id_a)
                .unwrap()
                .is_modified,
            "A 应标记 is_modified"
        );
        assert!(
            gbt.blocks
                .iter()
                .find(|b| b.block_id == id_b)
                .unwrap()
                .is_modified,
            "B 应标记 is_modified"
        );
    }

    /// split_block 后 cache 中的已修改块以 block_id 为 key，block_id 保持稳定不重新编号
    #[test]
    fn test_split_block_cache_block_id_stable() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);

        // 手动向 cache 插入一个 block_id=2 的块（模拟已修改并被驱逐的 block）
        let fake_block_id: BlockId = 2;
        let fake_file_start = gbt.block_indexs[2].file_start;
        gbt.cache.insert(
            fake_block_id,
            Block {
                data: GapBuffer::from_bytes(b"fake\n", CHAR_GAP_SIZE),
                file_start: fake_file_start,
                file_end: fake_file_start + 5,
                block_id: fake_block_id,
                is_modified: true,
            },
        );

        gbt.split_block(0).unwrap();

        // block_id=2 的 cache 条目以 key=2 保持，block_id 不变（不重新编号）
        let cached = gbt.cache.get(&fake_block_id).unwrap();
        assert_eq!(
            cached.block_id, fake_block_id,
            "cache 中 block_id 应保持稳定"
        );
    }

    /// split_block 后其他已修改缓存块的 block_id 也不受影响
    #[test]
    fn test_split_block_cache_lower_block_id_unchanged() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);

        // 插入一个 block_id=1 的缓存块
        let fake_block_id: BlockId = 1;
        let fake_file_start = gbt.block_indexs[1].file_start;
        gbt.cache.insert(
            fake_block_id,
            Block {
                data: GapBuffer::from_bytes(b"old\n", CHAR_GAP_SIZE),
                file_start: fake_file_start,
                file_end: fake_file_start + 4,
                block_id: fake_block_id,
                is_modified: true,
            },
        );

        gbt.split_block(0).unwrap();

        // block_id=1 的缓存块不受影响
        let cached = gbt.cache.get(&fake_block_id).unwrap();
        assert_eq!(
            cached.block_id, fake_block_id,
            "block_id=1 的缓存块不受影响"
        );
    }
}
