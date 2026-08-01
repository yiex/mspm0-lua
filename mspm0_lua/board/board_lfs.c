#include "board_lfs.h"
#include "board_spiflash.h"
#include "board_uart.h"

#include <string.h>

#define LFS_CACHE_SIZE 256
#define LFS_LOOKAHEAD_SIZE 16
#define LFS_BLOCK_SIZE SPIFLASH_SECTOR_SIZE
#ifndef LFS_BLOCK_COUNT
#define LFS_BLOCK_COUNT 0
#endif

static lfs_t s_lfs;
static struct lfs_config s_cfg;
static uint8_t s_read_buf[LFS_CACHE_SIZE];
static uint8_t s_prog_buf[LFS_CACHE_SIZE];
static uint8_t s_lookahead_buf[LFS_LOOKAHEAD_SIZE];
static uint8_t s_read_file_buf[LFS_CACHE_SIZE];
static uint8_t s_write_file_buf[LFS_CACHE_SIZE];
static struct lfs_file_config s_read_fcfg;
static struct lfs_file_config s_write_fcfg;
static lfs_file_t s_reader;
static lfs_file_t s_writer;
static bool s_reader_open;
static bool s_writer_open;
static bool s_ready;

static int lfs_bd_read(const struct lfs_config *c, lfs_block_t block,
    lfs_off_t off, void *buffer, lfs_size_t size)
{
    (void)c;
    uint32_t addr = (uint32_t)block * LFS_BLOCK_SIZE + off;
    return board_spiflash_read(addr, (uint8_t *)buffer, size) ? 0 : LFS_ERR_IO;
}

static int lfs_bd_prog(const struct lfs_config *c, lfs_block_t block,
    lfs_off_t off, const void *buffer, lfs_size_t size)
{
    (void)c;
    uint32_t addr = (uint32_t)block * LFS_BLOCK_SIZE + off;
    return board_spiflash_program(addr, (const uint8_t *)buffer, size) ? 0 : LFS_ERR_IO;
}

static int lfs_bd_erase(const struct lfs_config *c, lfs_block_t block)
{
    (void)c;
    return board_spiflash_erase_sector((uint32_t)block * LFS_BLOCK_SIZE) ? 0 : LFS_ERR_IO;
}

static int lfs_bd_sync(const struct lfs_config *c)
{
    (void)c;
    return 0;
}

static void lfs_cfg_init(void)
{
    memset(&s_cfg, 0, sizeof(s_cfg));
    s_cfg.read = lfs_bd_read;
    s_cfg.prog = lfs_bd_prog;
    s_cfg.erase = lfs_bd_erase;
    s_cfg.sync = lfs_bd_sync;
    s_cfg.read_size = 16;
    s_cfg.prog_size = 16;
    s_cfg.block_size = LFS_BLOCK_SIZE;
    s_cfg.block_count = LFS_BLOCK_COUNT ? LFS_BLOCK_COUNT :
        board_spiflash_capacity_bytes() / LFS_BLOCK_SIZE;
    s_cfg.cache_size = LFS_CACHE_SIZE;
    s_cfg.lookahead_size = LFS_LOOKAHEAD_SIZE;
    s_cfg.block_cycles = 100;
    s_cfg.read_buffer = s_read_buf;
    s_cfg.prog_buffer = s_prog_buf;
    s_cfg.lookahead_buffer = s_lookahead_buf;
    s_cfg.name_max = 31;
    s_cfg.file_max = s_cfg.block_count * s_cfg.block_size;
}

static int file_open(lfs_file_t *f, const char *path, int flags,
    struct lfs_file_config *fcfg, uint8_t *buffer)
{
    memset(fcfg, 0, sizeof(*fcfg));
    fcfg->buffer = buffer;
    return lfs_file_opencfg(&s_lfs, f, path, flags, fcfg);
}

/* Virgin NOR is 0xFF. Only then is auto-format safe on first boot. */
static int flash_looks_blank(void)
{
    uint8_t buf[64];
    unsigned blk;
    unsigned i;
    uint32_t cap = board_spiflash_capacity_bytes();
    if (cap < (2u * LFS_BLOCK_SIZE)) {
        return 0;
    }
    for (blk = 0; blk < 2u; blk++) {
        if (!board_spiflash_read(blk * LFS_BLOCK_SIZE, buf, sizeof(buf))) {
            return 0;
        }
        for (i = 0; i < sizeof(buf); i++) {
            if (buf[i] != 0xFFu) {
                return 0;
            }
        }
    }
    return 1;
}

