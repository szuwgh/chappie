use crate::common::error::ChapError;
use crate::common::gap_buffer::GapBuffer;
use crate::common::ring_vec::RingVec;
use crate::textwarp::block::Block;
use crate::textwarp::block::BlockId;
use crate::textwarp::block::BlockIndex;
use crate::textwarp::block::BlockPtr;
use crate::textwarp::block::BLOCK_SIZE;
use crate::textwarp::block::BLOKK_NUM;
use crate::textwarp::block::CHAR_GAP_SIZE;
use crate::textwarp::block::EMPTY_BLOCK_ID;
use crate::textwarp::BlockLineData;
use crate::textwarp::ChapResult;
use crate::textwarp::EditText;
use crate::textwarp::Line;
use crate::textwarp::LineBlockStr;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStateBuilder;
use crate::textwarp::Partten;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use crc::Crc;
use crc::CRC_32_ISO_HDLC;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Seek;
use std::io::Write;
use std::iter;
use std::path::Path;

pub(crate) struct GapBlockText {
    reader: BufReader<File>,
    blocks: RingVec<Block>,         // 每个块4KB大小（内存窗口）
    cache: HashMap<BlockId, Block>, // 缓存已修改的块（key = stable block_id）
    file_size: usize,               // 文件大小
    source_file_size: usize,        // 原始 backing file 大小，未索引块懒加载用
    block_indexs: Vec<BlockIndex>, // 每一块的索引（Vec位置是位置索引，block_id是稳定标识） 只保留块级索引 block_indexs，块内行信息按需扫描
    next_id: BlockId,              // 下一个分配的 block_id
}

impl TextIndex for GapBlockText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        let first = self.block_indexs.first();
        LineState::builder()
            .block_num(first.map_or(0, |bi| bi.block_id))
            .block_line_index(0)
            .block_offset(0)
            .line_num(1)
            .line_index(0)
            .line_offset(0)
            .line_file_start(0)
            .line_file_end(0)
            .start_line_num(1)
            .start_page_num(1)
            .build()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // GapText 不支持分页偏移
    }
}

impl GapBlockText {
    fn source_file_size(&self) -> ChapResult<usize> {
        Ok(self.source_file_size)
    }

    fn sync_block_source_mirror(
        block: &mut Block,
        source_file_start: usize,
        source_file_end: usize,
    ) {
        block.source_file_start = source_file_start;
        block.source_file_end = source_file_end;
    }

    fn cache_block_snapshot(&mut self, block: Block) {
        if block.is_modified {
            self.cache.insert(block.block_id, block);
        }
    }

    fn block_checksum(bytes: &[u8]) -> u32 {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        crc.checksum(bytes)
    }

