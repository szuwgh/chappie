//! 第五轮：16KB+ 大文件 SoftWrap 全操作测试
//!
//! 本模块通过构造 >16KB 的文件，测试以下场景下是否存在 panic 或光标越界：
//!
//! 1. 大文件滚动导航（连续向下/向上超过多个 4KB 块）
//! 2. SoftWrap 超长行（单行 > TV_W）的左右移动
//! 3. 在 4KB 块边界附近执行插入、删除、换行
//! 4. 粘贴 >4KB / >16KB 的大块数据
//! 5. 连续编辑使文件从 ~15KB 增长至超过 16KB
//! 6. 混合操作压力测试
//! 7. 滚动后执行编辑（start_line_num 与 scroll 的配合）

use crate::handle::edit::HandleEdit;
use crate::handle::Handle;
use crate::textwarp::edit_block::GapBlockText;
use crate::textwarp::{EditTextWarp, TextDisplay, TextOper, TextWarpType};
use crate::tui::ChapTui;
use std::io::Write;
use tempfile::NamedTempFile;

const TV_H: usize = 20;
const TV_W: usize = 80;

// ─── 测试辅助 ─────────────────────────────────────────────────────────────────

fn setup(content: &str) -> (ChapTui, TextDisplay, NamedTempFile) {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(content.as_bytes()).unwrap();
    tmp.flush().unwrap();
    let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
    let mut td = TextDisplay::EditBlock(EditTextWarp::new(gap, TV_H, TV_W, TextWarpType::SoftWrap));
    td.get_one_page(1).unwrap();
    let tui = ChapTui::for_test(TV_H, TV_W);
    (tui, td, tmp)
}

fn h() -> HandleEdit {
    HandleEdit::new()
}

// ─── 辅助：生成大文件内容 ─────────────────────────────────────────────────────

/// 210行 × 81字节 ≈ 17010字节（> 16384 = 16KB）
fn content_17kb() -> String {
    ("a".repeat(80) + "\n").repeat(210)
}

/// 400行 × 41字节 = 16400字节，横跨 4 个 4KB 块
fn content_4blocks() -> String {
    ("b".repeat(40) + "\n").repeat(400)
}

/// 单行超宽：TV_W*2+20 字符（触发 3 个视觉段）+ 5 个普通行
fn content_long_line() -> String {
    "c".repeat(TV_W * 2 + 20) + "\n" + &"pad\n".repeat(5)
}

/// 接近 16KB 的文件：190行 × 80字节 = 15200字节
fn content_near_16kb() -> String {
    ("a".repeat(79) + "\n").repeat(190)
}

// ═══════════════════════════════════════════════════════════════════════════════
// 1. 大文件滚动导航
// ═══════════════════════════════════════════════════════════════════════════════