bool board_lfs_init(void)
{
    s_ready = false;
    s_reader_open = false;
    s_writer_open = false;
    lfs_cfg_init();
    if (s_cfg.block_count < 16u) {
        board_uart_puts("LFS NO\n");
        return false;
    }
    int err = lfs_mount(&s_lfs, &s_cfg);
    if (err) {
        /* Retry once for transient SPI noise. */
        memset(&s_lfs, 0, sizeof(s_lfs));
        lfs_cfg_init();
        if (lfs_mount(&s_lfs, &s_cfg) < 0) {
            /*
             * Auto-format only when media looks erased (new board / full chip
             * erase). Corrupted or non-empty media still needs console "format".
             */
            if (flash_looks_blank()) {
                board_uart_puts("LFS FMT\n");
                memset(&s_lfs, 0, sizeof(s_lfs));
                lfs_cfg_init();
                if (lfs_format(&s_lfs, &s_cfg) >= 0 &&
                        lfs_mount(&s_lfs, &s_cfg) >= 0) {
                    board_uart_puts("LFS OK\n");
                    s_ready = true;
                    return true;
                }
            }
            board_uart_puts("LFS NO\n");
            return false;
        }
    }
    board_uart_puts("LFS OK\n");
    s_ready = true;
    return true;
}

bool board_lfs_format(void)
{
    if (s_reader_open) {
        (void)board_lfs_read_close();
    }
    board_lfs_write_abort();
    if (s_ready) {
        (void)lfs_unmount(&s_lfs);
    }
    s_ready = false;
    lfs_cfg_init();
    if (lfs_format(&s_lfs, &s_cfg) < 0 || lfs_mount(&s_lfs, &s_cfg) < 0) {
        return false;
    }
    s_ready = true;
    return true;
}

bool board_lfs_ready(void)
{
    return s_ready;
}

lfs_t *board_lfs_get(void)
{
    return s_ready ? &s_lfs : NULL;
}

uint32_t board_lfs_capacity_bytes(void)
{
    return (uint32_t)s_cfg.block_count * s_cfg.block_size;
}

int board_lfs_file_info(const char *path, uint32_t *size, uint32_t *crc32)
{
    uint8_t chunk[64];
    lfs_file_t file;
    lfs_ssize_t n;
    uint32_t total = 0;
    uint32_t crc = 0xFFFFFFFFu;
    if (!s_ready || !path || !size || !crc32 || s_reader_open) {
        return -1;
    }
    if (file_open(&file, path, LFS_O_RDONLY, &s_read_fcfg,
            s_read_file_buf) < 0) {
        return -2;
    }
    while ((n = lfs_file_read(&s_lfs, &file, chunk, sizeof(chunk))) > 0) {
        lfs_ssize_t i;
        total += (uint32_t)n;
        for (i = 0; i < n; i++) {
            unsigned bit;
            crc ^= chunk[i];
            for (bit = 0; bit < 8u; bit++) {
                crc = (crc >> 1) ^ (0xEDB88320u &
                    (uint32_t)-(int32_t)(crc & 1u));
            }
        }
    }
    if (lfs_file_close(&s_lfs, &file) < 0 || n < 0) {
        return -3;
    }
    *size = total;
    *crc32 = crc ^ 0xFFFFFFFFu;
    return 0;
}

int board_lfs_read_file(const char *path, char *buf, size_t buflen)
{
    lfs_file_t f;
    lfs_ssize_t n;
    /* s_read_fcfg owns the single read cache; never share it with a stream. */
    if (!s_ready || !path || !buf || buflen == 0 || s_reader_open) {
        return -1;
    }
    if (file_open(&f, path, LFS_O_RDONLY, &s_read_fcfg,
            s_read_file_buf) < 0) {
        return -1;
    }
    n = lfs_file_read(&s_lfs, &f, buf, buflen - 1);
    lfs_file_close(&s_lfs, &f);
    if (n < 0) {
        return (int)n;
    }
    buf[n] = 0;
    return (int)n;
}

int board_lfs_write_file(const char *path, const char *data, size_t len)
{
    lfs_file_t f;
    lfs_ssize_t n;
    int cerr;
    if (!s_ready || !path || !data) {
        return -1;
    }
    if (file_open(&f, path, LFS_O_WRONLY | LFS_O_CREAT | LFS_O_TRUNC,
            &s_write_fcfg, s_write_file_buf) < 0) {
        return -1;
    }
    n = lfs_file_write(&s_lfs, &f, data, len);
    cerr = lfs_file_close(&s_lfs, &f);
    if (n < 0) {
        return (int)n;
    }
    if (cerr < 0) {
        return cerr;
    }
    return (int)n;
}

