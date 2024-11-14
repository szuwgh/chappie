use memmap2::Mmap;
use std::fs::File;
use std::io;
// 使用 mmap 映射文件到内存
pub(crate) fn map_file(path: &str) -> io::Result<Mmap> {
    let file = File::open(path)?;
    let mmap = unsafe { Mmap::map(&file)? };
    Ok(mmap)
}