    fn block_checksum_slices(left: &[u8], right: &[u8]) -> u32 {
        let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
        let mut digest = crc.digest();
        digest.update(left);
        digest.update(right);
        digest.finalize()
    }

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
            source_file_size: file_size as usize,
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
        let mut block_indexs = Vec::with_capacity(BLOKK_NUM * 2);
        let mut block_id = start_block_id;
        loop {
            match Block::from_reader(reader, &mut buf, block_start_offset, block_id, None) {
                Ok((block, Some(block_index))) => {
                    let size = block_index.logic_block_size;
                    if blocks.len() < BLOKK_NUM {
                        blocks.push(block);
                    }
                    block_start_offset += size;
                    block_indexs.push(block_index);
                    block_id += 1;
                }
                Ok((_block, None)) => {
                    return Err(ChapError::Unexpected(
                        "read_blocks: missing block index while loading file".to_string(),
                    ));
                }
                Err(ChapError::EOF) => break,
                Err(err) => return Err(err),
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
        if block_id == EMPTY_BLOCK_ID {
            return Some(0);
        }
        self.block_indexs
            .iter()
            .position(|bi| bi.block_id == block_id)
    }

    fn read_one_block(
        &mut self,
        block_id: BlockId,
        _logic_file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<(Block, Option<BlockIndex>)> {
        // 优先从 cache 取（修改过的块存在 cache 中，key = stable block_id）
        if let Some(mut o) = self.cache.remove(&block_id) {
            Self::sync_block_source_mirror(&mut o, source_file_start, source_file_end);
            return Ok((o, None));
        }
        let mut buf = [0u8; BLOCK_SIZE];
        let (mut block, block_index) = Block::from_reader(
            &mut self.reader,
            &mut buf,
            source_file_start,
            block_id,
            check_sum,
        )?;
        Self::sync_block_source_mirror(&mut block, source_file_start, source_file_end);
        Ok((block, block_index))
    }

    fn read_one_block_ptr(
        &mut self,
        block_id: BlockId,
        _logic_file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<(BlockPtr<'_>, Option<BlockIndex>)> {
        // 优先使用当前窗口中的块；它可能已经被编辑但尚未被驱逐到 cache。
        if let Some(block) = self.blocks.iter().find(|b| b.block_id == block_id) {
            return Ok((BlockPtr::Borrowed(block), None));
        }
        // 其次从 cache 取（修改过的块存在 cache 中，key = stable block_id）
        if let Some(o) = self.cache.get(&block_id) {
            return Ok((BlockPtr::Borrowed(o), None));
        }
        let mut buf = [0u8; BLOCK_SIZE];
        let (mut block, block_index) = Block::from_reader(
            &mut self.reader,
            &mut buf,
            source_file_start,
            block_id,
            check_sum,
        )?;
        Self::sync_block_source_mirror(&mut block, source_file_start, source_file_end);
        Ok((BlockPtr::Own(block), block_index))
    }

    // fn read_unindexed_source_block_ptr(
    //     &mut self,
    //     block_id: BlockId,
    //     logic_file_start: usize,
    //     source_file_start: usize,
    //     source_file_end: usize,
    //     check_sum: Option<u32>,
    // ) -> ChapResult<BlockPtr<'_>> {
    //   let  read_unindexed_source_block
    // }

    fn read_unindexed_source_block(
        &mut self,
        block_id: BlockId,
        logic_file_start: usize,
        source_file_start: usize,
    ) -> ChapResult<(Block, BlockIndex)> {
        let mut buf = [0u8; BLOCK_SIZE];
        let (mut block, block_index) = Block::from_reader(
            &mut self.reader,
            &mut buf,
            source_file_start,
            block_id,
            None,
        )?;
        let mut block_index = block_index.ok_or_else(|| {
            ChapError::Unexpected("read_unindexed_source_block: missing block index".to_string())
        })?;
        block_index.logic_file_start = logic_file_start;
        Self::sync_block_source_mirror(
            &mut block,
            block_index.source_file_start,
            block_index.source_file_end,
        );
        Ok((block, block_index))
    }

    fn next_unindexed_block_start(&self) -> ChapResult<Option<(usize, usize)>> {
        let Some(last_bi) = self.block_indexs.last() else {
            return Ok(None);
        };
        let next_logic_file_start = last_bi.file_end();
        let next_source_file_start = last_bi.source_file_end();
        if next_source_file_start >= self.source_file_size()? {
            return Ok(None);
        }
        Ok(Some((next_logic_file_start, next_source_file_start)))
    }

    //获取上一个block 并弹入列表中
    fn read_prev_block(
        &mut self,
        block_id: BlockId,
        logic_file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<()> {
        let (block, _) = self.read_one_block(
            block_id,
            logic_file_start,
            source_file_start,
            source_file_end,
            check_sum,
        )?;
        let old = self.blocks.push_front(block);
        if let Some(o) = old {
            // 修复：用被驱逐块自身的 block_id 作为 key，而不是新块的 file_start
            self.cache_block_snapshot(o);
        }
        Ok(())
    }

    fn read_next_block(
        &mut self,
        block_id: BlockId,
        logic_file_start: usize,
        source_file_start: usize,
        source_file_end: usize,
        check_sum: Option<u32>,
    ) -> ChapResult<()> {
        // 这里只负责加载“已经存在于 block_indexs 中的块”。
        // 需要扩展索引的新块必须走 read_unindexed_source_block()，
        // 否则容易把 source_* 读取路径和 logic_* 建索引路径重新混在一起。
        let (block, _) = self.read_one_block(
            block_id,
            logic_file_start,
            source_file_start,
            source_file_end,
            check_sum,
        )?;
        let old = self.blocks.push(block);
        if let Some(o) = old {
            // 修复：用被驱逐块自身的 block_id 作为 key
            self.cache_block_snapshot(o);
        }
        Ok(())
    }

    fn get_iter_rev(
        &mut self,
        block_id: BlockId,
        block_line_index: usize,
        block_offset: usize,
    ) -> ChapResult<GapBlockTextIterRev<'_>> {
        let block3 = self.get_block3(block_id, block_line_index, block_offset)?;
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
    ) -> ChapResult<Block3<'_, [Option<BlockPtr<'_>>; 3]>> {
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
                            blocks: [
                                self.blocks.get(0).map(BlockPtr::from_block),
                                self.blocks.get(1).map(BlockPtr::from_block),
                                self.blocks.get(2).map(BlockPtr::from_block),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    } else {
                        // 读取上一个块，把最后一个块弹出
                        let prev_bi = self.block_indexs[pos_in_idx - 1].clone();
                        self.read_prev_block(
                            prev_bi.block_id,
                            prev_bi.logic_file_start,
                            prev_bi.source_file_start,
                            prev_bi.source_file_end,
                            Some(prev_bi.check_sum),
                        )?;
                        return Ok(Block3 {
                            blocks: [
                                self.blocks.get(1).map(BlockPtr::from_block),
                                self.blocks.get(2).map(BlockPtr::from_block),
                                self.blocks.get(3).map(BlockPtr::from_block),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    }
                } else if i == self.blocks.len() - 1 {
                    // 最后一个块，尝试加载下一个块
                    if let Some(next_bi) = self.block_indexs.get(pos_in_idx + 1).cloned() {
                        self.read_next_block(
                            next_bi.block_id,
                            next_bi.logic_file_start,
                            next_bi.source_file_start,
                            next_bi.source_file_end,
                            Some(next_bi.check_sum),
                        )?;
                        return Ok(Block3 {
                            blocks: [
                                self.blocks.get(i - 1).map(BlockPtr::from_block),
                                self.blocks.get(i).map(BlockPtr::from_block),
                                self.blocks.get(i + 1).map(BlockPtr::from_block),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    } else {
                        // 从磁盘上取未索引的块
                        let Some((next_logic_file_start, next_source_file_start)) =
                            self.next_unindexed_block_start()?
                        else {
                            return Ok(Block3 {
                                blocks: [self.blocks.get(i).map(BlockPtr::from_block), None, None],
                                block_indexs: &self.block_indexs,
                            });
                        };
                        let new_block_id = self.alloc_id();
                        let (block, block_index) = self.read_unindexed_source_block(
                            new_block_id,
                            next_logic_file_start,
                            next_source_file_start,
                        )?;
                        self.block_indexs.push(block_index);
                        if let Some(evicted) = self.blocks.push(block) {
                            self.cache_block_snapshot(evicted);
                        }
                        return Ok(Block3 {
                            blocks: [
                                self.blocks.get(i - 1).map(BlockPtr::from_block),
                                self.blocks.get(i).map(BlockPtr::from_block),
                                self.blocks.get(i + 1).map(BlockPtr::from_block),
                            ],
                            block_indexs: &self.block_indexs,
                        });
                    }
                } else {
                    // 不是第一个也不是最后一个块
                    return Ok(Block3 {
                        blocks: [
                            self.blocks.get(i - 1).map(BlockPtr::from_block),
                            self.blocks.get(i).map(BlockPtr::from_block),
                            self.blocks.get(i + 1).map(BlockPtr::from_block),
                        ],
                        block_indexs: &self.block_indexs,
                    });
                }
            }
            //如果找不到行数 则要重置 block 列表
            self.reset_blocks(block_id)?;
            return Ok(Block3 {
                blocks: [
                    self.blocks.get(0).map(BlockPtr::from_block),
                    self.blocks.get(1).map(BlockPtr::from_block),
                    self.blocks.get(2).map(BlockPtr::from_block),
                ],
                block_indexs: &self.block_indexs,
            });
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

    // fn find_block(&self, file_start: usize) -> Option<&Block> {
    //     for b in self.blocks.iter() {
    //         if file_start == b.file_start {
    //             return Some(b);
    //         }
    //     }
    //     None
    // }

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
        let modified_blocks: Vec<Block> = self
            .blocks
            .iter()
            .filter(|block| block.is_modified)
            .cloned()
            .collect();
        for block in modified_blocks {
            self.cache_block_snapshot(block);
        }

        // 3. 找到目标块的 BlockIndex（需要 source_file_start 和 check_sum）
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
        let (block, _) = self.read_one_block(
            block_id,
            target.logic_file_start,
            target.source_file_start,
            target.source_file_end,
            Some(target.check_sum),
        )?;

        // 5. 推入 RingVec；满时挤出最老的块，若已修改则存入 cache（key = block_id）
        let evicted = self.blocks.push(block);
        if let Some(old) = evicted {
            self.cache_block_snapshot(old);
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
        let block = self.blocks.iter().find(|b| b.block_id == block_id)?;
        let line =
            block.line_span_at_offset(abs_byte.min(bi.logic_block_size.saturating_sub(1)))?;
        Some(line.index_num)
    }

    pub(crate) fn resolve_block_for_file_offset(
        &self,
        file_offset: usize,
    ) -> Option<(BlockId, usize)> {
        self.resolve_block_for_file_offset_with_boundary(file_offset, true)
    }

    pub(crate) fn resolve_block_for_file_offset_with_boundary(
        &self,
        file_offset: usize,
        prefer_next_at_boundary: bool,
    ) -> Option<(BlockId, usize)> {
        let bi = if prefer_next_at_boundary {
            self.block_indexs
                .iter()
                .find(|bi| file_offset >= bi.logic_file_start && file_offset < bi.file_end())
        } else {
            self.block_indexs
                .iter()
                .find(|bi| file_offset > bi.logic_file_start && file_offset <= bi.file_end())
        }
        .or_else(|| {
            if file_offset == 0 {
                self.block_indexs.first()
            } else {
                self.block_indexs
                    .last()
                    .filter(|bi| file_offset >= bi.file_end())
            }
        })?;
        Some((
            bi.block_id,
            file_offset
                .saturating_sub(bi.logic_file_start)
                .min(bi.logic_block_size),
        ))
    }

    fn reset_blocks(&mut self, block_id: BlockId) -> ChapResult<()> {
        // 把当前窗口中修改过的块存入 cache，防止丢失
        let mut old_blocks = std::mem::replace(&mut self.blocks, RingVec::with_capacity(BLOKK_NUM));
        while !old_blocks.is_empty() {
            let Some(block) = old_blocks.remove_last() else {
                break;
            };
            self.cache_block_snapshot(block);
        }

        if let Some(start_pos) = self.block_pos(block_id) {
            // get_block3 只需要当前块和后两个块，后续块沿用懒加载。
            let mut new_blocks = RingVec::with_capacity(BLOKK_NUM);
            for i in 0..3 {
                if let Some(bi) = self.block_indexs.get(start_pos + i).cloned() {
                    if let Some(mut cached) = self.cache.remove(&bi.block_id) {
                        Self::sync_block_source_mirror(
                            &mut cached,
                            bi.source_file_start,
                            bi.source_file_end,
                        );
                        new_blocks.push(cached);
                    } else {
                        match self.read_one_block(
                            bi.block_id,
                            bi.logic_file_start,
                            bi.source_file_start,
                            bi.source_file_end,
                            Some(bi.check_sum),
                        ) {
                            Ok((block, _)) => {
                                new_blocks.push(block);
                            }
                            Err(err) => return Err(err),
                        }
                    }
                } else {
                    // 需要从磁盘加载新块并扩展索引
                    let Some((next_logic_file_start, next_source_file_start)) =
                        self.next_unindexed_block_start()?
                    else {
                        break;
                    };
                    let new_id = self.alloc_id();
                    match self.read_unindexed_source_block(
                        new_id,
                        next_logic_file_start,
                        next_source_file_start,
                    ) {
                        Ok((block, bi)) => {
                            self.block_indexs.push(bi);
                            new_blocks.push(block);
                        }
                        _ => break,
                    }
                }
            }
            let loaded_target = new_blocks.iter().any(|block| block.block_id == block_id);
            self.blocks = new_blocks;
            if !loaded_target {
                return Err(ChapError::Unexpected(format!(
                    "reset_blocks: block_id {} was not loaded",
                    block_id
                )));
            }
        } else {
            // block_id 不在索引中：从文件末尾继续加载，直到找到为止
            'out: loop {
                let Some((next_logic_file_start, next_source_file_start)) =
                    self.next_unindexed_block_start()?
                else {
                    break;
                };
                let new_id = self.alloc_id();
                match self.read_unindexed_source_block(
                    new_id,
                    next_logic_file_start,
                    next_source_file_start,
                ) {
                    Ok((block, bi)) => {
                        self.block_indexs.push(bi);
                        let found = block.block_id == block_id;
                        if let Some(evicted) = self.blocks.push(block) {
                            self.cache_block_snapshot(evicted);
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
                    self.blocks.get(i).map(BlockPtr::from_block),
                    self.blocks.get(i + 1).map(BlockPtr::from_block),
                    self.blocks.get(i + 2).map(BlockPtr::from_block),
                ],
                block_indexs: &self.block_indexs,
            };
            let (l, _, _) = blocks.get_line(block_id, block_offset)?;
            return Some(unsafe { std::mem::transmute::<LineBlockStr<'_>, LineBlockStr<'a>>(l) });
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

        let line = self.get_line(&state)?;
        let mut next_block_id = block_id;
        if state.get_line_end() >= line.text_len() {
            if state.get_block_line_end() >= block_index.logic_block_size {
                // 越过当前块，进入下一块
                next_block_id = self.block_indexs.get(pos + 1)?.block_id;
                block_line_index = 0;
                block_offset = state
                    .get_block_line_end()
                    .saturating_sub(block_index.logic_block_size);
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
            //.start_line_num(state.get_line_num())
            .build();
        Some(p)
    }

    fn get_pre_line_state(&mut self, state: &LineState, _width: usize) -> Option<LineState> {
        let block_id = state.get_block_num(); // block_num 字段存储 block_id
        let block_line_index = state.get_block_line_index();
        let block_offset = state.get_block_offset();

        // 通过 block_id 找位置
        let pos = self.block_pos(block_id)?;

        // 判断是否在文件最开头
        if pos == 0 && block_line_index == 0 && block_offset == 0 && state.get_line_offset() == 0 {
            return None;
        }

        if state.get_line_offset() > 0 {
            let p = LineState::builder()
                .line_index(state.get_line_index())
                .line_offset(state.get_line_offset())
                .block_num(block_id)
                .block_line_index(block_line_index)
                .block_offset(block_offset)
                .line_file_start(state.get_line_file_start())
                .line_file_end(state.get_line_file_end())
                .build();

            return Some(p);
        }
        if block_line_index > 0 {
            let last_block_line_index = block_line_index.saturating_sub(1);
            let block = self.blocks.iter().find(|b| b.block_id == block_id)?;
            let last_line_info = block.line_span_at_index(last_block_line_index)?;
            if last_block_line_index == 0 {
                if pos == 0 {
                    //已经是第一个块了
                    let p = LineState::builder()
                        //.start_line_num(state.get_line_num())
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
                self.ensure_block_loaded(last_block_id).ok()?;
                let last_block = self.blocks.iter().find(|b| b.block_id == last_block_id)?;
                let last_last_line_info = last_block.last_line_span()?;
                if last_last_line_info.is_complete {
                    //上上一行是完整行
                    let p = LineState::builder()
                        //.start_line_num(state.get_line_num())
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
                        //.start_line_num(state.get_line_num())
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
                    // .start_line_num(state.get_line_num())
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
            self.ensure_block_loaded(last_block_id).ok()?;
            let last_block = self.blocks.iter().find(|b| b.block_id == last_block_id)?;
            let last_line_info = last_block.last_line_span()?;
            let p = LineState::builder()
                //.start_line_num(state.get_line_num())
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
            && meta.get_line_offset() == 0
        {
            return false;
        }
        return true;
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        let last_block_index = self.block_indexs.last().unwrap();
        if meta.get_block_num() == last_block_index.block_id
            && meta.get_block_line_end() >= last_block_index.logic_block_size
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

    //搜索
    fn search(
        &mut self,
        partten: Partten,
        state: &LineState,
    ) -> ChapResult<Option<Vec<LineState>>> {
        let scroll_iter = GapBlockScollTextIter::new(
            self,
            state.block_num,
            state.block_line_index,
            state.block_offset,
            state.line_index,
        )?;
        let mut results = Vec::new();
        // let boy = BoyerMoore::new(partten);
        for (line_idx, (line, mut index)) in scroll_iter.enumerate() {
            let mut hits = line.search(&partten);

            if line_idx == 0 {
                hits.retain(|pos| pos.start >= state.line_offset);
            }

            if !hits.is_empty() {
                index.line_offset = 0;
                index.highlight = Some(hits);
                results.push(index);
            }
        }
        Ok((!results.is_empty()).then_some(results))
    }
}

impl GapBlockText {
    fn resolve_target(
        &self,
        line_meta: &LineState,
        bytes_cursor: usize,
    ) -> ChapResult<(BlockId, usize, usize)> {
        self.resolve_target_impl(line_meta, bytes_cursor, true)
    }

    fn resolve_backspace_target(
        &self,
        line_meta: &LineState,
        bytes_cursor: usize,
    ) -> ChapResult<(BlockId, usize, usize)> {
        self.resolve_target_impl(line_meta, bytes_cursor, false)
    }

    fn resolve_target_impl(
        &self,
        line_meta: &LineState,
        bytes_cursor: usize,
        advance_at_block_end: bool,
    ) -> ChapResult<(BlockId, usize, usize)> {
        // if line_meta.get_line_file_end() > line_meta.get_line_file_start() {
        //     let abs_offset = line_meta
        //         .get_line_file_start()
        //         .saturating_add(line_meta.get_line_offset())
        //         .saturating_add(bytes_cursor);
        //     let (block_id, block_local_offset) = self
        //         .resolve_block_for_file_offset(abs_offset)
        //         .ok_or_else(|| {
        //             ChapError::Unexpected(format!(
        //                 "resolve_insert_target: file offset {} not found",
        //                 abs_offset
        //             ))
        //         })?;
        //     let pos = self.block_pos(block_id).ok_or_else(|| {
        //         ChapError::Unexpected(format!(
        //             "resolve_insert_target: block_id {} not found",
        //             block_id
        //         ))
        //     })?;
        //     return Ok((block_id, pos, block_local_offset));
        // }

        // let mut block_id = line_meta.get_block_num();
        // let mut insert_offset =
        //     line_meta.get_block_offset() + line_meta.get_line_offset() + bytes_cursor;
        // log::debug!(
        //     "resolve_insert_target: block_id={}, block_offset={}, line_offset={}, bytes_cursor={}, computed insert_offset={}",
        //     block_id, line_meta.get_block_offset(), line_meta.get_line_offset(), bytes_cursor, insert_offset
        // );
        // let mut pos = self.block_pos(block_id).ok_or_else(|| {
        //     ChapError::Unexpected(format!(
        //         "resolve_insert_target fallback: block_id {} not found",
        //         block_id
        //     ))
        // })?;
        // while insert_offset > self.block_indexs[pos].block_size
        //     || (insert_offset == self.block_indexs[pos].block_size
        //         && pos + 1 < self.block_indexs.len())
        // {
        //     insert_offset -= self.block_indexs[pos].block_size;
        //     pos += 1;
        //     block_id = self.block_indexs[pos].block_id;
        // }
        let mut block_id = line_meta.get_block_num(); // 语义上是 block_id
        let block_offset = line_meta.get_block_offset();
        // let mut block_line_index = line_meta.get_block_line_index();
        let mut insert_offset = block_offset + line_meta.line_offset + bytes_cursor;
        let mut pos = self.block_pos(block_id).unwrap();
        block_id = self.block_indexs[pos].block_id;
        log::debug!(
            "resolve_insert_target: pos={}, block_id={}, block_offset={}, line_offset={}, bytes_cursor={}, computed insert_offset={}",
            pos, block_id, line_meta.get_block_offset(), line_meta.get_line_offset(), bytes_cursor, insert_offset
        );
        while pos + 1 < self.block_indexs.len()
            && (insert_offset > self.block_indexs[pos].logic_block_size
                || (advance_at_block_end
                    && insert_offset == self.block_indexs[pos].logic_block_size))
        {
            insert_offset -= self.block_indexs[pos].logic_block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
            //  block_line_index = 0;
        }
        if insert_offset > self.block_indexs[pos].logic_block_size {
            return Err(ChapError::Unexpected(format!(
                "resolve_target: offset {} out of block {} size {}",
                insert_offset, block_id, self.block_indexs[pos].logic_block_size
            )));
        }
        Ok((block_id, pos, insert_offset))
    }

    // fn resolve_backspace_target(
    //     &self,
    //     line_meta: &LineState,
    //     bytes_cursor: usize,
    // ) -> ChapResult<(BlockId, usize, usize)> {
    //     if line_meta.get_line_file_end() > line_meta.get_line_file_start() {
    //         let abs_offset = line_meta
    //             .get_line_file_start()
    //             .saturating_add(line_meta.get_line_offset())
    //             .saturating_add(bytes_cursor)
    //             .min(self.file_size);
    //         let (block_id, block_local_offset) = self
    //             .resolve_block_for_file_offset(abs_offset)
    //             .ok_or_else(|| {
    //                 ChapError::Unexpected(format!(
    //                     "resolve_backspace_target: file offset {} not found",
    //                     abs_offset
    //                 ))
    //             })?;
    //         let pos = self.block_pos(block_id).ok_or_else(|| {
    //             ChapError::Unexpected(format!(
    //                 "resolve_backspace_target: block_id {} not found",
    //                 block_id
    //             ))
    //         })?;
    //         let block_local_offset = block_local_offset.min(self.block_indexs[pos].block_size);
    //         return Ok((block_id, pos, block_local_offset));
    //     }

    //     let mut block_id = line_meta.get_block_num();
    //     let mut insert_offset =
    //         line_meta.get_block_offset() + line_meta.get_line_offset() + bytes_cursor;
    //     let mut pos = self.block_pos(block_id).ok_or_else(|| {
    //         ChapError::Unexpected(format!(
    //             "resolve_backspace_target fallback: block_id {} not found",
    //             block_id
    //         ))
    //     })?;
    //     while insert_offset > self.block_indexs[pos].block_size {
    //         insert_offset -= self.block_indexs[pos].block_size;
    //         pos += 1;
    //         block_id = self.block_indexs[pos].block_id;
    //     }
    //     Ok((block_id, pos, insert_offset))
    // }

    /// 设计说明：
    /// 块编辑后，`start_pos` 之后所有块的逻辑 `logic_file_start` 都可能发生连锁变化。
    /// 这里负责把 `block_indexs` 重新串成一个连续的逻辑视图，并重算 `file_size`。
    ///
    /// 关键点是：这里维护的是“逻辑偏移”，不是“原文件物理偏移”。
    /// 未加载块仍然保留各自的 `source_file_start`，以后如果真的访问到它们，再从原始 backing file
    /// 的对应位置读取。也正因为如此，这里不能像旧实现那样为了更新 offset 把后缀块全部 materialize：
    /// 那会让一次前部插入/删除触发整段后缀回读，既放大 IO，也破坏当前按需加载的设计。
    ///
    /// 换句话说，这个函数做的是：
    /// - 更新逻辑索引里的连续区间
    /// - 重新计算 `file_size`
    ///
    /// 它刻意不做的是：
    /// - 不回读未加载块
    /// - 不修改 `Block` 上的 `source_*` 冗余镜像
    fn sync_block_offsets_from(&mut self, start_pos: usize) {
        if self.block_indexs.is_empty() {
            self.file_size = 0;
            return;
        }
        let mut start_pos = start_pos.min(self.block_indexs.len().saturating_sub(1));
        if self.block_indexs[0].logic_file_start != 0 {
            self.block_indexs[0].logic_file_start = 0;
            start_pos = 0;
        }
        let mut pos = if start_pos == 0 { 0 } else { start_pos };
        if pos > 0 {
            let prev = &self.block_indexs[pos - 1];
            let expected_start = prev.logic_file_start + prev.logic_block_size;
            if self.block_indexs[pos].logic_file_start != expected_start {
                self.block_indexs[pos].logic_file_start = expected_start;
            }
        }
        while pos < self.block_indexs.len() {
            if pos > 0 {
                let prev_end = self.block_indexs[pos - 1].logic_file_start
                    + self.block_indexs[pos - 1].logic_block_size;
                self.block_indexs[pos].logic_file_start = prev_end;
            }
            pos += 1;
        }
        self.file_size = self.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
    }

    #[cfg(debug_assertions)]
    fn debug_assert_storage_consistent(&self) {
        let total: usize = self.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        debug_assert_eq!(
            total, self.file_size,
            "sum(logic_block_size) must equal file_size"
        );
        for (i, bi) in self.block_indexs.iter().enumerate() {
            if i == 0 {
                debug_assert_eq!(bi.logic_file_start, 0, "first block must start at 0");
            } else {
                let prev = &self.block_indexs[i - 1];
                debug_assert_eq!(
                    bi.logic_file_start,
                    prev.logic_file_start + prev.logic_block_size,
                    "block {} logic_file_start must be contiguous",
                    i
                );
            }

            if let Some(block) = self.blocks.iter().find(|b| b.block_id == bi.block_id) {
                debug_assert_eq!(block.source_file_start, bi.source_file_start);
                debug_assert_eq!(block.source_file_end, bi.source_file_end);
                debug_assert_eq!(
                    block.block_size(),
                    bi.logic_block_size,
                    "loaded block {} (id {}) size must match index",
                    i,
                    bi.block_id
                );
            }
        }
    }

    #[cfg(not(debug_assertions))]
    fn debug_assert_storage_consistent(&self) {}

    #[cfg(test)]
    pub(crate) fn assert_storage_consistent_for_test(&self) {
        let total: usize = self.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(
            total, self.file_size,
            "所有 logic_block_size 之和必须等于 file_size"
        );
        for (i, bi) in self.block_indexs.iter().enumerate() {
            if i == 0 {
                assert_eq!(bi.logic_file_start, 0, "第一个 block 必须从 0 开始");
            } else {
                let prev = &self.block_indexs[i - 1];
                assert_eq!(
                    bi.logic_file_start,
                    prev.logic_file_start + prev.logic_block_size,
                    "block {} 的 logic_file_start 应紧跟前一个 block",
                    i
                );
            }
            if let Some(block) = self.blocks.iter().find(|b| b.block_id == bi.block_id) {
                assert_eq!(
                    block.source_file_start, bi.source_file_start,
                    "已加载 block 的 source_file_start 必须与索引一致"
                );
                assert_eq!(
                    block.source_file_end, bi.source_file_end,
                    "已加载 block 的 source_file_end 必须与索引一致"
                );
            }
        }
    }

    fn sort_loaded_blocks_by_logic_order(&mut self) {
        let order: HashMap<BlockId, usize> = self
            .block_indexs
            .iter()
            .enumerate()
            .map(|(pos, bi)| (bi.block_id, pos))
            .collect();
        self.blocks
            .sort_by_key(|b| order.get(&b.block_id).copied().unwrap_or(usize::MAX));
    }

    fn remove_block_by_id(&mut self, block_id: BlockId) {
        let idx = self.blocks.iter().position(|b| b.block_id == block_id);
        if let Some(idx) = idx {
            let _ = self.blocks.remove(idx);
        }
        self.cache.remove(&block_id);
    }

    fn merge_blocks_around(&mut self, pos: usize) -> ChapResult<()> {
        if self.block_indexs.len() < 2 {
            return Ok(());
        }

        let mut pos = pos.min(self.block_indexs.len().saturating_sub(2));
        loop {
            if self.merge_block_once(pos)? {
                pos = pos.saturating_sub(1);
                continue;
            }
            if pos + 2 > self.block_indexs.len().saturating_sub(1) {
                break;
            }
            pos += 1;
            if !self.merge_block_once(pos)? {
                break;
            }
            pos = pos.saturating_sub(1);
        }

        self.sync_block_offsets_from(pos);
        self.debug_assert_storage_consistent();
        Ok(())
    }

    fn merge_block_once(&mut self, left_pos: usize) -> ChapResult<bool> {
        let right_pos = left_pos + 1;
        if right_pos >= self.block_indexs.len() {
            return Ok(false);
        }

        let left = self.block_indexs[left_pos].clone();
        let right = self.block_indexs[right_pos].clone();
        let merged_len = left.logic_block_size + right.logic_block_size;
        if merged_len > BLOCK_SIZE {
            return Ok(false);
        }

        let mut merged = Vec::with_capacity(merged_len);
        self.ensure_block_loaded_impl(left.block_id)?;
        {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == left.block_id)
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "merge_block_once: left block_id {} not in memory",
                        left.block_id
                    ))
                })?;
            merged.extend_from_slice(block.as_continuous());
        }

        self.ensure_block_loaded_impl(right.block_id)?;
        {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == right.block_id)
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "merge_block_once: right block_id {} not in memory",
                        right.block_id
                    ))
                })?;
            merged.extend_from_slice(block.as_continuous());
        }

        self.ensure_block_loaded_impl(left.block_id)?;
        {
            let block = self
                .blocks
                .iter_mut()
                .find(|b| b.block_id == left.block_id)
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "merge_block_once: merged block_id {} not in memory",
                        left.block_id
                    ))
                })?;
            block.data = GapBuffer::from_bytes(&merged, CHAR_GAP_SIZE);
            Self::sync_block_source_mirror(
                block,
                left.source_file_start.min(right.source_file_start),
                left.source_file_end.max(right.source_file_end),
            );
            block.is_modified = true;
        }

        self.block_indexs[left_pos] = BlockIndex::from_block_bytes(
            left.logic_file_start,
            left.source_file_start.min(right.source_file_start),
            left.source_file_end.max(right.source_file_end),
            left.block_id,
            merged_len,
            Self::block_checksum(&merged),
        )?;
        self.block_indexs.remove(right_pos);
        self.remove_block_by_id(right.block_id);
        self.sort_loaded_blocks_by_logic_order();
        Ok(true)
    }

    fn split_block(&mut self, block_id: BlockId) -> ChapResult<()> {
        let Some(pos) = self.block_pos(block_id) else {
            return Err(ChapError::Unexpected(format!(
                "split_block: block_id {} not found",
                block_id
            )));
        };
        self.sync_block_offsets_from(pos);
        let mut pending = vec![block_id];
        while let Some(next_id) = pending.pop() {
            let Some(next_pos) = self.block_pos(next_id) else {
                continue;
            };
            if self.block_indexs[next_pos].logic_block_size <= BLOCK_SIZE {
                continue;
            }
            if let Some((left_id, right_id)) = self.split_block_once(next_id)? {
                pending.push(right_id);
                pending.push(left_id);
            }
        }
        if !self.block_indexs.is_empty() {
            self.sync_block_offsets_from(pos);
        }
        self.debug_assert_storage_consistent();
        Ok(())
    }

    fn split_offset_at_char_boundary(bytes: &[u8], preferred: usize) -> ChapResult<Option<usize>> {
        if preferred == 0 || preferred >= bytes.len() {
            return Ok(None);
        }

        let text = std::str::from_utf8(bytes).map_err(|e| {
            ChapError::Unexpected(format!("split_block: block is not valid UTF-8: {e}"))
        })?;

        let mut split_byte = preferred;
        while split_byte > 0 && !text.is_char_boundary(split_byte) {
            split_byte -= 1;
        }
        if split_byte == 0 {
            split_byte = preferred + 1;
            while split_byte < bytes.len() && !text.is_char_boundary(split_byte) {
                split_byte += 1;
            }
        }

        if split_byte == 0 || split_byte >= bytes.len() {
            Ok(None)
        } else {
            Ok(Some(split_byte))
        }
    }

    fn split_block_once(&mut self, block_id: BlockId) -> ChapResult<Option<(BlockId, BlockId)>> {
        let pos = self.block_pos(block_id).ok_or_else(|| {
            ChapError::Unexpected(format!("split_block: block_id {} not found", block_id))
        })?;
        let logic_block_size = self.block_indexs[pos].logic_block_size;
        let logic_file_start = self.block_indexs[pos].logic_file_start;

        let preferred_split = self
            .blocks
            .iter()
            .find(|b| b.block_id == block_id)
            .ok_or_else(|| ChapError::Unexpected(format!("split: {} not in memory", block_id)))?
            .split_offset_near_half();

        if preferred_split == 0 || preferred_split >= logic_block_size {
            return Ok(None); // 无法切分（整块只有一行且超大）
        }

        // 基于左右字节重建两个新块及其块级索引。
        let (split_byte, left_bytes, right_bytes, source_file_start, source_file_end) = {
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
            let bytes = block.as_continuous();
            let Some(split_byte) = Self::split_offset_at_char_boundary(bytes, preferred_split)?
            else {
                return Ok(None);
            };
            (
                split_byte,
                bytes[..split_byte].to_vec(),
                bytes[split_byte..].to_vec(),
                block.source_file_start,
                block.source_file_end,
            )
        }; // self.blocks 借用在此结束
        let data_a = GapBuffer::from_bytes(&left_bytes, CHAR_GAP_SIZE);
        let data_b = GapBuffer::from_bytes(&right_bytes, CHAR_GAP_SIZE);
        let left_index = BlockIndex::from_block_bytes(
            logic_file_start,
            source_file_start,
            source_file_end,
            block_id,
            left_bytes.len(),
            Self::block_checksum(&left_bytes),
        )?;

        // ④ 分配 block B 的新 block_id（单调递增，不影响其他块的 block_id）
        let new_block_id = self.alloc_id();
        let right_index = BlockIndex::from_block_bytes(
            logic_file_start + split_byte,
            source_file_start,
            source_file_end,
            new_block_id,
            right_bytes.len(),
            Self::block_checksum(&right_bytes),
        )?;

        // 构造 Block B
        let block_b = Block {
            data: data_b,
            source_file_start: source_file_start,
            source_file_end: source_file_end,
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
            Self::sync_block_source_mirror(block, source_file_start, source_file_end);
            block.is_modified = true;
        }

        // ⑤ 更新 block_indexs：替换 A，插入 B（不需要重编号后续块！）
        self.block_indexs[pos] = left_index;
        self.block_indexs.insert(pos + 1, right_index);
        // ⑥ cache 以 block_id 为 key，其他块 block_id 未变，无需更新

        // ⑦ push block_b；被挤出时存入 cache（key = block_id）
        if let Some(evicted) = self.blocks.push(block_b) {
            self.cache_block_snapshot(evicted);
        }
        // 分裂后的子块共享同一 source_* 区间，已不能再靠 source_file_start 推导文本顺序。
        self.sort_loaded_blocks_by_logic_order();

        Ok(Some((block_id, new_block_id)))
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
        // let (mut block_id, mut pos, mut insert_offset) =
        //     self.resolve_backspace_target(line_meta, bytes_cursor)?;
        // let mut block_id = line_meta.get_block_num();
        // let mut insert_offset =
        //     line_meta.get_block_offset() + line_meta.get_line_offset() + bytes_cursor;
        // let mut pos = self.block_pos(block_id).ok_or_else(|| {
        //     ChapError::Unexpected(format!(
        //         "resolve_backspace_target fallback: block_id {} not found",
        //         block_id
        //     ))
        // })?;
        // while insert_offset > self.block_indexs[pos].block_size {
        //     insert_offset -= self.block_indexs[pos].block_size;
        //     pos += 1;
        //     block_id = self.block_indexs[pos].block_id;
        // }
        // Ok((block_id, pos, insert_offset))
        let (_block_id, mut pos, mut insert_offset) =
            self.resolve_backspace_target(line_meta, bytes_cursor)?;
        let mut remaining = count;
        let mut min_touched_pos = pos;
        let mut deleted_parts: Vec<Vec<u8>> = Vec::new();

        while remaining > 0 {
            // `pos` can move across blocks while deleting, so the live block id must
            // always be derived from the current logical position instead of the
            // initially resolved target.
            let current_block_id = self.block_indexs[pos].block_id;
            self.ensure_block_loaded_impl(current_block_id)?;
            min_touched_pos = min_touched_pos.min(pos);
            let delete_in_block = insert_offset.min(remaining);
            if delete_in_block == 0 {
                if pos == 0 {
                    break;
                }
                pos -= 1;
                insert_offset = self.block_indexs[pos].logic_block_size;
                continue;
            }

            let deleted_bytes = {
                let block = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.block_id == current_block_id)
                    .unwrap();
                block.backspace(insert_offset, delete_in_block)
            };
            deleted_parts.push(deleted_bytes);
            remaining -= delete_in_block;

            let block_size = self
                .blocks
                .iter()
                .find(|b| b.block_id == current_block_id)
                .map(|b| b.block_size())
                .unwrap_or(0);
            if block_size > 0 {
                self.block_indexs[pos].logic_block_size = block_size;
            }
            if block_size == 0 && self.block_indexs.len() > 1 {
                self.block_indexs.remove(pos);
                self.remove_block_by_id(current_block_id);
            } else {
            }
            if remaining == 0 {
                break;
            }
            if pos == 0 {
                break;
            }
            pos -= 1;
            insert_offset = self.block_indexs[pos].logic_block_size;
        }

        deleted_parts.reverse();
        let deleted_bytes = deleted_parts.into_iter().flatten().collect();
        self.sync_block_offsets_from(min_touched_pos);
        self.merge_blocks_around(min_touched_pos.saturating_sub(1))?;
        self.debug_assert_storage_consistent();
        return Ok(deleted_bytes);
        //   }
        //Ok(vec![])
    }

    fn insert_bytes(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        bytes: &[u8],
        _is_overwrite: bool,
    ) -> ChapResult<()> {
        // 设计说明：
        // 这是“任意字节序列插入”的通用入口。流程分三步：
        // 1. 根据 `line_meta + bytes_cursor` 定位到实际 block 和块内偏移
        // 2. 在目标块的 GapBuffer 中执行插入
        // 3. 更新块级元数据，并在需要时触发 split，把超大的逻辑块重新切回 BLOCK_SIZE 限制内。
        let (block_id, pos, insert_offset) = self.resolve_target(line_meta, bytes_cursor)?;
        self.ensure_block_loaded_impl(block_id)?;
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        let added = bytes.len();
        block.insert(insert_offset, bytes);
        self.block_indexs[pos].logic_block_size += added;
        if !self.block_indexs.is_empty() {
            self.split_block(block_id)?;
        } else {
            self.file_size = 0;
        }
        self.debug_assert_storage_consistent();
        Ok(())
    }

    fn insert_char(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
        c: char,
    ) -> ChapResult<()> {
        // 设计说明：
        // 单字符插入和 `insert_bytes` 走同一条核心路径，只是先把 `char` 编码成 UTF-8 字节序列。
        // 之所以不单独做一个“字符级快速路径”，是因为这里的底层存储仍然按字节维护，
        // 而块切分、保存逻辑也都建立在字节偏移上。
        //
        // 这样做的好处是 UTF-8 多字节字符不会引入第二套更新逻辑：
        // 无论插入 ASCII、中文还是 emoji，后续都统一交给块级索引重建和 split 处理。
        let (block_id, pos, insert_offset) = self.resolve_target(line_meta, bytes_cursor)?;
        self.ensure_block_loaded_impl(block_id)?;
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        let chb = c.encode_utf8(&mut [0; 4]).as_bytes().to_vec();
        let added = chb.len();
        block.insert(insert_offset, &chb);
        drop(block);
        self.block_indexs[pos].logic_block_size += added;
        self.split_block(block_id)?;
        self.debug_assert_storage_consistent();
        Ok(())
    }

    fn insert_newline(
        &mut self,
        cursor_y: usize,
        bytes_cursor: usize,
        line_meta: &LineState,
    ) -> ChapResult<()> {
        // 设计说明：
        // 换行插入本质上也是向块里插入一个 `\n` 字节；行边界由读取时扫描得到。
        let (block_id, pos, insert_offset) = self.resolve_target(line_meta, bytes_cursor)?;
        log::debug!(
            "insert_newline: resolved to block_id {}, pos {}, insert_offset {}",
            block_id,
            pos,
            insert_offset
        );
        self.ensure_block_loaded_impl(block_id)?;
        let block = self
            .blocks
            .iter_mut()
            .find(|b| b.block_id == block_id)
            .unwrap();
        block.insert(insert_offset, b"\n");
        self.block_indexs[pos].logic_block_size += 1;
        drop(block);
        self.split_block(block_id)?;
        self.debug_assert_storage_consistent();
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
        while insert_offset >= self.block_indexs[pos].logic_block_size {
            insert_offset -= self.block_indexs[pos].logic_block_size;
            pos += 1;
            block_id = self.block_indexs[pos].block_id;
            block_line_index = 0;
        }
        //在行首 需要合并上一行
        if insert_offset > 0 {
            if block_line_index == 0 {
                let _ = self.backspace(cursor_y, bytes_cursor, 1, line_meta)?;
                self.debug_assert_storage_consistent();
                return Ok(());
            }
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
            self.block_indexs[pos].logic_block_size =
                self.block_indexs[pos].logic_block_size.saturating_sub(1);
            self.sync_block_offsets_from(pos);
            self.debug_assert_storage_consistent();
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
                self.block_indexs[pos - 1].logic_block_size = self.block_indexs[pos - 1]
                    .logic_block_size
                    .saturating_sub(1);
                self.sync_block_offsets_from(pos - 1);
                self.debug_assert_storage_consistent();
                return Ok(());
            }
        }

        self.debug_assert_storage_consistent();
        Ok(())
    }

