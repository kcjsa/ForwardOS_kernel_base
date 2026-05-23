// src/net/udp.rs

#[repr(C, packed)]
pub struct UdpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub checksum: u16,
}

impl UdpHeader {
    pub fn new(src_port: u16, dst_port: u16, data_len: usize) -> Self {
        Self {
            src_port: src_port.to_be(),
            dst_port: dst_port.to_be(),
            length: (8 + data_len as u16).to_be(),
            checksum: 0, // 後で計算
        }
    }
}

/// UDPチェックサム計算（IP疑似ヘッダ含む）
pub fn calc_udp_checksum(
    src_ip: &[u8; 4],
    dst_ip: &[u8; 4],
    protocol: u8,
    udp_data: &[u8],
) -> u16 {
    let udp_len = udp_data.len() as u16;
    let mut sum: u32 = 0;

    // IP疑似ヘッダ（as u32を付ける）
    sum += (((src_ip[0] as u16) << 8) | (src_ip[1] as u16)) as u32;
    sum += (((src_ip[2] as u16) << 8) | (src_ip[3] as u16)) as u32;
    sum += (((dst_ip[0] as u16) << 8) | (dst_ip[1] as u16)) as u32;
    sum += (((dst_ip[2] as u16) << 8) | (dst_ip[3] as u16)) as u32;
    sum += protocol as u32;
    sum += udp_len as u32;

    // UDPデータ
    let mut i = 0;
    while i < udp_data.len() {
        let word = if i + 1 < udp_data.len() {
            ((udp_data[i] as u16) << 8) | (udp_data[i + 1] as u16)
        } else {
            (udp_data[i] as u16) << 8
        };
        sum += word as u32;
        i += 2;
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    if sum == 0xFFFF {
        0xFFFF
    } else {
        !(sum as u16)
    }
}

/// UDPパケット全体を組み立ててEthernetフレームを返す
pub fn build_udp_packet(
    src_mac: [u8; 6],
    dst_mac: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    data: &[u8],
) -> alloc::vec::Vec<u8> {
    use crate::net::ip::Ipv4Header;

    let udp = UdpHeader::new(src_port, dst_port, data.len());
    
    // UDPヘッダ + データ
    let mut udp_buf = alloc::vec![0u8; 8 + data.len()];
    udp_buf[0..2].copy_from_slice(&udp.src_port.to_be_bytes());
    udp_buf[2..4].copy_from_slice(&udp.dst_port.to_be_bytes());
    udp_buf[4..6].copy_from_slice(&udp.length.to_be_bytes());
    udp_buf[6..8].copy_from_slice(&[0, 0]); // checksumプレースホルダ
    udp_buf[8..].copy_from_slice(data);
    
    // チェックサム計算
    let checksum = calc_udp_checksum(&src_ip, &dst_ip, 17, &udp_buf);
    udp_buf[6] = (checksum >> 8) as u8;
    udp_buf[7] = (checksum & 0xFF) as u8;

    // IPヘッダ
    let ip = Ipv4Header::new(src_ip, dst_ip, udp_buf.len() as u16, 17); // 17 = UDP

    // Ethernetフレーム組み立て
    let mut pkt = alloc::vec::Vec::new();
    pkt.extend_from_slice(&dst_mac);
    pkt.extend_from_slice(&src_mac);
    pkt.extend_from_slice(&[0x08, 0x00]);
    let ip_bytes = unsafe { core::slice::from_raw_parts(&ip as *const _ as *const u8, 20) };
    pkt.extend_from_slice(ip_bytes);
    pkt.extend_from_slice(&udp_buf);

    pkt
}