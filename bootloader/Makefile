# 設定
IMG_NAME = disk.img
ESP_DIR = ./esp
BOOTLOADER_EFI = target/x86_64-unknown-uefi/release/my_os_loader.efi

.PHONY: all build img run clean

all: run

# 1. ブートローダのビルド
build:
	cargo build --release --target x86_64-unknown-uefi
	mkdir -p $(ESP_DIR)/EFI/BOOT
	cp $(BOOTLOADER_EFI) $(ESP_DIR)/EFI/BOOT/BOOTX64.efi

# 2. ディスクイメージの作成と更新
img: build
	@echo "--- Rebuilding Disk Image ---"
	@# 既存のイメージを消去して対話プロンプトを回避
	rm -f $(IMG_NAME)
	dd if=/dev/zero of=$(IMG_NAME) bs=1M count=64
	@# パーティション作成
	printf "o\nn\np\n1\n2048\n\nt\nc\nw\n" | fdisk $(IMG_NAME)
	@# FAT32フォーマット
	mkfs.vfat -F 32 --offset 2048 $(IMG_NAME)
	@# --- ディレクトリ作成 ---
	mmd -i $(IMG_NAME)@@1M ::/EFI
	mmd -i $(IMG_NAME)@@1M ::/EFI/BOOT
	@# --- ファイルコピー ---
	mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/EFI/BOOT/BOOTX64.efi ::/EFI/BOOT/
	@if [ -f $(ESP_DIR)/kernel.elf ]; then mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/kernel.elf ::/; fi
	@if [ -f $(ESP_DIR)/TXT.TXT ]; then mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/TXT.TXT ::/; fi
	@if [ -f $(ESP_DIR)/APP.ELF ]; then mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/APP.ELF ::/; fi
	@if [ -f $(ESP_DIR)/APP2.ELF ]; then mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/APP2.ELF ::/; fi
	@if [ -f $(ESP_DIR)/APP3.ELF ]; then mcopy -i $(IMG_NAME)@@1M -D o $(ESP_DIR)/APP3.ELF ::/; fi
	@if [ -d $(ESP_DIR)/sys64 ]; then mcopy -i $(IMG_NAME)@@1M -s -D o $(ESP_DIR)/sys64 ::/; fi
	@echo "--- Disk Image Ready ---"

run: img
	qemu-system-x86_64 \
    -m 2G \
    -drive if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd \
    -drive if=pflash,format=raw,file=./OVMF_VARS.fd \
    -device ich9-ahci,id=ahci0 \
    -drive if=none,file=disk.img,id=drive0,format=raw,cache=directsync \
    -device ide-hd,drive=drive0,bus=ahci0.0,bootindex=1 \
	-nic model=e1000 \
    -monitor stdio \
    -d guest_errors,int,cpu_reset \
    -D debug.log