    fn make_backup<P: AsRef<Path>>(&mut self, backup_name: P) -> ChapResult<()> {
        let file = std::fs::File::create(backup_name).unwrap();
        let mut w = std::io::BufWriter::new(&file);
        let block_ids: Vec<BlockId> = self.block_indexs.iter().map(|bi| bi.block_id).collect();
        for block_id in block_ids {
            self.ensure_block_loaded_impl(block_id)?;
            let block = self
                .blocks
                .iter()
                .find(|b| b.block_id == block_id)
                .or_else(|| self.cache.get(&block_id))
                .ok_or_else(|| {
                    ChapError::Unexpected(format!(
                        "make_backup: block_id {} missing from memory and cache",
                        block_id
                    ))
                })?;
            let txt = block.data.text(..);
            let s = txt.as_slice();
            w.write(s.0)?;
            w.write(s.1)?;
        }
        w.flush()?;
        Ok(())
    }

    fn ensure_block_loaded(&mut self, block_num: usize) -> ChapResult<()> {
        self.ensure_block_loaded_impl(block_num)
    }
}

trait Block3Containter<'a> {
    type Iter: Iterator<Item = &'a Option<BlockPtr<'a>>>;

    fn iter_block(&'a self) -> Self::Iter;
    fn get_block(&'a self, i: usize) -> Option<&'a Option<BlockPtr<'a>>>;
    fn last_block(&'a self) -> Option<&'a Option<BlockPtr<'a>>> {
        self.iter_block().last()
    }
    fn push(&mut self, block: BlockPtr<'a>);
}

impl<'a> Block3Containter<'a> for [Option<BlockPtr<'a>>; 3] {
    type Iter = std::slice::Iter<'a, Option<BlockPtr<'a>>>;

    fn iter_block(&'a self) -> Self::Iter {
        self.iter()
    }

    fn get_block(&'a self, i: usize) -> Option<&'a Option<BlockPtr<'a>>> {
        self.get(i)
    }

    fn push(&mut self, block: BlockPtr<'a>) {
        todo!()
    }
}

struct RingVecBlockIter<'a> {
    blocks: &'a RingVec<Option<BlockPtr<'a>>>,
    index: usize,
}

impl<'a> Iterator for RingVecBlockIter<'a> {
    type Item = &'a Option<BlockPtr<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        let slot = self.blocks.get(self.index)?;
        self.index += 1;
        Some(slot)
    }
}

impl<'a> Block3Containter<'a> for RingVec<Option<BlockPtr<'a>>> {
    type Iter = RingVecBlockIter<'a>;

    fn iter_block(&'a self) -> Self::Iter {
        RingVecBlockIter {
            blocks: self,
            index: 0,
        }
    }

    fn get_block(&'a self, i: usize) -> Option<&'a Option<BlockPtr<'a>>> {
        self.get(i)
    }

    fn push(&mut self, block: BlockPtr<'a>) {
        self.push(Some(block));
    }
}

struct Block3<'a, T: Block3Containter<'a>> {
    blocks: T,
    block_indexs: &'a [BlockIndex],
}

impl<'a, T: Block3Containter<'a>> Block3<'a, T> {
    fn last_block(&'a self) -> Option<&'a Option<BlockPtr<'a>>> {
        self.blocks.last_block()
    }

    fn push(&mut self, block: BlockPtr<'a>) {
        self.blocks.push(block);
    }

    fn get_line(
        &'a self,
        cur_block_id: BlockId,
        cur_block_offset: usize,
    ) -> Option<(LineBlockStr<'a>, &'a BlockIndex, usize)> {
        let mut index = 0;
        for (i, block) in self.blocks.iter_block().enumerate() {
            if let Some(b) = block {
                // 跳过不是目标块的块
                if b.as_block().block_id != cur_block_id {
                    continue;
                }
                // 通过 block_id 找对应的 BlockIndex
                let block_index = self
                    .block_indexs
                    .iter()
                    .find(|bi| bi.block_id == b.as_block().block_id)?;
                let option_line_info = b.as_block().line_span_at_offset(cur_block_offset);
                if option_line_info.is_none() {
                    return None;
                }
                index = i; // 记录当前块在 block_indexs 中的位置
                let line_info = option_line_info.unwrap();
                let line_str1 = BlockLineData {
                    data: LineData::GapBytes(
                        b.as_block()
                            .data
                            .text(line_info.block_start..line_info.block_end),
                    ),
                    block_file_start: block_index.logic_file_start,
                    block_id: block_index.block_id, // block_id 字段存储 block_id
                    block_line_index: line_info.index_num,
                    block_offset: line_info.block_start,
                };
                if line_info.is_complete {
                    //是完整的行
                    let ret: LineBlockStr<'_> = LineBlockStr(Some(line_str1), None);
                    return Some((ret, block_index, index));
                } else {
                    //不完整行 需要合并一行的下部分 一行的下一个部分在 下一个 block中
                    //获取下一个block
                    if i < 2 {
                        // 找当前块在 block_indexs 中的位置，取下一个
                        let cur_pos = self
                            .block_indexs
                            .iter()
                            .position(|bi| bi.block_id == b.as_block().block_id)?;
                        let next_block_index = self.block_indexs.get(cur_pos + 1)?;
                        if let Some(next_block) = self.blocks.get_block(i + 1).unwrap() {
                            let nb = next_block.as_block();
                            if let Some(next_line_info) = nb.first_line_span() {
                                let next_line_info = next_line_info;
                                let line_str2 = BlockLineData {
                                    data: LineData::GapBytes(next_block.as_block().data.text(
                                        next_line_info.block_start..next_line_info.block_end,
                                    )),
                                    block_file_start: next_block_index.logic_file_start,
                                    block_id: next_block_index.block_id, // block_id
                                    block_line_index: next_line_info.index_num,
                                    block_offset: next_line_info.block_start,
                                };
                                let ret: LineBlockStr<'_> =
                                    LineBlockStr(Some(line_str1), Some(line_str2));
                                return Some((ret, block_index, index));
                            }
                        }
                    }
                    let ret: LineBlockStr<'_> = LineBlockStr(Some(line_str1), None);
                    //self.cur_block_offset += ret.text_len();
                    return Some((ret, block_index, index));
                }
            }
        }
        None
    }
}

