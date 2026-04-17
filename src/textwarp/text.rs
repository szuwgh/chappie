use crate::common::util::mmap_file;
use crate::textwarp::ChapResult;
use crate::textwarp::GapBytes;
use crate::textwarp::LineData;
use crate::textwarp::LineState;
use crate::textwarp::LineStr;
use crate::textwarp::Path;
use crate::textwarp::Text;
use crate::textwarp::TextIndex;
use crate::textwarp::TextSelect;
use memmap2::Mmap;
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
            data: LineData::GapBytes(GapBytes::new(line, &[])),
            block_id: 0,
            block_offset: 0,
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

impl TextIndex for MmapText {
    fn get_page_offset(&self, line_num: usize) -> LineState {
        LineState::builder().build()
    }

    fn set_page_offset(&mut self, page_num: usize, page_offset: LineState) {
        // MmapText 不支持分页偏移
    }
}

impl Text for MmapText {
    type LineItem<'a> = LineStr<'a>;
    fn get_file_size(&self) -> usize {
        self.mmap.len()
    }

    fn text_from_sel(&self, sel: &TextSelect) -> Vec<u8> {
        todo!("Not implement text_from_sel for MmapText");
    }

    fn get_line<'a>(&'a self, state: &LineState) -> Option<LineStr<'a>> {
        let line = &self.mmap[state.line_file_start..state.line_file_end];
        Some(LineStr {
            data: LineData::GapBytes(GapBytes::new(line, &[])),
            block_id: state.block_num,
            block_offset: state.block_offset,
            line_file_start: state.line_file_start,
            line_file_end: state.line_file_end,
        })
    }

    fn has_pre_line(&self, meta: &LineState) -> bool {
        if meta.get_line_file_start() == 0 && meta.get_line_offset() == 0 {
            return false;
        }
        true
    }

    fn get_next_line_state(&self, state: &LineState) -> Option<LineState> {
        None
    }

    fn get_pre_line_state(&self, state: &LineState) -> Option<LineState> {
        None
    }

    fn get_line_text_len(&self, line_index: usize, line_start: usize, line_end: usize) -> usize {
        line_end - line_start
    }

    fn has_next_line(&self, meta: &LineState) -> bool {
        if meta.get_line_file_start() + meta.get_line_offset() + meta.get_txt_len()
            >= self.mmap.len() - 1
        {
            return false;
        }
        true
    }

    fn iter<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        MmapTextIter::new(
            &self.mmap,
            line_state.line_index,
            line_state.line_file_start,
            self.mmap.len(),
        )
    }

    fn iter_rev<'a>(&'a mut self, line_state: &LineState) -> impl Iterator<Item = LineStr<'a>> {
        self.iter(line_state)
    }

    fn iter_u8<'a>(
        &'a mut self,
        line_index: usize,
        line_offset: usize,
        line_file_start: usize,
    ) -> impl Iterator<Item = u8> {
        Vec::new().into_iter() // MmapText 不支持 u8 迭代
    }

    fn find(&mut self, state: &LineState, partten: &[u8]) -> ChapResult<Option<Vec<LineState>>> {
        Ok(None)
    }
}
