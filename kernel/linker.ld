/* linker.ld */
ENTRY(_start)

SECTIONS
{
    /* config.toml に書いていた 0x1000000 をここに集約 */
    . = 0x1000000;

    /* --- alv0 & alv1: 実行コード・定数 (WP対象) --- */
    __kernel_start = .;
    .text : {
        *(.text .text.*)
    }
    .rodata : {
        *(.rodata .rodata.*)
    }

    /* 4KB境界に揃えて、ここから属性を変える */
    . = ALIGN(4K);
    __kernel_readonly_end = .;

    /* ★★★ ガードページ（アクセス禁止）★★★ */
    . = ALIGN(4K);
    __guard_start = .;
    .guard : {
        /* 何も配置しない、ただの境界 */
    }
    . = ALIGN(4K);
    __guard_end = .;

    /* --- alv2: 標準機能・変数 (Writable) --- */
    .data : {
        *(.data .data.*)
    }
    .bss : {
        *(.bss .bss.*)
        *(COMMON)
    }

    /* ★★★ ガードページ（アクセス禁止）★★★ */
    . = ALIGN(4K);
    __guard2_start = .;
    .guard2 : {
        /* 何も配置しない */
    }
    . = ALIGN(4K);
    __guard2_end = .;

    /* ★★★ RamManager 専用隔離セクション ★★★ */
    . = ALIGN(4K);
    __ram_mgr_start = .;
    .ram_mgr : {
        *(.ram_mgr .ram_mgr.*)
    }
    __ram_mgr_end = .;

    /* ★★★ 最後のガードページ ★★★ */
    . = ALIGN(4K);
    __guard3_start = .;
    .guard3 : {
        /* 何も配置しない */
    }
    . = ALIGN(4K);
    __guard3_end = .;

    __kernel_end = .;
}