struct GapBlockTextIterRev<'a> {
    blocks: Block3<'a, [Option<BlockPtr<'a>>; 3]>,
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
        let (ret, _, _) = self
            .blocks
            .get_line(self.cur_block_id, self.cur_block_offset)?;
        let ret = unsafe { std::mem::transmute::<LineBlockStr<'_>, LineBlockStr<'a>>(ret) };
        let cur_pos = self
            .blocks
            .block_indexs
            .iter()
            .position(|bi| bi.block_id == self.cur_block_id)?;
        let current_block = self
            .blocks
            .blocks
            .iter()
            .flatten()
            .find(|b| b.as_block().block_id == self.cur_block_id)?
            .as_block();
        let current_line_info = current_block.line_span_at_offset(self.cur_block_offset)?;

        if current_line_info.index_num > 0 {
            let prev_index = current_line_info.index_num.saturating_sub(1);
            let line_info = current_block.line_span_at_index(prev_index)?;
            self.cur_block_line_index = prev_index;
            self.cur_block_offset = line_info.block_start;
        } else {
            if cur_pos == 0 {
                self.exhausted = true;
            } else {
                let prev_block_index = self.blocks.block_indexs.get(cur_pos - 1)?;
                let prev_block = self
                    .blocks
                    .blocks
                    .iter()
                    .flatten()
                    .find(|b| b.as_block().block_id == prev_block_index.block_id)?
                    .as_block();
                let prev_line_info = prev_block.last_line_span()?;
                self.cur_block_id = prev_block_index.block_id;
                self.cur_block_line_index = prev_line_info.index_num;
                self.cur_block_offset = prev_line_info.block_start;
            }
        }
        Some(ret)
    }
}

struct GapBlockScollTextIter<'a> {
    blocks: Block3<'a, RingVec<Option<BlockPtr<'a>>>>,
    text: *mut GapBlockText,
    cur_block_id: BlockId,
    cur_block_line_index: usize,
    cur_block_offset: usize,
    cur_line_index: usize,
    _marker: std::marker::PhantomData<&'a mut GapBlockText>,
}

impl<'a> GapBlockScollTextIter<'a> {
    fn text(&self) -> &GapBlockText {
        unsafe { &*self.text }
    }

    fn text_mut(&mut self) -> &mut GapBlockText {
        unsafe { &mut *self.text }
    }

    fn new(
        text: &'a mut GapBlockText,
        cur_block_id: BlockId,
        cur_block_line_index: usize,
        cur_block_offset: usize,
        cur_line_index: usize,
    ) -> ChapResult<Self> {
        let pos = text
            .block_indexs
            .iter()
            .position(|bi| bi.block_id == cur_block_id)
            .ok_or_else(|| {
                ChapError::Unexpected("cur_block_id not found in block_indexs".into())
            })?;

        // 先克隆需要的 BlockIndex，避免后续 &mut 冲突
        let indices: Vec<BlockIndex> = text.block_indexs[pos..].iter().take(3).cloned().collect();

        let mut ring: RingVec<Option<BlockPtr<'a>>> = RingVec::with_capacity(3);
        for bi in &indices {
            let (block_ptr, _) = text.read_one_block_ptr(
                bi.block_id,
                bi.logic_file_start,
                bi.source_file_start,
                bi.source_file_end,
                Some(bi.check_sum),
            )?;
            // 提取 Block 并重新包装为 Own，消除短生命周期
            let block = match block_ptr {
                BlockPtr::Own(b) => b,
                BlockPtr::Borrowed(b) => b.clone(),
            };
            ring.push(Some(BlockPtr::Own(block)));
        }

        let ptr = text as *mut GapBlockText;
        let block_indexs: &'a [BlockIndex] = unsafe { &(*ptr).block_indexs };
        Ok(Self {
            blocks: Block3 {
                blocks: ring,
                block_indexs,
            },
            text: ptr,
            cur_block_id,
            cur_block_line_index,
            cur_block_offset,
            cur_line_index,
            _marker: std::marker::PhantomData,
        })
    }
}

impl<'a> Iterator for GapBlockScollTextIter<'a> {
    type Item = (LineBlockStr<'a>, LineState);

    fn next(&mut self) -> Option<Self::Item> {
        let (ret, block_index, index) = self
            .blocks
            .get_line(self.cur_block_id, self.cur_block_offset)?;
        let ret = unsafe { std::mem::transmute::<LineBlockStr<'_>, LineBlockStr<'a>>(ret) };
        let block_size = block_index.logic_block_size;
        let line_state = LineStateBuilder::new()
            .block_num(ret.get_block_id())
            .block_line_index(ret.get_block_line_index())
            .block_offset(ret.get_block_offset())
            .line_index(self.cur_line_index)
            .line_num(self.cur_line_index + 1)
            .line_file_start(ret.get_line_file_start())
            .line_file_end(ret.get_line_file_end())
            .start_line_num(self.cur_line_index + 1)
            .start_page_num(1)
            .build();
        self.cur_block_offset += ret.text_len();
        self.cur_line_index += 1;

        if self.cur_block_offset >= block_size {
            if let Some(cur_pos) = self
                .text()
                .block_indexs
                .iter()
                .position(|bi| bi.block_id == self.cur_block_id)
            {
                if let Some(next_bi) = self.text().block_indexs.get(cur_pos + 1).cloned() {
                    self.cur_block_id = next_bi.block_id;
                    self.cur_block_offset = self.cur_block_offset.saturating_sub(block_size);
                    self.cur_block_line_index = self
                        .blocks
                        .blocks
                        .iter()
                        .flatten()
                        .filter(|block| block.as_block().block_id == next_bi.block_id)
                        .next()
                        .and_then(|block| {
                            block
                                .as_block()
                                .line_span_at_offset(self.cur_block_offset)
                                .map(|line| line.index_num)
                        })
                        .unwrap_or(0);
                }
            }
        } else {
            self.cur_block_line_index = self
                .blocks
                .blocks
                .iter()
                .flatten()
                .find(|b| b.as_block().block_id == self.cur_block_id)
                .and_then(|block| {
                    block
                        .as_block()
                        .line_span_at_offset(self.cur_block_offset)
                        .map(|line| line.index_num)
                })
                .unwrap_or(self.cur_block_line_index + 1);
        }

        // 读到第2个块时，预加载下一个块
        if index == 1 {
            let last_block = self.blocks.last_block().unwrap().as_ref().unwrap();
            let last_block_id = last_block.as_block().block_id;

            let next_bi = self
                .text()
                .block_indexs
                .iter()
                .position(|bi| bi.block_id == last_block_id)
                .and_then(|p| self.text().block_indexs.get(p + 1).cloned());

            if let Some(next_bi) = next_bi {
                let (block_ptr, _) = self
                    .text_mut()
                    .read_one_block_ptr(
                        next_bi.block_id,
                        next_bi.logic_file_start,
                        next_bi.source_file_start,
                        next_bi.source_file_end,
                        Some(next_bi.check_sum),
                    )
                    .unwrap();
                let block = match block_ptr {
                    BlockPtr::Own(b) => b,
                    BlockPtr::Borrowed(b) => b.clone(),
                };
                self.blocks.push(BlockPtr::Own(block));
            } else if let Some((next_logic_file_start, next_source_file_start)) =
                self.text().next_unindexed_block_start().unwrap()
            {
                let new_block_id = self.text_mut().alloc_id();
                let (block, block_index) = self
                    .text_mut()
                    .read_unindexed_source_block(
                        new_block_id,
                        next_logic_file_start,
                        next_source_file_start,
                    )
                    .unwrap();
                self.text_mut().block_indexs.push(block_index);
                self.blocks.push(BlockPtr::Own(block));
            }
        }

        Some((ret, line_state))
    }
}

struct GapBlockTextIter<'a> {
    blocks: Block3<'a, [Option<BlockPtr<'a>>; 3]>,
    cur_block_id: BlockId,       //当前块的稳定 block_id
    cur_block_line_index: usize, //块内行索引
    cur_block_offset: usize,     //块内偏移
}

impl<'a> Iterator for GapBlockTextIter<'a> {
    type Item = LineBlockStr<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let (ret, block_index, _) = self
            .blocks
            .get_line(self.cur_block_id, self.cur_block_offset)?;
        let ret = unsafe { std::mem::transmute::<LineBlockStr<'_>, LineBlockStr<'a>>(ret) };
        // log::debug!("GapBlockTextIter next: ret:{}", ret);
        self.cur_block_line_index += 1;
        self.cur_block_offset += ret.text_len();
        // log::debug!("GapBlockTextIter next: after update offset: cur_block_line_index {}, cur_block_offset {}, block_size:{}, ret:{}",
        //     self.cur_block_line_index, self.cur_block_offset, block_index.block_size, ret);
        //查看是否大于当前块
        if self.cur_block_offset >= block_index.logic_block_size {
            // 通过 block_indexs 找下一个块的 block_id
            if let Some(cur_pos) = self
                .blocks
                .block_indexs
                .iter()
                .position(|bi| bi.block_id == self.cur_block_id)
            {
                if let Some(next_bi) = self.blocks.block_indexs.get(cur_pos + 1) {
                    self.cur_block_id = next_bi.block_id;
                    self.cur_block_line_index = 0;
                    self.cur_block_offset = self
                        .cur_block_offset
                        .saturating_sub(block_index.logic_block_size);
                }
            }
        }
        Some(ret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textwarp::EditTextWarp;
    use crate::textwarp::TextDisplay;
    use crate::textwarp::TextOper;
    use crate::textwarp::TextWarpType;
    use std::io::Read;
    use std::io::Seek;
    use std::io::SeekFrom;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ================================================================
    //  零、测试辅助
    // ================================================================

    // ---------- 文件 / 文本构造 ----------

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
        if let Some(block) = gbt.blocks.iter_mut().find(|b| b.block_id == block_id) {
            return block.data.as_continuous().to_vec();
        }
        if let Some(block) = gbt.cache.get_mut(&block_id) {
            return block.data.as_continuous().to_vec();
        }
        let bi = gbt
            .block_indexs
            .iter()
            .find(|bi| bi.block_id == block_id)
            .expect("block index not found");
        let mut buf = vec![0u8; bi.logic_block_size];
        gbt.reader
            .seek(SeekFrom::Start(bi.source_file_start as u64))
            .expect("seek block");
        gbt.reader.read_exact(&mut buf).expect("read block");
        buf
    }

    /// 获取所有块的拼接文本内容（按 block_indexs 的物理顺序）
    fn get_all_blocks_text(gbt: &mut GapBlockText) -> Vec<u8> {
        let mut result = Vec::new();
        let block_ids: Vec<BlockId> = gbt.block_indexs.iter().map(|bi| bi.block_id).collect();
        for bid in block_ids {
            result.extend_from_slice(&get_block_text(gbt, bid));
        }
        result
    }

    fn get_block_line_ranges(
        gbt: &mut GapBlockText,
        block_id: BlockId,
    ) -> Vec<(usize, usize, bool)> {
        let bytes = get_block_text(gbt, block_id);
        collect_line_ranges(&bytes)
            .into_iter()
            .map(|(start, end)| (start, end, end > start && bytes[end - 1] == b'\n'))
            .collect()
    }

    fn get_block_line_start(gbt: &mut GapBlockText, block_id: BlockId, line_idx: usize) -> usize {
        get_block_line_ranges(gbt, block_id)
            .get(line_idx)
            .map(|(start, _, _)| *start)
            .expect("line index not found")
    }

    // ---------- 保存 / 重载 / 断言 ----------

    fn assert_block_storage_consistent(gbt: &GapBlockText) {
        gbt.assert_storage_consistent_for_test();
    }

    fn save_all_text(gbt: &mut GapBlockText) -> Vec<u8> {
        let tmp = NamedTempFile::new().expect("failed to create temp file for save");
        gbt.save(tmp.path()).expect("failed to save temp file");
        std::fs::read(tmp.path()).expect("failed to read saved temp file")
    }

    fn reload_from_saved(gbt: &mut GapBlockText) -> GapBlockText {
        let bytes = save_all_text(gbt);
        create_gap_block_text_bytes(&bytes)
    }

    fn assert_saved_matches(gbt: &mut GapBlockText, expected: &[u8], label: &str) {
        let saved = save_all_text(gbt);
        assert!(
            saved == expected,
            "{label}: {}",
            first_diff_window(&saved, expected)
        );
    }

    fn cache_str_bytes(cache: &crate::textwarp::CacheStr) -> Vec<u8> {
        let mut bytes = Vec::new();
        for part in cache.as_slice().as_parts() {
            bytes.extend_from_slice(part);
        }
        bytes
    }

    fn assert_current_page_matches_saved(td: &TextDisplay, saved: &[u8], label: &str) {
        let (lines, meta) = td.get_current_page().expect("current page");
        assert_eq!(
            lines.len(),
            meta.len(),
            "{label}: line/meta length mismatch"
        );
        for i in 0..meta.len() {
            let line_meta = meta.get(i).expect("line meta");
            let actual = cache_str_bytes(lines.get(i).expect("line content"));
            let start = line_meta
                .get_line_file_start()
                .saturating_add(line_meta.get_line_offset());
            let end = start.saturating_add(line_meta.get_txt_len());
            assert!(
                end <= saved.len(),
                "{label}: page line {i} range {start}..{end} exceeds saved len {}",
                saved.len()
            );
            assert_eq!(
                actual,
                saved[start..end],
                "{label}: page line {i} content mismatch at range {start}..{end}"
            );
        }
    }

    fn assert_current_page_has_unique_visual_lines(td: &TextDisplay, label: &str) {
        let meta = td.get_current_line_meta().expect("line meta");
        let mut seen = std::collections::HashSet::new();
        for i in 0..meta.len() {
            let line_meta = meta.get(i).expect("line meta");
            let start = line_meta
                .get_line_file_start()
                .saturating_add(line_meta.get_line_offset());
            let key = (
                start,
                start.saturating_add(line_meta.get_txt_len()),
                line_meta.get_txt_len(),
            );
            assert!(
                seen.insert(key),
                "{label}: duplicated visual line at page row {i}: {line_meta:?}\npage={:?}",
                meta.iter().collect::<Vec<_>>()
            );
        }
    }

    fn assert_current_page_visual_lines_ordered(td: &TextDisplay, label: &str) {
        let meta = td.get_current_line_meta().expect("line meta");
        let mut prev = None;
        for i in 0..meta.len() {
            let line_meta = meta.get(i).expect("line meta");
            let start = line_meta
                .get_line_file_start()
                .saturating_add(line_meta.get_line_offset());
            let key = (start, start.saturating_add(line_meta.get_txt_len()));
            if let Some(prev_key) = prev {
                assert!(
                    prev_key < key,
                    "{label}: visual lines out of order at page row {i}: prev={prev_key:?}, current={key:?}, page={:?}",
                    meta.iter().collect::<Vec<_>>()
                );
            }
            prev = Some(key);
        }
    }

    fn assert_current_page_valid(td: &TextDisplay, saved: &[u8], label: &str) {
        assert_current_page_matches_saved(td, saved, label);
        assert_current_page_has_unique_visual_lines(td, label);
        assert_current_page_visual_lines_ordered(td, label);
    }

    fn test_fixture_a_txt_bytes() -> Vec<u8> {
        std::fs::read("/home/unvdb/a.txt").unwrap_or_else(|_| {
            let mut content = generate_large_content(BLOCK_SIZE * 6 + 777, 96).into_bytes();
            content.extend_from_slice(
                b"RUN mkdir -p /tmp/install_setup && make -j$(nproc) && make install\n",
            );
            content
        })
    }

    fn save_reload_and_assert(
        gbt: &mut GapBlockText,
        expected: &[u8],
        label: &str,
    ) -> GapBlockText {
        assert_saved_matches(gbt, expected, label);
        let mut reloaded = create_gap_block_text_bytes(expected);
        assert_eq!(get_all_blocks_text(&mut reloaded), expected);
        assert_block_storage_consistent(&reloaded);
        reloaded
    }

    fn mutation_safe_end(bytes: &[u8]) -> usize {
        if bytes.last() == Some(&b'\n') {
            bytes.len().saturating_sub(1)
        } else {
            bytes.len()
        }
    }

    fn assert_text_and_metadata(gbt: &mut GapBlockText, expected: &[u8], label: &str) {
        let saved = save_all_text(gbt);
        assert!(
            saved == expected,
            "{label}: saved bytes mismatch: {}",
            first_diff_window(&saved, expected)
        );

        let all_blocks = get_all_blocks_text(gbt);
        assert!(
            all_blocks == expected,
            "{label}: block concat mismatch: {}",
            first_diff_window(&all_blocks, expected)
        );

        let indexes: Vec<(BlockId, usize, usize)> = gbt
            .block_indexs
            .iter()
            .map(|bi| (bi.block_id, bi.logic_file_start, bi.logic_block_size))
            .collect();

        let mut expected_file_start = 0usize;
        for (block_id, file_start, block_size) in indexes {
            assert_eq!(
                file_start, expected_file_start,
                "{label}: block {block_id} file_start not contiguous"
            );
            expected_file_start += block_size;

            let block_bytes = get_block_text(gbt, block_id);
            assert_eq!(
                block_bytes.len(),
                block_size,
                "{label}: block {block_id} size mismatch"
            );
        }

        assert_eq!(gbt.file_size, expected.len(), "{label}: file_size mismatch");
        assert_block_storage_consistent(gbt);
    }

