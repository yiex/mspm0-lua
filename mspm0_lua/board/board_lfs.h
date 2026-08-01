#ifndef BOARD_LFS_H
#define BOARD_LFS_H

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>
#include "lfs.h"

bool board_lfs_init(void);
bool board_lfs_ready(void);
bool board_lfs_format(void);
int board_lfs_read_file(const char *path, char *buf, size_t buflen);
int board_lfs_write_file(const char *path, const char *data, size_t len);
int board_lfs_remove(const char *path);
uint32_t board_lfs_capacity_bytes(void);
/* Query an exact regular-file size and IEEE CRC32 without loading it in RAM. */
int board_lfs_file_info(const char *path, uint32_t *size, uint32_t *crc32);

/* One sequential reader and one sequential writer may be open together. */
int board_lfs_read_open(const char *path);
int board_lfs_read_chunk(void *buf, size_t len);
int board_lfs_read_close(void);
int board_lfs_write_open(const char *path);
int board_lfs_write_chunk(const void *data, size_t len);
int board_lfs_write_close(void);
void board_lfs_write_abort(void);
int board_lfs_replace(const char *src, const char *dst);
/* copy src -> dst (overwrite). Returns bytes or <0 */
/* copy needs both static stream caches, so no sequential stream may be open. */
int board_lfs_copy(const char *src, const char *dst);
/* list root; cb(name, size, ud). Returns count or <0 */
typedef void (*board_lfs_list_cb)(const char *name, uint32_t size, void *ud);
int board_lfs_list(board_lfs_list_cb cb, void *ud);
lfs_t *board_lfs_get(void);

#endif
