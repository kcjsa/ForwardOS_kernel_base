// src/net/icmp.rs

use alloc::vec::Vec;
use crate::net::ip::Ipv4Header;

pub fn build_echo_reply(request: &[u8], our_mac: [u8; 6], dst_mac: [u8; 6]) -> Vec<u8> {
    let ip_off = 14;
    let icmp_off = ip_off + 20;

    if request.len() < icmp_off + 8 {
        return Vec::new();
    }

    let src_ip: [u8; 4] = [request[ip_off+12], request[ip_off+13], request[ip_off+14], request[ip_off+15]];
    let dst_ip: [u8; 4] = [request[ip_off+16], request[ip_off+17], request[ip_off+18], request[ip_off+19]];
    let icmp_data = &request[icmp_off..];
    let icmp_len = icmp_data.len();

    let ip = Ipv4Header::new(dst_ip, src_ip, icmp_len as u16, 1);

    let mut icmp = [0u8; 64];
    icmp[0] = 0;       // type = Echo Reply
    icmp[1] = 0;       // code = 0
    let copy_len = icmp_len.min(62);
    icmp[4..4+copy_len-4].copy_from_slice(&icmp_data[4..copy_len]); // id + seq + data
    
    // ★ ICMPチェックサム計算
    let checksum = calc_icmp_checksum(&icmp[..icmp_len]);
    icmp[2] = (checksum >> 8) as u8;
    icmp[3] = (checksum & 0xFF) as u8;

    let mut pkt = Vec::new();
    pkt.extend_from_slice(&dst_mac);
    pkt.extend_from_slice(&our_mac);
    pkt.extend_from_slice(&[0x08, 0x00]);
    let ip_bytes = unsafe { core::slice::from_raw_parts(&ip as *const _ as *const u8, 20) };
    pkt.extend_from_slice(ip_bytes);
    pkt.extend_from_slice(&icmp[..icmp_len]);

    pkt
}

fn calc_icmp_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < data.len() {
        let word = if i + 1 < data.len() {
            ((data[i] as u16) << 8) | (data[i+1] as u16)
        } else {
            (data[i] as u16) << 8
        };
        sum += word as u32;
        i += 2;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}