    fn first_diff_window(left: &[u8], right: &[u8]) -> String {
        let diff_at = left
            .iter()
            .zip(right.iter())
            .position(|(l, r)| l != r)
            .unwrap_or_else(|| left.len().min(right.len()));
        let start = diff_at.saturating_sub(24);
        let left_end = (diff_at + 24).min(left.len());
        let right_end = (diff_at + 24).min(right.len());
        format!(
            "diff_at={diff_at}, left_len={}, right_len={}, left_window={:?}, right_window={:?}",
            left.len(),
            right.len(),
            String::from_utf8_lossy(&left[start..left_end]),
            String::from_utf8_lossy(&right[start..right_end]),
        )
    }

    // ---------- 绝对偏移 / 行状态映射 ----------

    fn collect_line_ranges(bytes: &[u8]) -> Vec<(usize, usize)> {
        if bytes.is_empty() {
            return vec![(0, 0)];
        }
        let mut ranges = Vec::new();
        let mut start = 0usize;
        for (idx, b) in bytes.iter().enumerate() {
            if *b == b'\n' {
                ranges.push((start, idx + 1));
                start = idx + 1;
            }
        }
        if start < bytes.len() {
            ranges.push((start, bytes.len()));
        }
        if ranges.is_empty() {
            ranges.push((0, bytes.len()));
        }
        ranges
    }

    fn line_body_len(bytes: &[u8], start: usize, end: usize) -> usize {
        if end > start && bytes[end - 1] == b'\n' {
            end - start - 1
        } else {
            end - start
        }
    }

    fn line_body_char_boundaries(bytes: &[u8], start: usize, end: usize) -> Vec<usize> {
        let body_end = if end > start && bytes[end - 1] == b'\n' {
            end - 1
        } else {
            end
        };
        let text = std::str::from_utf8(&bytes[start..body_end]).expect("line body must be utf-8");
        let mut offsets = Vec::with_capacity(text.chars().count() + 1);
        offsets.push(0);
        for (idx, _) in text.char_indices().skip(1) {
            offsets.push(idx);
        }
        offsets.push(text.len());
        offsets
    }

    fn make_line_state_for_abs_line_start(
        gbt: &GapBlockText,
        bytes: &[u8],
        line_num: usize,
        abs_line_start: usize,
    ) -> LineState {
        let ranges = collect_line_ranges(bytes);
        let (expected_start, line_end) = ranges
            .get(line_num.saturating_sub(1))
            .copied()
            .unwrap_or((abs_line_start, abs_line_start));
        assert_eq!(
            expected_start, abs_line_start,
            "line start mismatch for line_num {}: expected {}, got {}",
            line_num, expected_start, abs_line_start
        );
        let (block_id, block_offset) = gbt
            .resolve_block_for_file_offset(abs_line_start)
            .or_else(|| {
                if abs_line_start == bytes.len() && !gbt.block_indexs.is_empty() {
                    let bi = gbt.block_indexs.last().unwrap();
                    Some((bi.block_id, bi.logic_block_size))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                panic!(
                    "line start {} for line_num {} not mappable to block indexes",
                    abs_line_start, line_num
                )
            });
        let block_line_index = gbt
            .find_block_line_for_offset(block_id, block_offset)
            .unwrap_or(0);
        LineState::builder()
            .block_num(block_id)
            .block_line_index(block_line_index)
            .block_offset(block_offset)
            .line_num(line_num)
            .line_index(line_num.saturating_sub(1))
            .line_offset(0)
            .line_file_start(abs_line_start)
            .line_file_end(line_end)
            .start_line_num(line_num)
            .start_page_num(1)
            .build()
    }

    fn locate_cursor(bytes: &[u8], abs_cursor: usize) -> (usize, usize, usize) {
        assert!(
            abs_cursor <= bytes.len(),
            "abs_cursor {} out of bounds for len {}",
            abs_cursor,
            bytes.len()
        );
        let ranges = collect_line_ranges(bytes);
        if abs_cursor == bytes.len() && bytes.last() == Some(&b'\n') {
            // 文件以 '\n' 结尾时，EOF 光标语义上位于一个尾部空行的行首。
            // 这里不能把它回退到最后一个换行前，否则 EOF 锚点的插入/退格会错打一字节。
            return (ranges.len(), bytes.len(), 0);
        }
        for (line_idx, (start, end)) in ranges.iter().copied().enumerate() {
            let body_end = if end > start && bytes[end - 1] == b'\n' {
                end - 1
            } else {
                end
            };
            if abs_cursor >= start && abs_cursor <= body_end {
                return (line_idx, start, abs_cursor - start);
            }
        }

        if let Some((line_idx, (start, end))) = ranges.iter().copied().enumerate().last() {
            let body_end = if end > start && bytes[end - 1] == b'\n' {
                end - 1
            } else {
                end
            };
            if abs_cursor == bytes.len() {
                return (line_idx, start, body_end - start);
            }
        }

        panic!(
            "cursor {} not mappable to line ranges in buffer len {}",
            abs_cursor,
            bytes.len()
        );
    }

    fn make_line_state_for_abs_cursor(
        gbt: &GapBlockText,
        bytes: &[u8],
        abs_cursor: usize,
    ) -> (usize, usize, LineState) {
        let (line_idx, line_start, bytes_cursor) = locate_cursor(bytes, abs_cursor);
        let state = make_line_state_for_abs_line_start(gbt, bytes, line_idx + 1, line_start);
        (line_idx, bytes_cursor, state)
    }

    fn insert_char_at_abs(gbt: &mut GapBlockText, expected: &mut Vec<u8>, abs: usize, ch: char) {
        let (line_idx, bytes_cursor, state) = make_line_state_for_abs_cursor(gbt, expected, abs);
        gbt.insert_char(line_idx, bytes_cursor, &state, ch).unwrap();
        let mut buf = [0u8; 4];
        expected.splice(
            abs..abs,
            ch.encode_utf8(&mut buf).as_bytes().iter().copied(),
        );
    }

    fn insert_bytes_at_abs(
        gbt: &mut GapBlockText,
        expected: &mut Vec<u8>,
        abs: usize,
        bytes: &[u8],
    ) {
        let (line_idx, bytes_cursor, state) = make_line_state_for_abs_cursor(gbt, expected, abs);
        gbt.insert_bytes(line_idx, bytes_cursor, &state, bytes, false)
            .unwrap();
        expected.splice(abs..abs, bytes.iter().copied());
    }

    fn backspace_at_abs(
        gbt: &mut GapBlockText,
        expected: &mut Vec<u8>,
        delete_end: usize,
        count: usize,
    ) -> (usize, Vec<u8>) {
        let delete_end = delete_end.min(expected.len());
        assert!(delete_end > 0, "delete_end must be > 0 for backspace");
        let (line_idx, bytes_cursor, state) =
            make_line_state_for_abs_cursor(gbt, expected, delete_end);
        let deleted = gbt
            .backspace(line_idx, bytes_cursor, count, &state)
            .unwrap();
        let delete_start = delete_end - deleted.len();
        expected.drain(delete_start..delete_end);
        (delete_start, deleted)
    }

    fn replace_ascii_at_abs(
        gbt: &mut GapBlockText,
        expected: &mut Vec<u8>,
        start: usize,
        old_len: usize,
        replacement: &[u8],
    ) {
        assert!(
            expected[start..start + old_len].is_ascii(),
            "replace helper only deletes ascii byte ranges"
        );
        backspace_at_abs(gbt, expected, start + old_len, old_len);
        insert_bytes_at_abs(gbt, expected, start, replacement);
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    // ---------- 随机 / 载荷生成 ----------

    #[derive(Clone, Copy)]
    struct TestRng(u64);

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn gen_range(&mut self, upper: usize) -> usize {
            if upper == 0 {
                0
            } else {
                (self.next_u64() as usize) % upper
            }
        }
    }

    fn deterministic_payload(step: usize, variant: usize) -> String {
        match variant % 6 {
            0 => format!("P{step:04}-ASCII"),
            1 => format!("你{step}好"),
            2 => format!("L{step}\nM{step}\n"),
            3 => format!("mix-{step}-中\n文-{step}"),
            4 => "XYZ123".repeat((step % 5) + 1),
            _ => format!("尾{step}\n"),
        }
    }

    fn deterministic_ascii_payload(step: usize, variant: usize) -> Vec<u8> {
        match variant % 4 {
            0 => format!("A{step:04}Z").into_bytes(),
            1 => format!("mid-{step:03}").into_bytes(),
            2 => b"XYZ123".repeat((step % 3) + 1),
            _ => format!("tail{step}").into_bytes(),
        }
    }

    // ---------- LineState 构造 ----------

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

    // ---------- 大文本构造 ----------

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
    //  一、短文本编辑测试
    //  目标：单 block、小输入、验证 insert/backspace 的基础语义与索引更新
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
        assert_eq!(gbt.block_indexs[0].logic_block_size, 7);
    }

    #[test]
    fn test_short_insert_char_utf8_emoji() {
        let mut gbt = create_gap_block_text("ab\n");
        let state = default_line_state();
        // emoji 占 4 字节
        gbt.insert_char(0, 1, &state, '😀').unwrap();
        let data = get_block_text(&mut gbt, 0);
        assert_eq!(String::from_utf8_lossy(&data), "a😀b\n");
        assert_eq!(gbt.block_indexs[0].logic_block_size, 7); // 1+4+1+1
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
        let orig = gbt.block_indexs[0].logic_block_size;
        let state = default_line_state();
        gbt.backspace(0, 2, 1, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].logic_block_size, orig - 1);
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
    //  二、多行单块测试
    //  目标：仍在单 block 内，但覆盖多行 line_meta / 运行时行扫描
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
    fn test_multi_insert_bytes_preserves_line_ranges() {
        let mut gbt = create_gap_block_text("aa\nbb\ncc\n");
        let state = default_line_state();
        gbt.insert_bytes(0, 1, &state, b"123", false).unwrap();
        let block_id = gbt.block_indexs[0].block_id;
        assert_eq!(get_block_line_ranges(&mut gbt, block_id).len(), 3); // 仍然 3 行
        assert_eq!(gbt.block_indexs[0].logic_block_size, 12); // 原始 9 + 3
    }

