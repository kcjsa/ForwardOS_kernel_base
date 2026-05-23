use crate::FrameBufferConfig;
extern crate alloc;
use alloc::vec::Vec;

// フォントをバイナリとして埋め込み
const FONT_BINARY: &[u8] = include_bytes!("font16.psf");

// --- 二重バッファ管理用の構造体 ---
pub struct DoubleBuffer {
    pub vram_shadow: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub scanline: u32,
}

impl DoubleBuffer {
    /// ALLOCATOR初期化後に呼び出し可能
    pub fn new(fb: &FrameBufferConfig) -> Self {
        let size = (fb.pixels_per_scan_line * fb.vertical_resolution) as usize;
        Self {
            vram_shadow: alloc::vec![0; size],
            width: fb.horizontal_resolution,
            height: fb.vertical_resolution,
            scanline: fb.pixels_per_scan_line,
        }
    }

    /// バックバッファの内容をVRAMへ一括転送
    pub fn flip(&self, fb: &FrameBufferConfig) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.vram_shadow.as_ptr(),
                fb.frame_buffer as *mut u32,
                self.vram_shadow.len(),
            );
        }
    }
}

// --- ヘルパー: フォント情報の解析 ---
fn get_font_info() -> Option<(usize, usize, usize)> {
    if FONT_BINARY.len() < 4 { return None; }
    if FONT_BINARY[0..4] == [0x72, 0xb5, 0x4a, 0x86] {
        let h_size = u32::from_le_bytes([FONT_BINARY[12], FONT_BINARY[13], FONT_BINARY[14], FONT_BINARY[15]]) as usize;
        let g_size = u32::from_le_bytes([FONT_BINARY[24], FONT_BINARY[25], FONT_BINARY[26], FONT_BINARY[27]]) as usize;
        let h = u32::from_le_bytes([FONT_BINARY[20], FONT_BINARY[21], FONT_BINARY[22], FONT_BINARY[23]]) as usize;
        Some((h_size, g_size, h))
    } else if FONT_BINARY[0..2] == [0x36, 0x04] {
        let h = FONT_BINARY[3] as usize;
        Some((4, h, h))
    } else {
        None
    }
}

// --- 1. 既存の直接描画 (VRAM Direct) ---
pub fn draw_char(fb: &FrameBufferConfig, x: u32, y: u32, c: u8, color: u32) {
    let (header_size, glyph_size, height) = match get_font_info() {
        Some(info) => info,
        None => return,
    };
    let offset = header_size + (c as usize) * glyph_size;
    if offset + height > FONT_BINARY.len() { return; }
    let glyph = &FONT_BINARY[offset..offset + height];

    for dy in 0..height {
        let row = glyph[dy];
        for dx in 0..8 {
            if (row << dx) & 0x80 != 0 {
                let px = x + dx as u32;
                let py = y + dy as u32;
                if px < fb.horizontal_resolution && py < fb.vertical_resolution {
                    let index = (fb.pixels_per_scan_line * py + px) as isize;
                    unsafe { fb.frame_buffer.offset(index).write_volatile(color); }
                }
            }
        }
    }
}

pub fn draw_string(fb: &FrameBufferConfig, mut x: u32, y: u32, s: &str, color: u32) {
    for c in s.bytes() {
        draw_char(fb, x, y, c, color);
        x += 8;
    }
}