int board_lfs_remove(const char *path)
{
    if (!s_ready || !path) {
        return -1;
    }
    return lfs_remove(&s_lfs, path);
}

int board_lfs_read_open(const char *path)
{
    if (!s_ready || !path || s_reader_open) {
        return -1;
    }
    if (file_open(&s_reader, path, LFS_O_RDONLY, &s_read_fcfg,
            s_read_file_buf) < 0) {
        return -1;
    }
    s_reader_open = true;
    return 0;
}

int board_lfs_read_chunk(void *buf, size_t len)
{
    if (!s_reader_open || !buf) {
        return -1;
    }
    return (int)lfs_file_read(&s_lfs, &s_reader, buf, (lfs_size_t)len);
}

int board_lfs_read_close(void)
{
    int err;
    if (!s_reader_open) {
        return -1;
    }
    err = lfs_file_close(&s_lfs, &s_reader);
    s_reader_open = false;
    return err;
}

int board_lfs_write_open(const char *path)
{
    if (!s_ready || !path || s_writer_open) {
        return -1;
    }
    if (file_open(&s_writer, path, LFS_O_WRONLY | LFS_O_CREAT | LFS_O_TRUNC,
            &s_write_fcfg, s_write_file_buf) < 0) {
        return -1;
    }
    s_writer_open = true;
    return 0;
}

int board_lfs_write_chunk(const void *data, size_t len)
{
    if (!s_writer_open || (!data && len)) {
        return -1;
    }
    return (int)lfs_file_write(&s_lfs, &s_writer, data, (lfs_size_t)len);
}

int board_lfs_write_close(void)
{
    int err;
    if (!s_writer_open) {
        return -1;
    }
    err = lfs_file_close(&s_lfs, &s_writer);
    s_writer_open = false;
    return err;
}

void board_lfs_write_abort(void)
{
    if (s_writer_open) {
        (void)lfs_file_close(&s_lfs, &s_writer);
        s_writer_open = false;
    }
}

int board_lfs_replace(const char *src, const char *dst)
{
    if (!s_ready || !src || !dst || s_reader_open || s_writer_open) {
        return -1;
    }
    return lfs_rename(&s_lfs, src, dst);
}

int board_lfs_copy(const char *src, const char *dst)
{
    /* small stack: reuse via two-pass file_buf path not available; 64B chunks */
    char chunk[64];
    lfs_file_t fi, fo;
    lfs_ssize_t n, total = 0;
    /* This operation needs both static LittleFS file caches. */
    if (!s_ready || !src || !dst || s_reader_open || s_writer_open) {
        return -1;
    }
    if (file_open(&fi, src, LFS_O_RDONLY, &s_read_fcfg,
            s_read_file_buf) < 0) {
        return -1;
    }
    if (file_open(&fo, dst, LFS_O_WRONLY | LFS_O_CREAT | LFS_O_TRUNC,
            &s_write_fcfg, s_write_file_buf) < 0) {
        lfs_file_close(&s_lfs, &fi);
        return -1;
    }
    for (;;) {
        n = lfs_file_read(&s_lfs, &fi, chunk, sizeof(chunk));
        if (n <= 0) {
            if (n < 0) {
                total = n;
            }
            break;
        }
        if (lfs_file_write(&s_lfs, &fo, chunk, (lfs_size_t)n) != n) {
            total = -1;
            break;
        }
        total += n;
    }
    lfs_file_close(&s_lfs, &fi);
    lfs_file_close(&s_lfs, &fo);
    return (int)total;
}

int board_lfs_list(board_lfs_list_cb cb, void *ud)
{
    lfs_dir_t dir;
    struct lfs_info info;
    int n = 0;
    if (!s_ready || !cb) {
        return -1;
    }
    if (lfs_dir_open(&s_lfs, &dir, "/") < 0) {
        return -1;
    }
    while (lfs_dir_read(&s_lfs, &dir, &info) > 0) {
        if (info.type == LFS_TYPE_REG) {
            cb(info.name, (uint32_t)info.size, ud);
            n++;
        }
    }
    lfs_dir_close(&s_lfs, &dir);
    return n;
}