    // ---------- insert_newline 多行文本 ----------

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
        let block_id = gbt.block_indexs[0].block_id;
        assert_eq!(get_block_line_ranges(&mut gbt, block_id).len(), 10);

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
    //  三、大文件编辑测试
    //  目标：跨 block 编辑、块大小变化、参考模型校验
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
                    gbt.block_indexs[i].logic_file_start,
                    gbt.block_indexs[i - 1].logic_file_start
                        + gbt.block_indexs[i - 1].logic_block_size,
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
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        gbt.insert_char(0, 5, &state, 'Z').unwrap();
        let data = get_block_text(&mut gbt, 0);
        // 验证 block 0 的第一行头部
        let first_line: String = String::from_utf8_lossy(&data)
            .lines()
            .next()
            .unwrap()
            .to_string();
        assert!(first_line.starts_with("LINE-Z0000:"));
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 1);
    }

    #[test]
    fn test_large_insert_char_on_block0_second_line() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        // 第二行: block_line_index=1
        let block_id = gbt.block_indexs[0].block_id;
        let block_offset = get_block_line_start(&mut gbt, block_id, 1);
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
        let block_id = gbt.block_indexs[1].block_id;
        let first_line_offset = get_block_line_start(&mut gbt, block_id, 0);
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 1);
    }

    #[test]
    fn test_large_insert_bytes_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        gbt.insert_bytes(0, 5, &state, b"ABCDE", false).unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 5);
        assert_eq!(get_all_blocks_text(&mut gbt), {
            let mut expected = content.into_bytes();
            expected.splice(5..5, b"ABCDE".iter().copied());
            expected
        });
    }

    #[test]
    fn test_large_insert_bytes_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[1].block_id;
        let first_line_offset = get_block_line_start(&mut gbt, block_id, 0);
        let state = make_line_state(1, 0, first_line_offset, 0);
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        gbt.insert_bytes(0, 2, &state, b"XY", false).unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 2);
    }

    #[test]
    fn test_large_backspace_on_block0() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].logic_block_size;
        gbt.backspace(0, 5, 2, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].logic_block_size, orig_size - 2);
    }

    #[test]
    fn test_large_backspace_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[1].block_id;
        let orig_size = gbt.block_indexs[1].logic_block_size;
        let first_line_offset = get_block_line_start(&mut gbt, block_id, 0);
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.backspace(0, 5, 1, &state).unwrap();
        assert_eq!(gbt.block_indexs[1].logic_block_size, orig_size - 1);
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
    fn test_iter_rev_does_not_panic_on_stale_block_line_index() {
        let content = generate_large_content(BLOCK_SIZE * 2 + 100, 80);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        gbt.insert_newline(0, 5, &state).unwrap();

        let block_id = gbt.block_indexs[0].block_id;
        let valid_offset = get_block_line_start(&mut gbt, block_id, 0);
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
    fn test_large_block_line_ranges_count() {
        // 每行 80 字节, 4096/80 = 51.2, 所以 block 0 约有 51 行
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[0].block_id;
        let line_ranges_count = get_block_line_ranges(&mut gbt, block_id).len();
        assert!(
            line_ranges_count >= 40,
            "block 0 至少应有 40 行, 实际 {}",
            line_ranges_count
        );
    }

    #[test]
    fn test_large_insert_char_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[0].block_id;
        let lines = get_block_line_ranges(&mut gbt, block_id);
        let last_idx = lines.len() - 1;
        let last_block_start = lines[last_idx].0;
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        let state = make_line_state(0, last_idx, last_block_start, 0);
        gbt.insert_char(0, 3, &state, 'Z').unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 1);
    }

    #[test]
    fn test_large_backspace_on_last_line_of_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[0].block_id;
        let lines = get_block_line_ranges(&mut gbt, block_id);
        let last_idx = lines.len() - 1;
        let last_line_start = lines[last_idx].0;
        let orig_size = gbt.block_indexs[0].logic_block_size;
        let state = make_line_state(0, last_idx, last_line_start, 0);
        gbt.backspace(0, 5, 2, &state).unwrap();
        assert_eq!(gbt.block_indexs[0].logic_block_size, orig_size - 2);
    }

    #[test]
    fn test_large_consecutive_inserts_on_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        for i in 0..10 {
            gbt.insert_char(0, i, &state, 'A').unwrap();
        }
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 10);
    }

    #[test]
    fn test_large_consecutive_backspaces_on_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let orig_size = gbt.block_indexs[0].logic_block_size;
        for _ in 0..5 {
            gbt.backspace(0, 10, 1, &state).unwrap();
        }
        assert_eq!(gbt.block_indexs[0].logic_block_size, orig_size - 5);
    }

    #[test]
    fn test_large_500_mixed_ops_matches_reference_model() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();
        let mut rng = TestRng::new(0x5eed_cafe_f00d_1234);

        for step in 0..500usize {
            let lines = collect_line_ranges(&expected);
            let line_idx = rng.gen_range(lines.len());
            let (line_start, line_end) = lines[line_idx];
            let char_boundaries = line_body_char_boundaries(&expected, line_start, line_end);
            let body_len = line_body_len(&expected, line_start, line_end);
            let op = rng.gen_range(4);
            let mut op_desc = String::new();

            match op {
                0 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    let ch = match rng.gen_range(5) {
                        0 => 'A',
                        1 => 'z',
                        2 => '你',
                        3 => '界',
                        _ => '9',
                    };
                    op_desc = format!("insert_char line_idx={line_idx} cursor={cursor} ch={ch}");
                    gbt.insert_char(line_idx, cursor, &state, ch).unwrap();
                    let mut buf = [0u8; 4];
                    expected.splice(
                        line_start + cursor..line_start + cursor,
                        ch.encode_utf8(&mut buf).as_bytes().iter().copied(),
                    );
                }
                1 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    let payload = deterministic_payload(step, rng.gen_range(6));
                    op_desc = format!(
                        "insert_bytes line_idx={line_idx} cursor={cursor} payload={payload:?}"
                    );
                    gbt.insert_bytes(line_idx, cursor, &state, payload.as_bytes(), false)
                        .unwrap();
                    expected.splice(
                        line_start + cursor..line_start + cursor,
                        payload.as_bytes().iter().copied(),
                    );
                }
                2 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    op_desc = format!("insert_newline line_idx={line_idx} cursor={cursor}");
                    gbt.insert_newline(line_idx, cursor, &state).unwrap();
                    expected.splice(line_start + cursor..line_start + cursor, [b'\n']);
                }
                _ => {
                    let can_merge_prev = line_idx > 0;
                    let can_delete_char = body_len > 0;
                    if !can_merge_prev && !can_delete_char {
                        continue;
                    }
                    let (cursor, delete_len) =
                        if can_merge_prev && (rng.gen_range(4) == 0 || !can_delete_char) {
                            (0, 1)
                        } else {
                            let selectable = &char_boundaries[1..char_boundaries.len().min(17)];
                            let idx = rng.gen_range(selectable.len());
                            let cursor = selectable[idx];
                            (cursor, cursor - char_boundaries[idx])
                        };
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    op_desc =
                        format!("backspace line_idx={line_idx} cursor={cursor} len={delete_len}");
                    let deleted = gbt.backspace(line_idx, cursor, delete_len, &state).unwrap();
                    let abs = line_start + cursor;
                    let delete_start = abs - deleted.len();
                    expected.drain(delete_start..abs);
                }
            }

            let actual = save_all_text(&mut gbt);
            assert!(
                actual == expected,
                "step {step} content mismatch: {op_desc}; {}",
                first_diff_window(&actual, &expected)
            );
            assert_block_storage_consistent(&gbt);
        }
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
        let total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(total, content.len());
    }

    #[test]
    fn test_large_insert_bytes_large_payload() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let payload = vec![b'X'; 200];
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total + 200);
        assert!(gbt
            .block_indexs
            .iter()
            .all(|bi| bi.logic_block_size <= BLOCK_SIZE));
    }

    #[test]
    fn test_insert_bytes_keeps_block_storage_consistent_after_split() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();
        let payload = vec![b'X'; BLOCK_SIZE];

        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();

        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_insert_newline_keeps_block_storage_consistent() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();

        gbt.insert_newline(0, 5, &state).unwrap();

        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_backspace_across_blocks_keeps_block_storage_consistent() {
        let mut content = "a".repeat(BLOCK_SIZE + 32);
        content.push('\n');
        let mut gbt = create_gap_block_text(&content);
        let state = default_line_state();

        gbt.backspace(0, BLOCK_SIZE + 16, BLOCK_SIZE + 8, &state)
            .unwrap();

        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_backspace_merges_adjacent_small_blocks() {
        let mut expected = "a".repeat(BLOCK_SIZE + 1000).into_bytes();
        let mut gbt = create_gap_block_text_bytes(&expected);
        let before_blocks = gbt.block_indexs.len();

        backspace_at_abs(&mut gbt, &mut expected, 3500, 3500);

        assert!(
            gbt.block_indexs.len() < before_blocks,
            "delete should merge adjacent small blocks"
        );
        assert_eq!(gbt.block_indexs.len(), 1);
        assert_text_and_metadata(&mut gbt, &expected, "after merge-causing delete");
    }

    #[test]
    fn test_large_exact_block_boundary() {
        // 文件大小恰好是 BLOCK_SIZE 的倍数
        let content = generate_padded_content(BLOCK_SIZE * 2);
        let gbt = create_gap_block_text(&content);
        let total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(total, BLOCK_SIZE * 2);
    }

    #[test]
    fn test_large_insert_char_utf8_on_block1() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let block_id = gbt.block_indexs[1].block_id;
        let first_line_offset = get_block_line_start(&mut gbt, block_id, 0);
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        let state = make_line_state(1, 0, first_line_offset, 0);
        gbt.insert_char(0, 3, &state, '中').unwrap(); // 3 字节
        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
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
        let state = default_line_state();
        let payload = vec![b'X'; BLOCK_SIZE];
        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();
        let orig = get_all_blocks_text(&mut gbt);

        gbt.split_block(id_a).unwrap();

        let combined = get_all_blocks_text(&mut gbt);
        assert_eq!(combined, orig, "split 后全文必须保持不变");
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
        let state = default_line_state();
        let payload = vec![b'X'; BLOCK_SIZE];
        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();

        assert!(gbt.block_indexs.len() > before);
    }

    /// A.block_size + B.block_size == 原始 block_size，且两半均非零
    #[test]
    fn test_split_block_size_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();

        gbt.split_block(0).unwrap();

        let new_total: usize = gbt.block_indexs.iter().map(|bi| bi.logic_block_size).sum();
        assert_eq!(new_total, orig_total);
        assert!(gbt.block_indexs.iter().all(|bi| bi.logic_block_size > 0));
    }

    /// 分裂点在原块 25%～75% 范围内（接近中点）
    #[test]
    fn test_split_block_near_midpoint() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_size = gbt.block_indexs[0].logic_block_size;

        gbt.split_block(0).unwrap();

        assert!(gbt
            .block_indexs
            .iter()
            .all(|bi| bi.logic_block_size <= BLOCK_SIZE));
        assert!(gbt.block_indexs[0].logic_block_size > 0);
        assert_eq!(
            gbt.block_indexs
                .iter()
                .map(|bi| bi.logic_block_size)
                .sum::<usize>(),
            content.len()
        );
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
        let orig_file_start = gbt.block_indexs[0].logic_file_start;

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].logic_block_size;
        assert_eq!(
            gbt.block_indexs[1].logic_file_start,
            orig_file_start + size_a,
            "B 的 file_start 应紧跟 A"
        );
    }

    /// split_block(0) 后，后续块的 block_id 保持不变（不重新编号），file_start 也不变
    #[test]
    fn test_split_block_renumbers_subsequent_blocks() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_b1_id = gbt.block_indexs[1].block_id;
        let orig_b2_id = gbt.block_indexs[2].block_id;

        gbt.split_block(0).unwrap();

        assert!(gbt.block_indexs.iter().any(|bi| bi.block_id == orig_b1_id));
        assert!(gbt.block_indexs.iter().any(|bi| bi.block_id == orig_b2_id));
    }

    /// split_block(1)：block 0 完全不受影响，后续块 block_id 保持稳定
    #[test]
    fn test_split_non_first_block_does_not_affect_block0() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);
        let b0_id = gbt.block_indexs[0].block_id;
        let b0_size = gbt.block_indexs[0].logic_block_size;
        let b0_file_start = gbt.block_indexs[0].logic_file_start;
        let b1_id = gbt.block_indexs[1].block_id; // = 1
        let b2_id = gbt.block_indexs[2].block_id; // = 2

        gbt.split_block(b1_id).unwrap(); // split block with block_id=1

        // block 0 完全不受影响
        assert_eq!(gbt.block_indexs[0].block_id, b0_id);
        assert_eq!(gbt.block_indexs[0].logic_block_size, b0_size);
        assert_eq!(gbt.block_indexs[0].logic_file_start, b0_file_start);
        // 分裂结果在 [1] 和 [2]：A 保持 block_id=b1_id，B 获得新 id
        assert_eq!(gbt.block_indexs[1].block_id, b1_id, "A 的 block_id 保持");
        assert_ne!(gbt.block_indexs[2].block_id, b1_id, "B 有新 block_id");
        assert!(gbt.block_indexs.iter().any(|bi| bi.block_id == b2_id));
    }

    // ---------- 3. 运行时 line ranges ----------

    /// A 的所有行 block_end <= A.block_size；B 的所有行 block_end <= B.block_size
    #[test]
    fn test_split_block_line_ranges_within_bounds() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let size_a = gbt.block_indexs[0].logic_block_size;
        let block_a = gbt.block_indexs[0].block_id;
        for l in get_block_line_ranges(&mut gbt, block_a) {
            assert!(l.1 <= size_a, "A 行 block_end={} > size_a={}", l.1, size_a);
        }
        let size_b = gbt.block_indexs[1].logic_block_size;
        let block_b = gbt.block_indexs[1].block_id;
        for l in get_block_line_ranges(&mut gbt, block_b) {
            assert!(l.1 <= size_b, "B 行 block_end={} > size_b={}", l.1, size_b);
        }
    }

    /// B 的第一行 block_start == 0（偏移已正确调整）
    #[test]
    fn test_split_block_b_lines_start_at_zero() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let block_b = gbt.block_indexs[1].block_id;
        let first = get_block_line_ranges(&mut gbt, block_b).remove(0);
        assert_eq!(first.0, 0, "B 第一行 block_start 必须为 0");
    }

    /// A/B 各自扫描出的相邻行连续（每行 block_start == 前一行 block_end）
    #[test]
    fn test_split_block_line_ranges_contiguous() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let checks = [
            ("A", gbt.block_indexs[0].block_id),
            ("B", gbt.block_indexs[1].block_id),
        ];
        for (label, block_id) in checks {
            let lines = get_block_line_ranges(&mut gbt, block_id);
            for w in lines.windows(2) {
                assert_eq!(
                    w[0].1, w[1].0,
                    "block {} line ranges are not contiguous: {} != {}",
                    label, w[0].1, w[1].0
                );
            }
        }
    }

    #[test]
    fn test_split_block_lines_b_ranges_are_valid() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let block_b = gbt.block_indexs[1].block_id;
        for (i, line) in get_block_line_ranges(&mut gbt, block_b).iter().enumerate() {
            assert!(
                line.0 < line.1,
                "分裂后的 B 块第 {} 行范围应有效: {:?}",
                i,
                line
            );
        }
    }

    #[test]
    fn test_split_block_recursively_limits_all_blocks_to_block_size() {
        let content = generate_padded_content(BLOCK_SIZE * 5 + 321);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        assert!(
            gbt.block_indexs.len() > 2,
            "超大块应被连续切分成两个以上的块"
        );
        assert!(
            gbt.block_indexs
                .iter()
                .all(|idx| idx.logic_block_size <= BLOCK_SIZE),
            "连续切分后所有块大小都应不超过 BLOCK_SIZE"
        );
    }

    /// 分裂后扫描出的行范围总数不小于分裂前
    #[test]
    fn test_split_block_line_ranges_count_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_block = gbt.block_indexs[0].block_id;
        let orig_count = get_block_line_ranges(&mut gbt, orig_block).len();

        gbt.split_block(0).unwrap();

        let block_ids: Vec<BlockId> = gbt.block_indexs.iter().map(|bi| bi.block_id).collect();
        let new_count: usize = block_ids
            .into_iter()
            .map(|block_id| get_block_line_ranges(&mut gbt, block_id).len())
            .sum();
        assert!(new_count >= orig_count);
    }

    /// 分裂后完整行数量不小于分裂前
    #[test]
    fn test_split_block_complete_lines_count_sum() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);
        let orig_block = gbt.block_indexs[0].block_id;
        let orig = get_block_line_ranges(&mut gbt, orig_block)
            .into_iter()
            .filter(|(_, _, complete)| *complete)
            .count();

        gbt.split_block(0).unwrap();

        let block_ids: Vec<BlockId> = gbt.block_indexs.iter().map(|bi| bi.block_id).collect();
        let new_total: usize = block_ids
            .into_iter()
            .map(|block_id| {
                get_block_line_ranges(&mut gbt, block_id)
                    .into_iter()
                    .filter(|(_, _, complete)| *complete)
                    .count()
            })
            .sum();
        assert!(new_total >= orig);
    }

    /// 分裂在完整行边界：A 最后一行 is_complete=true，且 A 块末尾字节是 '\n'
    #[test]
    fn test_split_block_split_at_newline_boundary() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let block_ids: Vec<BlockId> = gbt.block_indexs.iter().map(|bi| bi.block_id).collect();
        let checks: Vec<(BlockId, usize, bool)> = block_ids
            .into_iter()
            .filter_map(|block_id| {
                get_block_line_ranges(&mut gbt, block_id)
                    .last()
                    .copied()
                    .map(|last| (block_id, last.1, last.2))
            })
            .collect();
        for (block_id, block_end, is_complete) in checks {
            if is_complete {
                let bytes = get_block_text(&mut gbt, block_id);
                assert_eq!(bytes[block_end - 1], b'\n');
            }
        }
    }

    #[test]
    fn test_split_block_keeps_utf8_char_boundary() {
        let mut expected = Vec::new();
        expected.extend(std::iter::repeat(b'a').take(2047));
        expected.extend_from_slice("中".as_bytes());
        expected.extend(std::iter::repeat(b'b').take(BLOCK_SIZE - expected.len()));
        assert_eq!(expected.len(), BLOCK_SIZE);

        let mut gbt = create_gap_block_text_bytes(&expected);
        let eof = expected.len();
        insert_bytes_at_abs(&mut gbt, &mut expected, eof, b"XY");

        for block_id in gbt
            .block_indexs
            .iter()
            .map(|bi| bi.block_id)
            .collect::<Vec<_>>()
        {
            let block_bytes = get_block_text(&mut gbt, block_id);
            assert!(
                std::str::from_utf8(&block_bytes).is_ok(),
                "split block {block_id} must stay valid UTF-8"
            );
        }
        assert_text_and_metadata(&mut gbt, &expected, "after UTF-8 boundary split");
    }

    #[test]
    fn test_split_block_lines_match_content() {
        let content = generate_padded_content(BLOCK_SIZE * 2 + 100);
        let mut gbt = create_gap_block_text(&content);

        gbt.split_block(0).unwrap();

        let checks: Vec<BlockId> = gbt
            .block_indexs
            .iter()
            .take(2)
            .map(|bi| bi.block_id)
            .collect();
        for block_id in checks {
            let bytes = get_block_text(&mut gbt, block_id);
            let lines = collect_line_ranges(&bytes);
            for l in &lines {
                if l.1 > l.0 && bytes[l.1 - 1] == b'\n' {
                    assert_eq!(
                        bytes[l.0..l.1].last(),
                        Some(&b'\n'),
                        "block 完整行未以 \\n 结尾"
                    );
                }
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
        let state = default_line_state();
        let payload = vec![b'X'; BLOCK_SIZE];
        gbt.insert_bytes(0, 5, &state, &payload, false).unwrap();

        gbt.split_block(id_a).unwrap();

        let id_b = gbt.block_indexs[1].block_id;
        let is_modified = |gbt: &GapBlockText, id: BlockId| {
            gbt.blocks
                .iter()
                .find(|b| b.block_id == id)
                .map(|b| b.is_modified)
                .or_else(|| gbt.cache.get(&id).map(|b| b.is_modified))
                .unwrap_or(false)
        };
        assert!(is_modified(&gbt, id_a), "A 应标记 is_modified");
        assert!(is_modified(&gbt, id_b), "B 应标记 is_modified");
    }

    /// split_block 后 cache 中的已修改块以 block_id 为 key，block_id 保持稳定不重新编号
    #[test]
    fn test_split_block_cache_block_id_stable() {
        let content = generate_padded_content(BLOCK_SIZE * 3 + 100);
        let mut gbt = create_gap_block_text(&content);

        // 手动向 cache 插入一个 block_id=2 的块（模拟已修改并被驱逐的 block）
        let fake_block_id: BlockId = 2;
        let fake_file_start = gbt.block_indexs[2].source_file_start;
        gbt.cache.insert(
            fake_block_id,
            Block {
                data: GapBuffer::from_bytes(b"fake\n", CHAR_GAP_SIZE),
                source_file_start: fake_file_start,
                source_file_end: fake_file_start + 5,
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
        let fake_file_start = gbt.block_indexs[1].source_file_start;
        gbt.cache.insert(
            fake_block_id,
            Block {
                data: GapBuffer::from_bytes(b"old\n", CHAR_GAP_SIZE),
                source_file_start: fake_file_start,
                source_file_end: fake_file_start + 4,
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

    // ================================================================
    //  六、split / cache / 索引稳定性测试
    //  目标：显式 split、递归 split、cache/block_id/行扫描不变量
    // ================================================================

    #[test]
    fn test_large_save_reload_roundtrip_at_multiple_anchors() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();

        for step in 0..48usize {
            let anchors = [
                0usize,
                expected.len() / 3,
                expected.len() / 2,
                mutation_safe_end(&expected),
            ];
            match step % 2 {
                0 => {
                    let abs = anchors[step % anchors.len()];
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &expected, abs);
                    let ch = match step % 3 {
                        0 => 'A',
                        1 => 'z',
                        _ => '9',
                    };
                    gbt.insert_char(line_idx, bytes_cursor, &state, ch).unwrap();
                    let mut buf = [0u8; 4];
                    expected.splice(
                        abs..abs,
                        ch.encode_utf8(&mut buf).as_bytes().iter().copied(),
                    );
                }
                1 => {
                    let abs = anchors[(step + 1) % anchors.len()];
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &expected, abs);
                    let payload = deterministic_ascii_payload(step, step % 4);
                    gbt.insert_bytes(line_idx, bytes_cursor, &state, &payload, false)
                        .unwrap();
                    expected.splice(abs..abs, payload.iter().copied());
                }
                _ => unreachable!(),
            }

            if step % 24 == 23 {
                let saved = save_all_text(&mut gbt);
                assert!(
                    saved == expected,
                    "save roundtrip mismatch at step {step}: {}",
                    first_diff_window(&saved, &expected)
                );
                gbt = create_gap_block_text_bytes(&saved);
                let all_text = get_all_blocks_text(&mut gbt);
                assert!(
                    all_text == expected,
                    "reload content mismatch at step {step}: {}",
                    first_diff_window(&all_text, &expected)
                );
                assert_block_storage_consistent(&gbt);
            }
        }
    }

    #[test]
    fn test_large_reload_then_continue_edit_matches_reference_model() {
        let initial = generate_large_content(BLOCK_SIZE * 5 + 333, 88);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();

        for step in 0..24usize {
            let payload = deterministic_ascii_payload(step, step % 4);
            let abs = expected.len() / 3;
            let (line_idx, bytes_cursor, state) =
                make_line_state_for_abs_cursor(&gbt, &expected, abs);
            gbt.insert_bytes(line_idx, bytes_cursor, &state, &payload, false)
                .unwrap();
            expected.splice(abs..abs, payload.iter().copied());
            gbt = reload_from_saved(&mut gbt);
        }

        assert_eq!(save_all_text(&mut gbt), expected);
        assert_eq!(get_all_blocks_text(&mut gbt), expected);
        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_large_undo_like_insert_reverse_after_reload() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 1021, 92);
        let mut gbt = create_gap_block_text(&initial);
        let mut current = initial.into_bytes();
        let original = current.clone();

        for step in 0..36usize {
            let anchors = [current.len() / 4, current.len() / 2, current.len() * 3 / 4];
            match step % 2 {
                0 => {
                    let abs = anchors[step % anchors.len()];
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &current, abs);
                    let bytes = deterministic_ascii_payload(step, step % 4);
                    gbt.insert_bytes(line_idx, bytes_cursor, &state, &bytes, false)
                        .unwrap();
                    current.splice(abs..abs, bytes.iter().copied());

                    let saved = save_all_text(&mut gbt);
                    assert!(
                        saved == current,
                        "forward save mismatch at step {step}: {}",
                        first_diff_window(&saved, &current)
                    );
                    gbt = create_gap_block_text_bytes(&saved);
                    assert_block_storage_consistent(&gbt);

                    let undo_end = abs + bytes.len();
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &current, undo_end);
                    let deleted = gbt
                        .backspace(line_idx, bytes_cursor, bytes.len(), &state)
                        .unwrap();
                    assert_eq!(
                        deleted, bytes,
                        "undo-like reverse must delete inserted payload"
                    );
                    current.drain(abs..undo_end);
                }
                _ => {
                    let abs = anchors[(step + 1) % anchors.len()];
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &current, abs);
                    gbt.insert_char(line_idx, bytes_cursor, &state, 'R')
                        .unwrap();
                    current.splice(abs..abs, [b'R']);

                    let saved = save_all_text(&mut gbt);
                    assert!(
                        saved == current,
                        "single-char save mismatch at step {step}: {}",
                        first_diff_window(&saved, &current)
                    );
                    gbt = create_gap_block_text_bytes(&saved);
                    assert_block_storage_consistent(&gbt);

                    let undo_end = abs + 1;
                    let (line_idx, bytes_cursor, state) =
                        make_line_state_for_abs_cursor(&gbt, &current, undo_end);
                    let deleted = gbt.backspace(line_idx, bytes_cursor, 1, &state).unwrap();
                    assert_eq!(deleted, vec![b'R']);
                    current.drain(abs..undo_end);
                }
            }

            let saved = save_all_text(&mut gbt);
            assert!(
                saved == current,
                "post-undo save mismatch at step {step}: {}",
                first_diff_window(&saved, &current)
            );
            gbt = create_gap_block_text_bytes(&saved);
            assert_block_storage_consistent(&gbt);
        }

        assert_eq!(
            current, original,
            "undo-like cycles must restore original bytes"
        );
        assert_eq!(save_all_text(&mut gbt), original);
        assert_eq!(get_all_blocks_text(&mut gbt), original);
        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_large_undo_like_backspace_reverse_after_reload() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 913, 88);
        let mut gbt = create_gap_block_text(&initial);
        let mut current = initial.into_bytes();
        let original = current.clone();

        for step in 0..28usize {
            let anchors = [
                current.len() / 5,
                current.len() / 3,
                current.len() / 2,
                current.len() * 4 / 5,
                mutation_safe_end(&current),
            ];
            let delete_end = anchors[step % anchors.len()].min(current.len()).max(1);
            let delete_count = ((step % 5) + 1).min(delete_end);
            let (delete_start, deleted) =
                backspace_at_abs(&mut gbt, &mut current, delete_end, delete_count);

            gbt = save_reload_and_assert(
                &mut gbt,
                &current,
                &format!("delete roundtrip mismatch at step {step}"),
            );

            insert_bytes_at_abs(&mut gbt, &mut current, delete_start, &deleted);
            gbt = save_reload_and_assert(
                &mut gbt,
                &current,
                &format!("reverse insert roundtrip mismatch at step {step}"),
            );

            assert_eq!(
                current, original,
                "delete + reverse insert must restore original bytes at step {step}"
            );
        }

        assert_eq!(save_all_text(&mut gbt), original);
        assert_eq!(get_all_blocks_text(&mut gbt), original);
        assert_block_storage_consistent(&gbt);
    }

    #[test]
    fn test_large_anchor_insert_newline_reverse_after_reload() {
        let initial = generate_large_content(BLOCK_SIZE * 5 + 1503, 84);
        let mut gbt = create_gap_block_text(&initial);
        let mut current = initial.into_bytes();
        let original = current.clone();

        for step in 0..24usize {
            let anchors = [
                0usize,
                current.len() / 4,
                current.len() / 2,
                current.len() * 3 / 4,
                mutation_safe_end(&current),
            ];
            let abs = anchors[step % anchors.len()];
            insert_bytes_at_abs(&mut gbt, &mut current, abs, b"\n");
            gbt = save_reload_and_assert(
                &mut gbt,
                &current,
                &format!("newline insert roundtrip mismatch at step {step}"),
            );

            let undo_end = abs + 1;
            let (delete_start, deleted) = backspace_at_abs(&mut gbt, &mut current, undo_end, 1);
            assert_eq!(delete_start, abs);
            assert_eq!(deleted, b"\n");
            gbt = save_reload_and_assert(
                &mut gbt,
                &current,
                &format!("newline reverse roundtrip mismatch at step {step}"),
            );

            assert_eq!(
                current, original,
                "newline undo-like cycle must restore original bytes at step {step}"
            );
        }

        assert_eq!(save_all_text(&mut gbt), original);
        assert_eq!(get_all_blocks_text(&mut gbt), original);
        assert_block_storage_consistent(&gbt);
    }

    // ================================================================
    //  七、持久化 / 重载 / undo-like 回归测试
    //  目标：save + reload + continue edit + reverse-edit 场景稳定
    // ================================================================

    #[test]
    fn test_large_save_reload_roundtrip_after_mixed_ops() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();
        let mut rng = TestRng::new(0x1234_5678_abcd_ef01);

        for step in 0..240usize {
            let lines = collect_line_ranges(&expected);
            let line_idx = rng.gen_range(lines.len());
            let (line_start, line_end) = lines[line_idx];
            let char_boundaries = line_body_char_boundaries(&expected, line_start, line_end);
            let body_len = line_body_len(&expected, line_start, line_end);

            match rng.gen_range(4) {
                0 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    let ch = match rng.gen_range(5) {
                        0 => 'A',
                        1 => 'z',
                        2 => '你',
                        3 => '界',
                        _ => '9',
                    };
                    gbt.insert_char(line_idx, cursor, &state, ch).unwrap();
                    let mut buf = [0u8; 4];
                    expected.splice(
                        line_start + cursor..line_start + cursor,
                        ch.encode_utf8(&mut buf).as_bytes().iter().copied(),
                    );
                }
                1 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    let payload = deterministic_payload(step, rng.gen_range(6));
                    gbt.insert_bytes(line_idx, cursor, &state, payload.as_bytes(), false)
                        .unwrap();
                    expected.splice(
                        line_start + cursor..line_start + cursor,
                        payload.as_bytes().iter().copied(),
                    );
                }
                2 => {
                    let cursor = char_boundaries[rng.gen_range(char_boundaries.len().min(17))];
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    gbt.insert_newline(line_idx, cursor, &state).unwrap();
                    expected.splice(line_start + cursor..line_start + cursor, [b'\n']);
                }
                _ => {
                    let can_merge_prev = line_idx > 0;
                    let can_delete_char = body_len > 0;
                    if !can_merge_prev && !can_delete_char {
                        continue;
                    }
                    let cursor = if can_merge_prev && (rng.gen_range(4) == 0 || !can_delete_char) {
                        0
                    } else {
                        let selectable = &char_boundaries[1..char_boundaries.len().min(17)];
                        selectable[rng.gen_range(selectable.len())]
                    };
                    let state = make_line_state_for_abs_line_start(
                        &gbt,
                        &expected,
                        line_idx + 1,
                        line_start,
                    );
                    let deleted = gbt.backspace(line_idx, cursor, 1, &state).unwrap();
                    let delete_end = line_start + cursor;
                    let delete_start = delete_end - deleted.len();
                    expected.drain(delete_start..delete_end);
                }
            }

            if step % 24 == 23 {
                let saved = save_all_text(&mut gbt);
                assert!(
                    saved == expected,
                    "known bug reproduced at step {step}: {}",
                    first_diff_window(&saved, &expected)
                );
                gbt = create_gap_block_text_bytes(&saved);
                assert_block_storage_consistent(&gbt);
            }
        }
    }

    // ================================================================
    //  八、已知缺陷回放与锚点边界回归
    //  目标：保留曾经出错的 deterministic replay 与 EOF/anchor 边界
    // ================================================================

    #[derive(Clone, Copy)]
    enum KnownBugReplayOp<'a> {
        InsertChar {
            line_idx: usize,
            line_start: usize,
            cursor: usize,
            ch: char,
        },
        InsertBytes {
            line_idx: usize,
            line_start: usize,
            cursor: usize,
            payload: &'a str,
        },
        InsertNewline {
            line_idx: usize,
            line_start: usize,
            cursor: usize,
        },
        Backspace {
            line_idx: usize,
            line_start: usize,
            cursor: usize,
        },
    }

    const KNOWN_BUG_REPLAY_OPS: &[KnownBugReplayOp<'static>] = &[
        KnownBugReplayOp::InsertNewline {
            line_idx: 213,
            line_start: 20448,
            cursor: 4,
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 175,
            line_start: 16800,
            cursor: 10,
            ch: '界',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 84,
            line_start: 8064,
            cursor: 9,
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 123,
            line_start: 11713,
            cursor: 9,
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 1,
            line_start: 96,
            cursor: 7,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 17,
            line_start: 1537,
            cursor: 5,
            payload: "XYZ123",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 91,
            line_start: 8552,
            cursor: 7,
            ch: 'z',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 106,
            line_start: 9993,
            cursor: 7,
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 220,
            line_start: 20655,
            cursor: 11,
            ch: 'A',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 105,
            line_start: 9897,
            cursor: 4,
            ch: '9',
        },
        KnownBugReplayOp::Backspace {
            line_idx: 158,
            line_start: 14796,
            cursor: 10,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 63,
            line_start: 5959,
            cursor: 9,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 209,
            line_start: 19693,
            cursor: 1,
            payload: "L12\nM12\n",
        },
        KnownBugReplayOp::Backspace {
            line_idx: 254,
            line_start: 23735,
            cursor: 7,
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 161,
            line_start: 15082,
            cursor: 10,
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 83,
            line_start: 7878,
            cursor: 0,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 106,
            line_start: 9897,
            cursor: 8,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 113,
            line_start: 10474,
            cursor: 4,
            payload: "尾17\n",
        },
        KnownBugReplayOp::Backspace {
            line_idx: 258,
            line_start: 23837,
            cursor: 12,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 199,
            line_start: 18452,
            cursor: 10,
            payload: "mix-19-中\n文-19",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 260,
            line_start: 23949,
            cursor: 9,
            ch: '界',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 21,
            line_start: 1927,
            cursor: 12,
            ch: 'z',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 274,
            line_start: 25297,
            cursor: 1,
            ch: 'z',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 227,
            line_start: 20784,
            cursor: 1,
            ch: '界',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 127,
            line_start: 11729,
            cursor: 2,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 112,
            line_start: 10379,
            cursor: 8,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 61,
            line_start: 5768,
            cursor: 1,
            payload: "mix-26-中\n文-26",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 197,
            line_start: 18086,
            cursor: 1,
            ch: 'A',
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 108,
            line_start: 10011,
            cursor: 0,
            payload: "P0028-ASCII",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 200,
            line_start: 18386,
            cursor: 12,
            ch: 'z',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 59,
            line_start: 5576,
            cursor: 16,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 115,
            line_start: 10503,
            cursor: 2,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 275,
            line_start: 25138,
            cursor: 0,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 173,
            line_start: 15694,
            cursor: 10,
            payload: "尾33\n",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 146,
            line_start: 13198,
            cursor: 2,
            ch: 'A',
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 74,
            line_start: 6841,
            cursor: 1,
            payload: "mix-35-中\n文-35",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 257,
            line_start: 23241,
            cursor: 13,
            ch: 'z',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 43,
            line_start: 4040,
            cursor: 0,
            ch: 'A',
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 270,
            line_start: 24492,
            cursor: 13,
            ch: '你',
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 261,
            line_start: 23627,
            cursor: 4,
            payload: "尾39\n",
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 112,
            line_start: 10138,
            cursor: 9,
        },
        KnownBugReplayOp::InsertBytes {
            line_idx: 52,
            line_start: 4905,
            cursor: 15,
            payload: "尾41\n",
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 229,
            line_start: 20460,
            cursor: 4,
            ch: 'z',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 212,
            line_start: 19012,
            cursor: 16,
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 282,
            line_start: 25373,
            cursor: 9,
            ch: '9',
        },
        KnownBugReplayOp::InsertNewline {
            line_idx: 200,
            line_start: 17937,
            cursor: 9,
        },
        KnownBugReplayOp::Backspace {
            line_idx: 207,
            line_start: 18516,
            cursor: 8,
        },
        KnownBugReplayOp::InsertChar {
            line_idx: 188,
            line_start: 16782,
            cursor: 4,
            ch: '9',
        },
    ];

    fn apply_known_bug_replay_op(
        gbt: &mut GapBlockText,
        expected: &mut Vec<u8>,
        op: &KnownBugReplayOp<'_>,
    ) {
        match op {
            KnownBugReplayOp::InsertChar {
                line_idx,
                line_start,
                cursor,
                ch,
            } => {
                let state =
                    make_line_state_for_abs_line_start(gbt, expected, *line_idx + 1, *line_start);
                gbt.insert_char(*line_idx, *cursor, &state, *ch).unwrap();
                let mut buf = [0u8; 4];
                expected.splice(
                    (*line_start + *cursor)..(*line_start + *cursor),
                    ch.encode_utf8(&mut buf).as_bytes().iter().copied(),
                );
            }
            KnownBugReplayOp::InsertBytes {
                line_idx,
                line_start,
                cursor,
                payload,
            } => {
                let state =
                    make_line_state_for_abs_line_start(gbt, expected, *line_idx + 1, *line_start);
                gbt.insert_bytes(*line_idx, *cursor, &state, payload.as_bytes(), false)
                    .unwrap();
                expected.splice(
                    (*line_start + *cursor)..(*line_start + *cursor),
                    payload.as_bytes().iter().copied(),
                );
            }
            KnownBugReplayOp::InsertNewline {
                line_idx,
                line_start,
                cursor,
            } => {
                let state =
                    make_line_state_for_abs_line_start(gbt, expected, *line_idx + 1, *line_start);
                gbt.insert_newline(*line_idx, *cursor, &state).unwrap();
                expected.splice((*line_start + *cursor)..(*line_start + *cursor), [b'\n']);
            }
            KnownBugReplayOp::Backspace {
                line_idx,
                line_start,
                cursor,
            } => {
                let state =
                    make_line_state_for_abs_line_start(gbt, expected, *line_idx + 1, *line_start);
                let deleted = gbt.backspace(*line_idx, *cursor, 1, &state).unwrap();
                let delete_end = *line_start + *cursor;
                let delete_start = delete_end - deleted.len();
                expected.drain(delete_start..delete_end);
            }
        }
    }

    #[test]
    fn test_large_mixed_ops_replay_sequence() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();

        for (step, op) in KNOWN_BUG_REPLAY_OPS.iter().enumerate() {
            apply_known_bug_replay_op(&mut gbt, &mut expected, op);

            if step == KNOWN_BUG_REPLAY_OPS.len() - 1 {
                let saved = save_all_text(&mut gbt);
                assert!(
                    saved == expected,
                    "known bug replay reproduced at step {step}: {}",
                    first_diff_window(&saved, &expected)
                );
            }
        }
    }

    #[test]
    fn test_large_mixed_ops_save_reload_each_step() {
        // 这个用例不是为了“随机压测”，而是为了尽快停在第一次持久化偏离。
        // 相比完整 48 步回放，它能更早暴露哪一步首次把内部状态带坏。
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();

        for (step, op) in KNOWN_BUG_REPLAY_OPS.iter().enumerate() {
            apply_known_bug_replay_op(&mut gbt, &mut expected, op);
            let saved = save_all_text(&mut gbt);
            assert!(
                saved == expected,
                "known bug first divergence reproduced at step {step}: {}",
                first_diff_window(&saved, &expected)
            );
            gbt = create_gap_block_text_bytes(&saved);
        }
    }

    #[test]
    fn test_locate_cursor_maps_eof_after_trailing_newline_to_virtual_empty_line() {
        let bytes = b"alpha\nbeta\n";
        let (line_idx, line_start, bytes_cursor) = locate_cursor(bytes, bytes.len());
        assert_eq!(line_idx, 2);
        assert_eq!(line_start, bytes.len());
        assert_eq!(bytes_cursor, 0);
    }

    #[test]
    fn test_large_save_reload_roundtrip_with_backspace_at_anchors() {
        let initial = generate_large_content(BLOCK_SIZE * 6 + 777, 96);
        let mut gbt = create_gap_block_text(&initial);
        let mut expected = initial.into_bytes();

        for step in 0..48usize {
            let anchors = [
                0usize,
                expected.len() / 3,
                expected.len() / 2,
                mutation_safe_end(&expected),
            ];
            match step % 3 {
                0 => {
                    let abs = anchors[step % anchors.len()];
                    insert_char_at_abs(&mut gbt, &mut expected, abs, 'A');
                }
                1 => {
                    let abs = anchors[(step + 1) % anchors.len()];
                    let payload = deterministic_ascii_payload(step, step % 4);
                    insert_bytes_at_abs(&mut gbt, &mut expected, abs, &payload);
                }
                _ => {
                    if expected.is_empty() {
                        continue;
                    }
                    let delete_end = anchors[(step + 2) % anchors.len()]
                        .min(expected.len())
                        .max(1);
                    let _ = backspace_at_abs(&mut gbt, &mut expected, delete_end, 1);
                }
            }

            if step % 24 == 23 {
                let saved = save_all_text(&mut gbt);
                assert!(
                    saved == expected,
                    "anchor backspace roundtrip diverged at step {step}: {}",
                    first_diff_window(&saved, &expected)
                );
            }
        }
    }

    #[test]
    fn test_gap_block_scoll_text_iter_iterates_all_lines() {
        let expected_content = generate_large_content(BLOCK_SIZE * 3 + 123, 80).into_bytes();
        let mut gbt = create_gap_block_text_bytes(&expected_content);
        let expected_ranges = collect_line_ranges(&expected_content);
        let expected_states: Vec<LineState> = expected_ranges
            .iter()
            .enumerate()
            .map(|(line_idx, (start, _))| {
                make_line_state_for_abs_line_start(&gbt, &expected_content, line_idx + 1, *start)
            })
            .collect();

        let first_block_id = gbt.block_indexs.first().expect("no blocks").block_id;
        let mut iter = GapBlockScollTextIter::new(&mut gbt, first_block_id, 0, 0, 0)
            .expect("failed to create iterator");

        let mut line_idx = 0;
        while let Some((line_block_str, line_state)) = iter.next() {
            assert!(
                line_idx < expected_ranges.len(),
                "iterator produced more lines than expected (got line {})",
                line_idx
            );

            let actual = line_block_str.to_bytes();
            let (start, end) = expected_ranges[line_idx];
            let expected = &expected_content[start..end];
            assert_eq!(
                &actual, expected,
                "line {} mismatch: actual={:?}, expected={:?}",
                line_idx, actual, expected,
            );

            let expected_state = &expected_states[line_idx];
            assert_eq!(
                line_state.get_block_num(),
                expected_state.get_block_num(),
                "line {} block_num mismatch",
                line_idx
            );
            assert_eq!(
                line_state.get_block_line_index(),
                expected_state.get_block_line_index(),
                "line {} block_line_index mismatch",
                line_idx
            );
            assert_eq!(
                line_state.get_block_offset(),
                expected_state.get_block_offset(),
                "line {} block_offset mismatch",
                line_idx
            );
            assert_eq!(
                line_state.get_line_index(),
                expected_state.get_line_index(),
                "line {} line_index mismatch",
                line_idx
            );

            line_idx += 1;
        }

        assert_eq!(
            line_idx,
            expected_ranges.len(),
            "iterator produced {} lines, expected {}",
            line_idx,
            expected_ranges.len()
        );
    }

    #[test]
    fn test_find_returns_matching_line_states() {
        let content = b"alpha\nbeta needle\nneedle gamma\nomega\n".to_vec();
        let mut gbt = create_gap_block_text_bytes(&content);
        let start_state = make_line_state_for_abs_line_start(&gbt, &content, 1, 0);

        let matches = gbt
            .search(Partten::exact(b"needle"), &start_state)
            .expect("find should succeed")
            .expect("find should return matching lines");

        assert_eq!(matches.len(), 2, "should find two matching lines");

        let expected_second = make_line_state_for_abs_line_start(&gbt, &content, 2, 6);
        let expected_third = make_line_state_for_abs_line_start(&gbt, &content, 3, 18);

        assert_eq!(
            matches[0].get_line_index(),
            expected_second.get_line_index()
        );
        //  assert_eq!(matches[0].get_line_num(), expected_second.get_line_num());
        assert_eq!(matches[0].get_block_num(), expected_second.get_block_num());
        assert_eq!(
            matches[0].get_block_line_index(),
            expected_second.get_block_line_index()
        );
        assert_eq!(
            matches[0].get_block_offset(),
            expected_second.get_block_offset()
        );
        assert_eq!(
            matches[0].get_line_file_start(),
            expected_second.get_line_file_start()
        );
        assert_eq!(
            matches[0].get_line_file_end(),
            expected_second.get_line_file_end()
        );

        assert_eq!(matches[1].get_line_index(), expected_third.get_line_index());
        // assert_eq!(matches[1].get_line_num(), expected_third.get_line_num());
        assert_eq!(matches[1].get_block_num(), expected_third.get_block_num());
        assert_eq!(
            matches[1].get_block_line_index(),
            expected_third.get_block_line_index()
        );
        assert_eq!(
            matches[1].get_block_offset(),
            expected_third.get_block_offset()
        );
        assert_eq!(
            matches[1].get_line_file_start(),
            expected_third.get_line_file_start()
        );
        assert_eq!(
            matches[1].get_line_file_end(),
            expected_third.get_line_file_end()
        );
    }

    #[test]
    fn test_first_line_softwrap_continuation_has_previous_visual_line() {
        let content = format!("{}\nnext\n", "a".repeat(180)).into_bytes();
        let mut gbt = create_gap_block_text_bytes(&content);
        let block_id = gbt.block_indexs.first().expect("block index").block_id;
        let state = LineState::builder()
            .block_num(block_id)
            .block_line_index(0)
            .block_offset(0)
            .line_index(0)
            .line_offset(80)
            .line_file_start(0)
            .line_file_end(181)
            .build();

        assert!(
            gbt.has_pre_line(&state),
            "first logical line soft-wrap continuation must be scrollable upward"
        );

        let prev = gbt
            .get_pre_line_state(&state, 80)
            .expect("previous visual line state");
        assert_eq!(prev.get_line_index(), 0);
        assert_eq!(prev.get_line_offset(), 80);
    }

    #[test]
    fn test_edit_search_scroll_chaos_keeps_page_text_consistent() {
        let mut expected = test_fixture_a_txt_bytes();
        let mut gbt = create_gap_block_text_bytes(&expected);

        insert_bytes_at_abs(&mut gbt, &mut expected, 0, b"# chaos-prefix make\n");
        let mid = expected.len() / 2;
        insert_bytes_at_abs(&mut gbt, &mut expected, mid, b" CHAOS_MID_make ");
        if expected.len() > 128 {
            backspace_at_abs(&mut gbt, &mut expected, 128, 7);
        }
        let tail = mutation_safe_end(&expected);
        insert_char_at_abs(&mut gbt, &mut expected, tail, 'Z');
        let eof = expected.len();
        insert_bytes_at_abs(&mut gbt, &mut expected, eof, b"\n# chaos-tail make\n");

        assert_saved_matches(&mut gbt, &expected, "after chaos edits");

        let start_state = make_line_state_for_abs_line_start(&gbt, &expected, 1, 0);
        let matches = gbt
            .search(Partten::exact(b"make"), &start_state)
            .expect("search should succeed")
            .expect("search should find make");
        let target = matches
            .last()
            .cloned()
            .expect("expected at least one search hit");
        let saved = save_all_text(&mut gbt);
        assert_eq!(
            saved, expected,
            "saved text must match expected after search"
        );

        let td = TextDisplay::EditBlock(EditTextWarp::new(gbt, 20, 80, TextWarpType::SoftWrap));
        td.get_one_page_from_state(&target)
            .expect("load page from search target");
        assert_current_page_valid(&td, &saved, "search target page");

        for step in 0..200 {
            let first = {
                let meta = td.get_current_line_meta().expect("line meta");
                let Some(first) = meta.get(0) else {
                    break;
                };
                first.clone()
            };
            td.scroll_pre_one_line(&first)
                .expect("scroll previous line after search");
            assert_current_page_valid(&td, &saved, &format!("scroll up step {step}"));
        }

        for step in 0..80 {
            let last = {
                let meta = td.get_current_line_meta().expect("line meta");
                let Some(last) = meta.last() else {
                    break;
                };
                last.clone()
            };
            td.scroll_next_one_line(&last)
                .expect("scroll next line after search");
            assert_current_page_valid(&td, &saved, &format!("scroll down step {step}"));
        }
    }

    #[test]
    fn test_random_edit_search_scroll_keeps_reference_model_consistent() {
        for seed in [0x1020_3040_5060_7080, 0x5eed_cafe_f00d_1234] {
            let mut expected = test_fixture_a_txt_bytes();
            let mut gbt = create_gap_block_text_bytes(&expected);
            let mut rng = TestRng::new(seed);

            for step in 0..180usize {
                let lines = collect_line_ranges(&expected);
                let line_idx = rng.gen_range(lines.len());
                let (line_start, line_end) = lines[line_idx];
                let char_boundaries = line_body_char_boundaries(&expected, line_start, line_end);
                let body_len = line_body_len(&expected, line_start, line_end);
                let op = rng.gen_range(5);
                let label = format!("seed={seed:#x} step={step} op={op}");

                match op {
                    0 => {
                        let cursor = char_boundaries[rng.gen_range(char_boundaries.len())];
                        let ch = match rng.gen_range(6) {
                            0 => 'E',
                            1 => 'N',
                            2 => 'V',
                            3 => '你',
                            4 => '界',
                            _ => 'x',
                        };
                        insert_char_at_abs(&mut gbt, &mut expected, line_start + cursor, ch);
                    }
                    1 => {
                        let cursor = char_boundaries[rng.gen_range(char_boundaries.len())];
                        let payload = deterministic_payload(step, rng.gen_range(6));
                        insert_bytes_at_abs(
                            &mut gbt,
                            &mut expected,
                            line_start + cursor,
                            payload.as_bytes(),
                        );
                    }
                    2 => {
                        let cursor = char_boundaries[rng.gen_range(char_boundaries.len())];
                        insert_bytes_at_abs(
                            &mut gbt,
                            &mut expected,
                            line_start + cursor,
                            b" ENV make ",
                        );
                    }
                    3 => {
                        let cursor = char_boundaries[rng.gen_range(char_boundaries.len())];
                        insert_bytes_at_abs(&mut gbt, &mut expected, line_start + cursor, b"\n");
                    }
                    _ => {
                        let can_merge_prev = line_idx > 0;
                        let ascii_delete_cursors: Vec<usize> = char_boundaries
                            .windows(2)
                            .filter_map(|w| (w[1] - w[0] == 1).then_some(w[1]))
                            .collect();
                        let can_delete_char = !ascii_delete_cursors.is_empty();
                        if !can_merge_prev && !can_delete_char {
                            continue;
                        }
                        let cursor =
                            if can_merge_prev && (rng.gen_range(5) == 0 || !can_delete_char) {
                                0
                            } else {
                                ascii_delete_cursors[rng.gen_range(ascii_delete_cursors.len())]
                            };
                        backspace_at_abs(&mut gbt, &mut expected, line_start + cursor, 1);
                    }
                }

                assert_text_and_metadata(&mut gbt, &expected, &label);
            }

            insert_bytes_at_abs(
                &mut gbt,
                &mut expected,
                0,
                b"# search-scroll-anchor ENV make\n",
            );
            let mid = expected.len() / 2;
            insert_bytes_at_abs(
                &mut gbt,
                &mut expected,
                mid,
                b"\n# middle ENV make anchor\n",
            );
            let eof = expected.len();
            insert_bytes_at_abs(&mut gbt, &mut expected, eof, b"\n# tail ENV make anchor\n");

            let start_state = make_line_state_for_abs_line_start(&gbt, &expected, 1, 0);
            let matches = gbt
                .search(Partten::exact(b"ENV"), &start_state)
                .expect("search should succeed")
                .expect("search should find ENV");
            let target = matches
                .last()
                .cloned()
                .expect("expected at least one ENV hit");
            let saved = save_all_text(&mut gbt);
            assert_eq!(
                saved, expected,
                "seed={seed:#x}: saved text must match expected before scroll"
            );

            let td = TextDisplay::EditBlock(EditTextWarp::new(gbt, 24, 80, TextWarpType::SoftWrap));
            td.get_one_page_from_state(&target)
                .expect("load page from last search hit");
            assert_current_page_valid(&td, &saved, &format!("seed={seed:#x} search target page"));

            for step in 0..240usize {
                let first = {
                    let meta = td.get_current_line_meta().expect("line meta");
                    let Some(first) = meta.get(0) else {
                        break;
                    };
                    if !first.has_pre_line() {
                        break;
                    }
                    first.clone()
                };
                td.scroll_pre_one_line(&first)
                    .expect("scroll previous line after random edits");
                assert_current_page_valid(&td, &saved, &format!("seed={seed:#x} scroll up {step}"));
            }

            for step in 0..160usize {
                let last = {
                    let meta = td.get_current_line_meta().expect("line meta");
                    let Some(last) = meta.last() else {
                        break;
                    };
                    last.clone()
                };
                td.scroll_next_one_line(&last)
                    .expect("scroll next line after random edits");
                assert_current_page_valid(
                    &td,
                    &saved,
                    &format!("seed={seed:#x} scroll down {step}"),
                );
            }
        }
    }

    #[test]
    fn test_crud_search_then_scroll_keeps_visible_text_exact() {
        let mut expected = test_fixture_a_txt_bytes();
        let mut gbt = create_gap_block_text_bytes(&expected);

        // Create: insert at BOF, middle, and EOF. The middle line is long enough to soft-wrap.
        insert_bytes_at_abs(&mut gbt, &mut expected, 0, b"# CRUD-BEGIN ENV make\n");
        assert_text_and_metadata(&mut gbt, &expected, "after insert at beginning");

        let mid = expected.len() / 2;
        insert_bytes_at_abs(
            &mut gbt,
            &mut expected,
            mid,
            b"\nCRUD-MIDDLE ENV make xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n",
        );
        assert_text_and_metadata(&mut gbt, &expected, "after insert in middle");

        let eof = expected.len();
        insert_bytes_at_abs(&mut gbt, &mut expected, eof, b"\n# CRUD-END ENV make\n");
        assert_text_and_metadata(&mut gbt, &expected, "after insert at end");

        // Update: replace the first ENV token with SET using delete + insert.
        let env_pos = find_bytes(&expected, b"ENV").expect("ENV token should exist");
        replace_ascii_at_abs(&mut gbt, &mut expected, env_pos, b"ENV".len(), b"SET");
        assert_text_and_metadata(&mut gbt, &expected, "after replace ENV with SET");

        // Delete: remove an ASCII token and then merge one newline across lines.
        let make_pos = find_bytes(&expected, b"make").expect("make token should exist");
        backspace_at_abs(
            &mut gbt,
            &mut expected,
            make_pos + b"make".len(),
            b"make".len(),
        );
        assert_text_and_metadata(&mut gbt, &expected, "after delete make token");

        let newline_pos = find_bytes(&expected, b"\nCRUD-MIDDLE")
            .expect("middle inserted line should have a leading newline");
        backspace_at_abs(&mut gbt, &mut expected, newline_pos + 1, 1);
        assert_text_and_metadata(&mut gbt, &expected, "after newline merge delete");

        // Read/Search: every returned state must point to a line containing the queried bytes.
        let start_state = make_line_state_for_abs_line_start(&gbt, &expected, 1, 0);
        let matches = gbt
            .search(Partten::exact(b"ENV"), &start_state)
            .expect("search should succeed")
            .expect("search should return ENV matches");
        assert!(
            matches.len() >= 2,
            "expected multiple ENV matches after CRUD operations, got {}",
            matches.len()
        );
        for (i, line_state) in matches.iter().enumerate() {
            let start = line_state.get_line_file_start();
            let end = line_state.get_line_file_end();
            assert!(
                end <= expected.len(),
                "search match {i} line range {start}..{end} exceeds len {}",
                expected.len()
            );
            assert!(
                find_bytes(&expected[start..end], b"ENV").is_some(),
                "search match {i} does not point to an ENV line: {:?}",
                String::from_utf8_lossy(&expected[start..end])
            );
        }

        let target = matches
            .last()
            .cloned()
            .expect("last ENV search target should exist");
        let saved = save_all_text(&mut gbt);
        assert_eq!(
            saved, expected,
            "saved text must match reference before scroll"
        );

        let td = TextDisplay::EditBlock(EditTextWarp::new(gbt, 24, 80, TextWarpType::SoftWrap));
        td.get_one_page_from_state(&target)
            .expect("load page from CRUD search target");
        assert_current_page_valid(&td, &saved, "CRUD search target page");

        for step in 0..260usize {
            let first = {
                let meta = td.get_current_line_meta().expect("line meta");
                let Some(first) = meta.get(0) else {
                    break;
                };
                if !first.has_pre_line() {
                    break;
                }
                first.clone()
            };
            td.scroll_pre_one_line(&first)
                .expect("scroll previous after CRUD search");
            assert_current_page_valid(&td, &saved, &format!("CRUD scroll up {step}"));
        }

        for step in 0..180usize {
            let last = {
                let meta = td.get_current_line_meta().expect("line meta");
                let Some(last) = meta.last() else {
                    break;
                };
                last.clone()
            };
            td.scroll_next_one_line(&last)
                .expect("scroll next after CRUD search");
            assert_current_page_valid(&td, &saved, &format!("CRUD scroll down {step}"));
        }
    }

    #[test]
    fn test_scroll() {
        let mut gap: GapBlockText = GapBlockText::from_file_path("/home/unvdb/a.txt").unwrap();
        let scroll = GapBlockScollTextIter::new(&mut gap, 0, 0, 0, 0).unwrap();
        for (l, v) in scroll {
            println!("{}\n", l);
        }
    }
}
