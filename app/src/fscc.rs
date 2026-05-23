// fscc.rs
#![no_std]

use core::arch::asm;


//==============================================
//global
//==============================================
// ============================================
// 描画 (SYS_WRITE_AT = 15)
// ============================================

#[inline(always)]
pub fn draw_text(x: u32, y: u32, text: &str) {
    let ptr = text.as_ptr();
    let len = text.len();
    unsafe {
        asm!("int 0x80",
            in("rax") 15,
            in("rdi") x as u64,
            in("rsi") y as u64,
            in("rdx") ptr,
            in("rcx") len,
        );
    }
}


// ============================================
// キーボード読み取り（ポート直接読み込み版）
// ============================================

#[inline(always)]
pub fn read_key() -> Option<u8> {
    let status: u8;
    unsafe { asm!("in al, dx", out("al") status, in("dx") 0x64); }
    
    if (status & 0x01) != 0 {
        let sc: u8;
        unsafe { asm!("in al, dx", out("al") sc, in("dx") 0x60); }
        Some(sc)
    } else {
        None
    }
}

// 待機版（必要なら）
#[inline(always)]
pub fn wait_key() -> u8 {
    loop {
        if let Some(key) = read_key() {
            return key;
        }
        for _ in 0..1000 {
            unsafe { core::arch::asm!("pause"); }
        }
    }
}

// ============================================
// プロセス終了 (SYS_EXIT = 2)
// ============================================
//taskを終了できないです。　使用しないでください　(CのABIを使ってください)
#[inline(always)]
pub fn exit(code: i32) -> ! {
    unsafe {
        asm!("int 0x80",
            in("rax") 2,
            in("rdi") code as u64,
        );
    }
    loop {}
}

// ============================================
// ファイルオープン (SYS_OPEN = 3)
// ============================================

#[inline(always)]
pub fn open(path: &str) -> i32 {
    unsafe {
        let ret: i32;
        asm!("int 0x80",
            in("rax") 3,
            in("rdi") path.as_ptr() as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// ファイルクローズ (SYS_CLOSE = 4)
// ============================================

#[inline(always)]
pub fn close(fd: i32) -> i32 {
    unsafe {
        let ret: i32;
        asm!("int 0x80",
            in("rax") 4,
            in("rdi") fd as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// ファイルシーク (SYS_LSEEK = 5)
// ============================================

#[inline(always)]
pub fn lseek(fd: i32, offset: i64, whence: u32) -> i64 {
    unsafe {
        let ret: i64;
        asm!("int 0x80",
            in("rax") 5,
            in("rdi") fd as u64,
            in("rsi") offset as u64,
            in("rdx") whence as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// ファイル情報取得 (SYS_FSTAT = 6)
// ============================================

#[repr(C)]
pub struct FileStat {
    pub file_size: u32,
    pub cluster: u32,
    pub position: u64,
    pub is_dir: bool,
}

#[inline(always)]
pub fn fstat(fd: i32, stat: &mut FileStat) -> i32 {
    unsafe {
        let ret: i32;
        asm!("int 0x80",
            in("rax") 6,
            in("rdi") fd as u64,
            in("rsi") stat as *mut FileStat as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// プログラム実行 (SYS_EXEC = 7)
// ============================================

#[inline(always)]
pub fn exec(path: &str) -> ! {
    unsafe {
        asm!("int 0x80",
            in("rax") 7,
            in("rdi") path.as_ptr() as u64,
        );
    }
    loop {}
}

// ============================================
// 譲る (SYS_YIELD = 8)
// ============================================

#[inline(always)]
pub fn yield_now() {
    unsafe {
        asm!("int 0x80",
            in("rax") 8,
        );
    }
}

// ============================================
// ファイル読み込み (SYS_READ_FILE = 9)
// ============================================

#[inline(always)]
pub fn read_file(fd: i32, buf: &mut [u8]) -> isize {
    unsafe {
        let ret: isize;
        asm!("int 0x80",
            in("rax") 9,
            in("rdi") fd as u64,
            in("rsi") buf.as_ptr() as u64,
            in("rdx") buf.len() as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// ファイル書き込み (SYS_WRITE_FILE = 10)
// ============================================

#[inline(always)]
pub fn write_file(fd: i32, buf: &[u8]) -> isize {
    unsafe {
        let ret: isize;
        asm!("int 0x80",
            in("rax") 10,
            in("rdi") fd as u64,
            in("rsi") buf.as_ptr() as u64,
            in("rdx") buf.len() as u64,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// シーク (SYS_SEEK = 11)
// ============================================

#[inline(always)]
pub fn seek(fd: i32, pos: u64) -> i64 {
    unsafe {
        let ret: i64;
        asm!("int 0x80",
            in("rax") 11,
            in("rdi") fd as u64,
            in("rsi") pos,
            lateout("rax") ret,
        );
        ret
    }
}

// ============================================
// メモリ確保 (SYS_MGET = 13)
// ============================================

#[inline(always)]
pub fn mget(pages: u64) -> u64 {
    let mut addr: u64 = 0;
    unsafe {
        asm!("int 0x80",
            in("rax") 13,
            in("rdi") pages,
            in("rsi") 0,
            in("rdx") &mut addr as *mut u64 as u64,
        );
    }
    addr
}

// ============================================
// メモリマップ (SYS_MMAP = 14)
// ============================================

#[inline(always)]
pub fn mmap(pages: u64, align: u64) -> u64 {
    let mut addr: u64 = 0;
    unsafe {
        asm!("int 0x80",
            in("rax") 14,
            in("rdi") pages,
            in("rsi") align,
            in("rdx") &mut addr as *mut u64 as u64,
        );
    }
    addr
}