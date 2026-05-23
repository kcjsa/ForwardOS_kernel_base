// src/net/e1000.rs

use crate::boot::sys_mmap;

pub struct E1000 {
    pub mmio_base: usize,
    pub mac: [u8; 6],
    rx_descs: *mut RxDesc,
    rx_buffers: *mut [u8; 2048],
    tx_descs: *mut TxDesc,
    tx_buffers: *mut [u8; 2048],
}

const RX_DESC_COUNT: usize = 32;
const TX_DESC_COUNT: usize = 8;
const BUFFER_SIZE: usize = 2048;

#[repr(C, align(16))]
struct RxDesc {
    buffer_addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

#[repr(C, align(16))]
struct TxDesc {
    buffer_addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

impl E1000 {
    pub unsafe fn new(bar0: u32) -> Self {
        let mmio_base = bar0 as usize;
        
        // MACアドレス読み取り
        let ral = core::ptr::read_volatile((mmio_base + 0x5400) as *const u32);
        let rah = core::ptr::read_volatile((mmio_base + 0x5404) as *const u32);
        let mac = [
            ral as u8, (ral >> 8) as u8, (ral >> 16) as u8, (ral >> 24) as u8,
            rah as u8, (rah >> 8) as u8,
        ];
        
        let mut nic = Self {
            mmio_base,
            mac,
            rx_descs: core::ptr::null_mut(),
            rx_buffers: core::ptr::null_mut(),
            tx_descs: core::ptr::null_mut(),
            tx_buffers: core::ptr::null_mut(),
        };
        
        nic.init();
        nic.setup_rx();
        nic.setup_tx();
        nic.enable();
        
        nic
    }
    
    unsafe fn init(&mut self) {
        // リセット
        let ctrl = self.read_reg(0x0000);
        self.write_reg(0x0000, ctrl | (1 << 26));
        while self.read_reg(0x0000) & (1 << 26) != 0 { core::hint::spin_loop(); }
        
        // リンクアップ待ち
        while self.read_reg(0x0008) & (1 << 1) == 0 { core::hint::spin_loop(); }
    }
    
    unsafe fn setup_rx(&mut self) {
        // 受信ディスクリプタ用メモリ確保
        let mut rx_desc_addr: u64 = 0;
        sys_mmap(1, 0, &mut rx_desc_addr as *mut u64 as u64);
        self.rx_descs = rx_desc_addr as *mut RxDesc;
        core::ptr::write_bytes(self.rx_descs, 0, RX_DESC_COUNT);
        
        // 受信バッファ用メモリ確保
        let mut rx_buf_addr: u64 = 0;
        sys_mmap(16, 0, &mut rx_buf_addr as *mut u64 as u64);
        self.rx_buffers = rx_buf_addr as *mut [u8; 2048];
        
        // 各ディスクリプタにバッファを設定
        for i in 0..RX_DESC_COUNT {
            let buf_ptr = (rx_buf_addr + (i * 2048) as u64) as *mut [u8; 2048];
            (*self.rx_descs.add(i)).buffer_addr = buf_ptr as u64;
            (*self.rx_descs.add(i)).status = 0; // ハードウェアが書き込む
        }
        
        // 受信ディスクリプタのベースアドレスを設定
        self.write_reg(0x2800, self.rx_descs as u64 as u32);      // RDBAL
        self.write_reg(0x2804, (self.rx_descs as u64 >> 32) as u32); // RDBAH
        self.write_reg(0x2808, (RX_DESC_COUNT * 16) as u32);       // RDLEN
        
        // 受信ヘッド／テールポインタ
        self.write_reg(0x2810, 0); // RDH
        self.write_reg(0x2818, RX_DESC_COUNT as u32 - 1); // RDT
    }
    