/// 17KB 文件，连续向下滚动 300 次，不应 panic，cursor_y <= TV_H
#[test]
fn large_scroll_down_300_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..300 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    assert!(
        tui.cursor_y <= TV_H,
        "向下300次后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

/// 向下 200 次再向上 200 次，不应 panic，cursor_y <= TV_H
#[test]
fn large_scroll_down_then_up_200_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..200 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    for _ in 0..200 {
        h().handle_up(&mut tui, meta, &td).unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

/// 每步向下都检查 cursor_y 不超出 TV_H（TV_H*10 次）
#[test]
fn large_scroll_down_cursor_y_bounded_every_step() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    for i in 0..(TV_H * 10) {
        h().handle_down(&mut tui, meta, &td).unwrap();
        assert!(
            tui.cursor_y <= TV_H,
            "第{}次向下 cursor_y={} 超出 TV_H={}",
            i,
            tui.cursor_y,
            TV_H
        );
    }
}

/// 跨 4 个块来回滚动（各 380 次），不应 panic
#[test]
fn four_blocks_scroll_back_and_forth_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..380 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    for _ in 0..380 {
        h().handle_up(&mut tui, meta, &td).unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

/// 大文件向下时 cursor_x 在每步均不超出 TV_W
#[test]
fn large_scroll_cursor_x_bounded_every_step() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    tui.cursor_x = TV_W / 2;
    for i in 0..200 {
        h().handle_down(&mut tui, meta, &td).unwrap();
        assert!(
            tui.cursor_x <= TV_W,
            "第{}次向下后 cursor_x={} 超出 TV_W={}",
            i,
            tui.cursor_x,
            TV_W
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 2. SoftWrap 超长行左右移动
// ═══════════════════════════════════════════════════════════════════════════════

/// 超长行向右移动 TV_W*3 次，cursor_y 应跨视觉段增加（Bug 12 已修复）
#[test]
fn long_line_right_no_panic_cursor_y_recorded() {
    let (mut tui, td, _f) = setup(&content_long_line());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..(TV_W * 3) {
        h().handle_right(&mut tui, meta, &td).unwrap();
    }
    assert!(
        tui.cursor_y >= 1,
        "cursor_y({}) 应 >= 1，右移 TV_W*3 次应跨视觉段",
        tui.cursor_y
    );
    assert!(
        tui.cursor_y <= TV_H,
        "cursor_y({}) 不应超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

/// 超长行向右移动过程中 cursor_x 不应超出 TV_W
#[test]
fn long_line_right_cursor_x_never_exceeds_tv_width() {
    let (mut tui, td, _f) = setup(&content_long_line());
    let meta = td.get_current_line_meta().unwrap();
    for i in 0..(TV_W * 3) {
        h().handle_right(&mut tui, meta, &td).unwrap();
        assert!(
            tui.cursor_x <= TV_W,
            "第{}次右移 cursor_x={} 超出 TV_W={}",
            i,
            tui.cursor_x,
            TV_W
        );
    }
}

/// 超长行：先右移 TV_W*2 步再左移 TV_W*2 步，不应 panic
#[test]
fn long_line_right_then_left_no_panic() {
    let (mut tui, td, _f) = setup(&content_long_line());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..(TV_W * 2) {
        h().handle_right(&mut tui, meta, &td).unwrap();
    }
    let y_peak = tui.cursor_y;
    for _ in 0..(TV_W * 2) {
        h().handle_left(&mut tui, meta, &td).unwrap();
    }
    assert!(
        tui.cursor_y <= y_peak,
        "左移后 cursor_y({}) 不应超出右移峰值({})",
        tui.cursor_y,
        y_peak
    );
}

/// 超长行左右移动过程中 cursor_x 均不超出 TV_W
#[test]
fn long_line_left_cursor_x_never_exceeds_tv_width() {
    let (mut tui, td, _f) = setup(&content_long_line());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..(TV_W + 10) {
        h().handle_right(&mut tui, meta, &td).unwrap();
    }
    for i in 0..(TV_W + 10) {
        h().handle_left(&mut tui, meta, &td).unwrap();
        assert!(
            tui.cursor_x <= TV_W,
            "第{}次左移 cursor_x={} 超出 TV_W={}",
            i,
            tui.cursor_x,
            TV_W
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// 3. 4KB 块边界附近的 handle_char
// ═══════════════════════════════════════════════════════════════════════════════

/// 在第 100 行附近（接近 4KB 边界）连续插入 10 字符，不应 panic
#[test]
fn char_insert_near_4kb_boundary_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.cursor_x = 5;
    tui.bytes_cursor = 5;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..10 {
        h().handle_char(&mut tui, meta2, &td, 'X').unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

/// 从 ~15KB 文件连续插入 1000 字符，文件超过 16KB，不应 panic
#[test]
fn char_insert_grow_file_past_16kb_no_panic() {
    let (mut tui, td, _f) = setup(&content_near_16kb());
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..1000 {
        h().handle_char(&mut tui, meta, &td, 'Z').unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
    assert!(tui.cursor_x <= TV_W);
}

/// 向小文件连续插入 5000 字符（>4KB），触发块分裂，不应 panic
#[test]
fn char_insert_5000_triggers_block_split_no_panic() {
    let (mut tui, td, _f) = setup("start\n");
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..5000 {
        h().handle_char(&mut tui, meta, &td, 'A').unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

/// handle_char 插入直至触发多次视觉折行，cursor_y 不超出 TV_H
#[test]
fn char_insert_wraps_visual_line_cursor_y_bounded() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..(TV_W * 3) {
        h().handle_char(&mut tui, meta, &td, 'K').unwrap();
    }
    assert!(
        tui.cursor_y <= TV_H,
        "连续插入折行后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 4. 4KB 块边界附近的 handle_backspace
// ═══════════════════════════════════════════════════════════════════════════════

/// 在第 100 行附近连续 backspace 20 次，不应 panic
#[test]
fn backspace_near_4kb_boundary_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.cursor_x = 20;
    tui.bytes_cursor = 20;
    tui.bytes_cursor_size = 1;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..20 {
        h().handle_backspace(&mut tui, meta2, &td).unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

/// 大文件顶部（cursor_y=0, cursor_x=0）backspace 应为 noop
#[test]
fn backspace_at_file_start_large_file_is_noop() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.cursor_y = 0;
    tui.cursor_x = 0;
    let meta = td.get_current_line_meta().unwrap();
    h().handle_backspace(&mut tui, meta, &td).unwrap();
    assert_eq!(tui.cursor_y, 0, "文件顶部 backspace 后 cursor_y 不应改变");
    assert_eq!(tui.cursor_x, 0, "文件顶部 backspace 后 cursor_x 不应改变");
}

/// 大文件中间连续 backspace 50 次，不应 panic
#[test]
fn backspace_in_large_file_middle_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..10 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.cursor_x = 40;
    tui.bytes_cursor = 40;
    tui.bytes_cursor_size = 1;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..50 {
        h().handle_backspace(&mut tui, meta2, &td).unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 5. 4KB 块边界附近的 handle_enter
// ═══════════════════════════════════════════════════════════════════════════════

/// 大文件连续按回车 50 次，cursor_y <= TV_H
#[test]
fn enter_in_large_file_cursor_y_bounded() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..50 {
        h().handle_enter(&mut tui, meta, &td).unwrap();
    }
    assert!(
        tui.cursor_y <= TV_H,
        "大文件 Enter 后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

/// 在第 100 行附近插入 50 个换行，不应 panic
#[test]
fn enter_near_4kb_boundary_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.bytes_cursor = 10;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..50 {
        h().handle_enter(&mut tui, meta2, &td).unwrap();
    }
    assert!(tui.cursor_y <= TV_H);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 6. 粘贴大量数据（>4KB / >16KB）
// ═══════════════════════════════════════════════════════════════════════════════

/// 粘贴 4097 字节纯文本，cursor_y <= TV_H
#[test]
fn paste_4kb_plus_one_cursor_y_bounded() {
    let (mut tui, td, _f) = setup("start\n");
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "x".repeat(4097);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert!(
        tui.cursor_y <= TV_H,
        "粘贴4KB+ cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
    println!(
        "paste 4097 bytes → cursor_x={}, cursor_y={}",
        tui.cursor_x, tui.cursor_y
    );
}

/// 粘贴 16385 字节（> 16KB），cursor_y <= TV_H，cursor_x <= TV_W
#[test]
fn paste_16kb_plus_one_bounds() {
    let (mut tui, td, _f) = setup("start\n");
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "x".repeat(16385);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert!(
        tui.cursor_y <= TV_H,
        "粘贴16KB+ cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
    assert!(
        tui.cursor_x <= TV_W,
        "粘贴16KB+ cursor_x({}) 超出 TV_W({})",
        tui.cursor_x,
        TV_W
    );
    println!(
        "paste 16385 bytes → cursor_x={}, cursor_y={}",
        tui.cursor_x, tui.cursor_y
    );
}

/// 粘贴 TV_W+1 字符恰好触发一次折行，cursor_x 应为 1，cursor_y 应为 1
#[test]
fn paste_tv_width_plus_one_in_large_file_wraps() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "y".repeat(TV_W + 1);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert_eq!(
        tui.cursor_y, 1,
        "TV_W+1 粘贴后 cursor_y 应为 1，实际: {}",
        tui.cursor_y
    );
    assert_eq!(
        tui.cursor_x, 1,
        "TV_W+1 粘贴后 cursor_x 应为 1，实际: {}",
        tui.cursor_x
    );
}

/// 粘贴 1000 行（"abc\n" × 1000），cursor_y <= TV_H
#[test]
fn paste_1000_newlines_cursor_y_bounded() {
    let (mut tui, td, _f) = setup("start\n");
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "abc\n".repeat(1000);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert!(
        tui.cursor_y <= TV_H,
        "1000换行粘贴后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

/// 粘贴 (TV_W+1 字符 + 换行) × 50，cursor_x 和 cursor_y 均有界
#[test]
fn paste_many_wrapped_lines_bounds() {
    let (mut tui, td, _f) = setup("start\n");
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let unit = "z".repeat(TV_W + 1) + "\n";
    let paste = unit.repeat(50);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert!(
        tui.cursor_x <= TV_W,
        "大量折行粘贴后 cursor_x({}) 超出 TV_W({})",
        tui.cursor_x,
        TV_W
    );
    assert!(
        tui.cursor_y <= TV_H,
        "大量折行粘贴后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}

/// 在 17KB 大文件中粘贴 16KB，不应 panic
#[test]
fn paste_16kb_into_17kb_file_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "p".repeat(16384);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    assert!(tui.cursor_y <= TV_H);
}

/// 粘贴恰好 TV_W 字符（不触发折行边界），记录 cursor_x 实际值
/// Bug 候选：char_with == TV_W 时 `char_with > TV_W` 为 false → cursor_x = TV_W（不换行）
#[test]
fn paste_exactly_tv_width_no_wrap_cursor_x_record() {
    let (mut tui, td, _f) = setup(&content_17kb());
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    let paste = "q".repeat(TV_W);
    h().handle_paste(&mut tui, meta, &td, &paste).unwrap();
    println!(
        "paste TV_W({}) chars → cursor_x={}, cursor_y={}",
        TV_W, tui.cursor_x, tui.cursor_y
    );
    // cursor_x 应 <= TV_W（允许 == TV_W，但此时光标处于视觉行末外）
    assert!(
        tui.cursor_x <= TV_W,
        "cursor_x({}) 不应超出 TV_W({})",
        tui.cursor_x,
        TV_W
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 7. 滚动后执行编辑（测试 start_line_num 不更新的影响）
// ═══════════════════════════════════════════════════════════════════════════════

/// 向下滚动 100 行后插入字符：handle_char 调用 get_one_page(start_line_num=1)
/// 注意：handle_down/up 不更新 start_line_num，插入后视图从第 1 行重渲染，不应 panic
#[test]
fn char_after_scroll_start_line_num_mismatch_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    println!("scroll 100 lines, start_line_num={}", tui.start_line_num);
    tui.cursor_x = 5;
    tui.bytes_cursor = 5;
    let meta2 = td.get_current_line_meta().unwrap();
    h().handle_char(&mut tui, meta2, &td, 'T').unwrap();
}

/// 向下滚动后粘贴内容，不应 panic
#[test]
fn paste_after_scroll_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta2 = td.get_current_line_meta().unwrap();
    h().handle_paste(&mut tui, meta2, &td, "hello world\n")
        .unwrap();
}

/// 向下滚动后回车，不应 panic，cursor_y <= TV_H
#[test]
fn enter_after_scroll_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..150 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.bytes_cursor = 0;
    let meta2 = td.get_current_line_meta().unwrap();
    h().handle_enter(&mut tui, meta2, &td).unwrap();
    assert!(tui.cursor_y <= TV_H);
}

// ═══════════════════════════════════════════════════════════════════════════════
// 8. 内容正确性验证（粘贴后文件可读）
// ═══════════════════════════════════════════════════════════════════════════════

/// 向 "hello\n" 粘贴 " world"，保存后文件应同时包含两者
#[test]
fn paste_then_save_content_correct() {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(b"hello\n").unwrap();
    tmp.flush().unwrap();

    let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
    let mut td = TextDisplay::EditBlock(EditTextWarp::new(gap, TV_H, TV_W, TextWarpType::SoftWrap));
    td.get_one_page(1).unwrap();
    let mut tui = ChapTui::for_test(TV_H, TV_W);

    tui.cursor_x = 5;
    tui.bytes_cursor = 5;
    let meta = td.get_current_line_meta().unwrap();
    h().handle_paste(&mut tui, meta, &td, " world").unwrap();

    h().handle_ctrl_s(&mut tui, tmp.path(), &mut td).unwrap();
    let saved = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(
        saved.contains("hello") && saved.contains("world"),
        "保存后文件应同时包含 hello 和 world，实际: {:?}",
        saved
    );
}

/// 在大文件中插入 1000 字符后保存，文件大小应增大
#[test]
fn char_insert_1000_save_file_grows() {
    let content = ("a".repeat(80) + "\n").repeat(100);
    let original_size = content.len();

    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(content.as_bytes()).unwrap();
    tmp.flush().unwrap();

    let gap = GapBlockText::from_file_path(tmp.path()).unwrap();
    let mut td = TextDisplay::EditBlock(EditTextWarp::new(gap, TV_H, TV_W, TextWarpType::SoftWrap));
    td.get_one_page(1).unwrap();
    let mut tui = ChapTui::for_test(TV_H, TV_W);

    tui.cursor_x = 0;
    tui.bytes_cursor = 0;
    let meta = td.get_current_line_meta().unwrap();
    for _ in 0..1000 {
        h().handle_char(&mut tui, meta, &td, 'Z').unwrap();
    }

    h().handle_ctrl_s(&mut tui, tmp.path(), &mut td).unwrap();
    let new_size = std::fs::metadata(tmp.path()).unwrap().len() as usize;
    assert!(
        new_size > original_size,
        "插入1000字符后文件大小({})应大于原始大小({})",
        new_size,
        original_size
    );
    println!(
        "original={} bytes, after 1000 inserts={} bytes",
        original_size, new_size
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// 9. 混合操作压力测试
// ═══════════════════════════════════════════════════════════════════════════════

/// 17KB 文件中混合执行多种操作，不应 panic，光标始终有界
#[test]
fn stress_mixed_ops_large_file_no_panic() {
    let (mut tui, td, _f) = setup(&content_17kb());
    let meta = td.get_current_line_meta().unwrap();

    // 向下 5 行
    for _ in 0..5 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }

    // 插入 20 字符
    tui.cursor_x = 5;
    tui.bytes_cursor = 5;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..20 {
        h().handle_char(&mut tui, meta2, &td, 'M').unwrap();
    }

    // 向上 3 行
    for _ in 0..3 {
        h().handle_up(&mut tui, meta, &td).unwrap();
    }

    // 粘贴内容
    tui.bytes_cursor = 0;
    let meta3 = td.get_current_line_meta().unwrap();
    h().handle_paste(&mut tui, meta3, &td, "inserted_text\n")
        .unwrap();

    // 回车
    tui.bytes_cursor = 0;
    h().handle_enter(&mut tui, meta3, &td).unwrap();

    // backspace
    tui.bytes_cursor_size = 1;
    h().handle_backspace(&mut tui, meta3, &td).unwrap();

    // 右移 10 步
    for _ in 0..10 {
        h().handle_right(&mut tui, meta, &td).unwrap();
    }
    // 左移 10 步
    for _ in 0..10 {
        h().handle_left(&mut tui, meta, &td).unwrap();
    }

    assert!(
        tui.cursor_y <= TV_H,
        "混合操作后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
    assert!(
        tui.cursor_x <= TV_W,
        "混合操作后 cursor_x({}) 超出 TV_W({})",
        tui.cursor_x,
        TV_W
    );
}

/// 4 块文件中混合滚动 + 编辑，不应 panic
#[test]
fn stress_mixed_ops_4blocks_no_panic() {
    let (mut tui, td, _f) = setup(&content_4blocks());
    let meta = td.get_current_line_meta().unwrap();

    // 第1轮：向下100行再编辑
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.bytes_cursor = 10;
    tui.cursor_x = 10;
    let meta2 = td.get_current_line_meta().unwrap();
    for _ in 0..5 {
        h().handle_char(&mut tui, meta2, &td, 'R').unwrap();
    }
    h().handle_enter(&mut tui, meta2, &td).unwrap();

    // 第2轮：继续向下100行再backspace
    for _ in 0..100 {
        h().handle_down(&mut tui, meta, &td).unwrap();
    }
    tui.cursor_x = 15;
    tui.bytes_cursor = 15;
    tui.bytes_cursor_size = 1;
    let meta3 = td.get_current_line_meta().unwrap();
    for _ in 0..10 {
        h().handle_backspace(&mut tui, meta3, &td).unwrap();
    }

    // 第3轮：向上滚动回来
    for _ in 0..200 {
        h().handle_up(&mut tui, meta, &td).unwrap();
    }

    assert!(
        tui.cursor_y <= TV_H,
        "4块混合操作后 cursor_y({}) 超出 TV_H({})",
        tui.cursor_y,
        TV_H
    );
}