pub fn draw_u64(fb: &FrameBufferConfig, x: u32, y: u32, mut val: u64, color: u32) {
    let mut buf = [0u8; 20];
    let mut i = 0;
    if val == 0 { draw_char(fb, x, y, b'0', color); return; }
    while val > 0 && i < 20 {
        buf[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    for j in 0..i {
        draw_char(fb, x + ((i - 1 - j) as u32 * 8), y, buf[j], color);
    }
}

pub fn clear_screen(fb: &FrameBufferConfig, color: u32) {
    for i in 0..(fb.pixels_per_scan_line * fb.vertical_resolution) {
        unsafe { fb.frame_buffer.offset(i as isize).write_volatile(color); }
    }
}

// --- 2. 二重バッファ用描画 (Shadow Buffer) ---
pub fn draw_char_db(db: &mut DoubleBuffer, x: u32, y: u32, c: u8, color: u32) {
    let (header_size, glyph_size, height) = match get_font_info() {
        Some(info) => info,
        None => return,
    };
    let offset = header_size + (c as usize) * glyph_size;
    if offset + height > FONT_BINARY.len() { return; }
    let glyph = &FONT_BINARY[offset..offset + height];

    for dy in 0..height {
        let row = glyph[dy];
        for dx in 0..8 {
            if (row << dx) & 0x80 != 0 {
                let px = x + dx as u32;
                let py = y + dy as u32;
                if px < db.width && py < db.height {
                    let index = (db.scanline * py + px) as usize;
                    db.vram_shadow[index] = color;
                }
            }
        }
    }
}

pub fn draw_string_db(db: &mut DoubleBuffer, mut x: u32, y: u32, s: &str, color: u32) {
    for c in s.bytes() {
        draw_char_db(db, x, y, c, color);
        x += 8;
    }
}

pub fn clear_screen_db(db: &mut DoubleBuffer, color: u32) {
    db.vram_shadow.fill(color);
}

/// ダブルバッファに対して数値を16進数で描画する
pub fn draw_u64_db(db: &mut DoubleBuffer, x: u32, y: u32, mut val: u64, color: u32) {
    if val == 0 {
        draw_string_db(db, x, y, "0", color);
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 0;
    while val > 0 {
        let m = (val % 16) as u8;
        buf[i] = if m < 10 { b'0' + m } else { b'A' + (m - 10) };
        val /= 16;
        i += 1;
    }
    for j in 0..i {
        let s = [buf[i - 1 - j]; 1];
        draw_string_db(db, x + (j as u32 * 8), y, unsafe { core::str::from_utf8_unchecked(&s) }, color);
    }
}


pub fn fill_rect_db(db: &mut DoubleBuffer, x: u32, y: u32, width: u32, height: u32, color: u32) {
    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            // 構造体の持つ width, height を使って境界チェック
            if px < db.width && py < db.height {
                // フィールド名を vram_shadow に修正し、scanline を考慮
                db.vram_shadow[py as usize * db.scanline as usize + px as usize] = color;
            }
        }
    }
}

pub fn draw_hex_dump(db: &mut DoubleBuffer, x: u32, y: u32, data: *const u8, len: usize, color: u32) {
    for i in 0..len {
        let val = unsafe { *data.add(i) };
        let dx = (i % 16) as u32 * 24; // 16バイトごとに改行
        let dy = (i / 16) as u32 * 12;
        
        // 簡易的な16進数表示（上位・下位4ビットを文字に変換）
        let h = (val >> 4) & 0xF;
        let l = val & 0xF;
        let hex_chars = b"0123456789ABCDEF";
        
        let s = [hex_chars[h as usize], hex_chars[l as usize], b' '];
        draw_string_db(db, x + dx, y + dy, unsafe { core::str::from_utf8_unchecked(&s) }, color);
        
        if i >= 127 { break; } // 画面に収まるよう最初の128バイトまでに制限
    }
}


pub fn draw_hex_db(db: &mut DoubleBuffer, x: u32, y: u32, value: u64, color: u32) {
    let mut temp = value;
    for i in (0..16).rev() {
        let hex = (temp & 0xF) as u8;
        let c = if hex < 10 { b'0' + hex } else { b'A' + (hex - 10) };
        draw_char_db(db, x + (i * 8), y, c, color);
        temp >>= 4;
    }
}

pub fn draw_rect_db(db: &mut DoubleBuffer, x: u32, y: u32, width: u32, height: u32, color: u32) {
    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px < db.width && py < db.height {
                db.vram_shadow[py as usize * db.scanline as usize + px as usize] = color;
            }
        }
    }}