    unsafe fn setup_tx(&mut self) {
        let mut tx_desc_addr: u64 = 0;
        sys_mmap(1, 0, &mut tx_desc_addr as *mut u64 as u64);
        self.tx_descs = tx_desc_addr as *mut TxDesc;
        core::ptr::write_bytes(self.tx_descs, 0, TX_DESC_COUNT);
        
        let mut tx_buf_addr: u64 = 0;
        sys_mmap(4, 0, &mut tx_buf_addr as *mut u64 as u64);
        self.tx_buffers = tx_buf_addr as *mut [u8; 2048];
        
        for i in 0..TX_DESC_COUNT {
            let buf_ptr = (tx_buf_addr + (i * 2048) as u64) as *mut [u8; 2048];
            (*self.tx_descs.add(i)).buffer_addr = buf_ptr as u64;
            (*self.tx_descs.add(i)).status = 1; // Done
        }
        
        self.write_reg(0x3800, self.tx_descs as u64 as u32);      // TDBAL
        self.write_reg(0x3804, (self.tx_descs as u64 >> 32) as u32); // TDBAH
        self.write_reg(0x3808, (TX_DESC_COUNT * 16) as u32);       // TDLEN
        
        self.write_reg(0x3810, 0); // TDH
        self.write_reg(0x3818, 0); // TDT
    }
    
 unsafe fn enable(&self) {
    // 受信：全て受け取る
    let mut rctl = self.read_reg(0x0100);
    rctl &= !0xFFFF; // 一旦クリア
    rctl |= (1 << 1);   // UPE: Unicast Promiscuous
    rctl |= (1 << 3);   // MPE: Multicast Promiscuous
    rctl |= (1 << 4);   // BAM: Broadcast Accept Mode
    rctl |= (1 << 6);   // SECRC: Strip CRC
    rctl |= (1 << 15);  // BSEX: Buffer Size Extension
    rctl |= (1 << 27);  // VFE: VLAN Filter Enable
    rctl |= 1;          // EN: Enable
    
    self.write_reg(0x0100, rctl);
    core::arch::asm!("mfence");
    
    // 送信有効化
    let mut tctl = self.read_reg(0x0400);
    tctl |= (1 << 1); // EN
    tctl |= (1 << 3); // PSP: Pad Short Packets
    self.write_reg(0x0400, tctl);
    core::arch::asm!("mfence");
    
    crate::boot::debug_print(&alloc::format!("[e1000] RCTL=0x{:X} TCTL=0x{:X}", rctl, tctl));
}

pub unsafe fn receive(&mut self) -> Option<&[u8]> {
    let rdh = self.read_reg(0x2810) as usize;
    let rdt = self.read_reg(0x2818) as usize;
    
    // 確認すべきはRDTの次の位置
    let idx = (rdt + 1) % RX_DESC_COUNT;
    
    let desc = &*self.rx_descs.add(idx);
    
    if desc.status & 1 != 0 {
        let len = desc.length as usize;
        let buf = core::slice::from_raw_parts(
            self.rx_buffers.add(idx) as *const u8,
            len
        );
        
        (*self.rx_descs.add(idx)).status = 0;
        core::arch::asm!("mfence");
        
        // テールを進める
        self.write_reg(0x2818, idx as u32);
        
        Some(buf)
    } else {
        None
    }
}
    
    /// パケット受信までポーリング
    pub unsafe fn poll_receive(&mut self) -> Option<&[u8]> {
        self.receive()
    }
    
    /// パケット送信
    pub unsafe fn send(&mut self, data: &[u8]) -> bool {
        if data.len() > 2048 { return false; }
        
        let tdt = self.read_reg(0x3818) as usize;
        let desc = &mut *self.tx_descs.add(tdt);
        
        // 前回の送信が完了してるか
        if desc.status & 0x0F != 1 { return false; }
        
        // データをコピー
        let buf_ptr = self.tx_buffers.add(tdt) as *mut u8;
        core::ptr::copy_nonoverlapping(data.as_ptr(), buf_ptr, data.len());
        core::arch::asm!("mfence");
        
        // ディスクリプタ設定
        desc.length = data.len() as u16;
        desc.cmd = 0x0B; // EOP | IFCS | RS
        desc.status &= !1; // クリア
        
        // テール進める
        let new_tdt = (tdt + 1) % TX_DESC_COUNT;
        self.write_reg(0x3818, new_tdt as u32);
        
        true
    }

    pub fn read_reg(&self, offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((self.mmio_base + offset) as *const u32) }
}

pub fn write_reg(&self, offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((self.mmio_base + offset) as *mut u32, value) }
}
}

