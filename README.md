**Forward OS** - Educational Real-Time OS with Async Runtime written in Rust

[![Build Status](https://github.com/kcjsa/ForwardOS_kernel_base/blob/main/icon.png)](https://github.com/kcjsa/ForwardOS_kernel_base/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org)

---

## 📖 Overview

Forward OS is an **operating system** that demonstrates:

- ✅ **Async/Await Runtime** inside kernel space
- ✅ **High-precision TSC Timer** (μs accuracy, no APIC garbage)
- ✅ **FAT32 File System** with AHCI driver
- ✅ **ELF Loader** for user applications
- ✅ **Lightweight Container** for process isolation
- ✅ **E1000 Network Driver** (ARP/ICMP/UDP)

Designed for **learning OS development** with practical, working examples.

---

## 🚀 Quick Start (QEMU)

```bash
# Clone
git clone https://github.com/kcjsa/ForwardOS_kernel_base
cd ForwardOS_kernel_base

# Build kernel
cd kernel
make run 


cd ..

cd bootloader 

make run
