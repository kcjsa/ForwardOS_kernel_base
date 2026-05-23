// src/net/ip.rs

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Ipv4Header {
    pub version_ihl: u8,
    pub dscp_ecn: u8,
    pub total_len: u16,
    pub id: u16,
    pub flags_offset: u16,
    pub ttl: u8,
    pub protocol: u8,
    pub checksum: u16,
    pub src_ip: [u8; 4],
    pub dst_ip: [u8; 4],
}

impl Ipv4Header {
    pub fn new(src: [u8; 4], dst: [u8; 4], payload_len: u16, protocol: u8) -> Self {
        let mut hdr = Self {
            version_ihl: 0x45,        // version=4, IHL=5 (20bytes)
            dscp_ecn: 0,
            total_len: (20 + payload_len).to_be(),
            id: 0,
            flags_offset: 0,
            ttl: 64,
            protocol,
            checksum: 0,
            src_ip: src,
            dst_ip: dst,
        };
        hdr.checksum = hdr.calc_checksum();
        hdr
    }

    pub fn calc_checksum(&self) -> u16 {
        let ptr = self as *const _ as *const u16;
        let mut sum: u32 = 0;
        for i in 0..10 {
            sum += unsafe { ptr.add(i).read_unaligned() } as u32;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }
}