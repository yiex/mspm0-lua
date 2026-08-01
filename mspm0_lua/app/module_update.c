#include "module_update.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

#include "board_crc.h"
#include "board_irq.h"
#include "board_lfs.h"
#include "board_uart.h"
#include "board_wdt.h"
#include "native_module.h"
#include "release_identity.h"
#include "ti_msp_dl_config.h"

#define NMUP_MAGIC              0x50554D4Eu
#define NMUP_FORMAT             1u
#define NMUP_HEADER_SIZE        32u
#define NMUP_ENTRY_SIZE         32u
#define NMUP_CRC_OFFSET         16u
#define NMUP_TABLE_SIZE         (NMUP_HEADER_SIZE + \
    NATIVE_MODULE_SLOT_COUNT * NMUP_ENTRY_SIZE)
#define NMUP_MAX_SIZE           (NMUP_TABLE_SIZE + \
    NATIVE_MODULE_SLOT_COUNT * NATIVE_MODULE_SLOT_SIZE)
#define FLASH_SECTOR_SIZE       1024u
#define IO_CHUNK                128u
#define PENDING_FILE            ".module.pending"
#define PENDING_TEMP            ".module.pending.tmp"

typedef struct {
    uint32_t image_size;
    uint32_t payload_offset;
    uint32_t image_crc32;
    uint16_t module_crc16;
    uint8_t present;
    char name[8];
} module_plan_entry_t;

typedef struct {
    uint32_t total_size;
    uint32_t bundle_crc32;
    uint8_t selected_count;
    module_plan_entry_t entry[NATIVE_MODULE_SLOT_COUNT];
} module_plan_t;

static module_plan_t s_plan;
static const char *s_error = "none";
static int name_valid(const char *name);

static uint16_t get_u16(const uint8_t *p)
{
    return (uint16_t)((uint16_t)p[0] | ((uint16_t)p[1] << 8));
}

static uint32_t get_u32(const uint8_t *p)
{
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) |
        ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}

static void put_u32_dec(uint32_t value)
{
    char out[11];
    char reverse[10];
    unsigned n = 0, i = 0;
    if (value == 0u) {
        board_uart_puts("0");
        return;
    }
    while (value && n < sizeof(reverse)) {
        reverse[n++] = (char)('0' + value % 10u);
        value /= 10u;
    }
    while (n) out[i++] = reverse[--n];
    out[i] = 0;
    board_uart_puts(out);
}

static void put_u32_hex(uint32_t value)
{
    static const char hex[] = "0123456789abcdef";
    char out[9];
    unsigned i;
    for (i = 0; i < 8u; i++) {
        out[7u - i] = hex[value & 0x0fu];
        value >>= 4;
    }
    out[8] = 0;
    board_uart_puts(out);
}

static uint32_t crc32_update(uint32_t crc, const uint8_t *data, size_t length)
{
    while (length--) {
        unsigned bit;
        crc ^= *data++;
        for (bit = 0; bit < 8u; bit++) {
            crc = (crc >> 1) ^ (0xEDB88320u &
                (uint32_t)-(int32_t)(crc & 1u));
        }
    }
    return crc;
}

static int pending_transaction_id(uint32_t *id)
{
    uint8_t header[NMUP_HEADER_SIZE];
    char path[32];
    int n = board_lfs_read_file(PENDING_FILE, path, sizeof(path));
    if (n < 0) return 0;
    if (n == 0 || n >= (int)sizeof(path) || strlen(path) != (size_t)n ||
            !name_valid(path)) return -1;
    if (board_lfs_read_open(path) < 0) return -1;
    n = board_lfs_read_chunk(header, sizeof(header));
    if (board_lfs_read_close() < 0 || n != (int)sizeof(header) ||
            get_u32(header) != NMUP_MAGIC) return -1;
    *id = get_u32(&header[NMUP_CRC_OFFSET]);
    return 1;
}

static uint16_t crc16_update(uint16_t crc, const uint8_t *data, size_t length)
{
    while (length--) {
        unsigned bit;
        crc ^= *data++;
        for (bit = 0; bit < 8u; bit++) {
            crc = (uint16_t)((crc >> 1) ^
                (uint16_t)(0xA001u & (uint16_t)-(int16_t)(crc & 1u)));
        }
    }
    return crc;
}

static int name_valid(const char *name)
{
    size_t i, length;
    if (!name) return 0;
    length = strlen(name);
    if (length == 0u || length > 28u) return 0;
    for (i = 0; i < length; i++) {
        char c = name[i];
        if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
                (c >= '0' && c <= '9') || c == '_' || c == '.' ||
                c == '-')) return 0;
    }
    return 1;
}

static int module_name_valid(const uint8_t *raw, char *name)
{
    unsigned i;
    int ended = 0;
    for (i = 0; i < 8u; i++) {
        uint8_t c = raw[i];
        if (ended) {
            if (c != 0u) return 0;
        } else if (c == 0u) {
            if (i == 0u) return 0;
            name[i] = 0;
            ended = 1;
        } else {
            if (!((c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') ||
                    c == '_')) return 0;
            name[i] = (char)c;
        }
    }
    if (!ended) return 0;
    name[7] = 0;
    return 1;
}

static int read_exact(void *buffer, size_t length)
{
    uint8_t *out = (uint8_t *)buffer;
    while (length) {
        int n = board_lfs_read_chunk(out, length);
        if (n <= 0) return -1;
        out += (size_t)n;
        length -= (size_t)n;
    }
    return 0;
}

static int skip_exact(uint32_t length)
{
    uint8_t buffer[IO_CHUNK];
    while (length) {
        size_t chunk = length > sizeof(buffer) ? sizeof(buffer) : length;
        if (read_exact(buffer, chunk) != 0) return -1;
        length -= (uint32_t)chunk;
    }
    return 0;
}

static int validate_header_and_table(const char *path)
{
    uint8_t raw[NMUP_ENTRY_SIZE];
    uint32_t expected_offset = NMUP_TABLE_SIZE;
    unsigned slot, count = 0;
    memset(&s_plan, 0, sizeof(s_plan));
    if (board_lfs_read_open(path) < 0) return -1;
    if (read_exact(raw, NMUP_HEADER_SIZE) != 0) goto bad;
    if (get_u32(raw) != NMUP_MAGIC || get_u16(raw + 4) != NMUP_FORMAT ||
            get_u16(raw + 6) != NATIVE_CORE_ABI_VERSION ||
            get_u16(raw + 8) != NMUP_TABLE_SIZE ||
            raw[10] != NATIVE_MODULE_SLOT_COUNT ||
            raw[11] > NATIVE_MODULE_SLOT_COUNT) goto bad;
    s_plan.selected_count = raw[11];
    s_plan.total_size = get_u32(raw + 12);
    s_plan.bundle_crc32 = get_u32(raw + 16);
    if (s_plan.total_size < NMUP_TABLE_SIZE ||
            s_plan.total_size > NMUP_MAX_SIZE) goto bad;
    for (slot = 20u; slot < NMUP_HEADER_SIZE; slot++) {
        if (raw[slot] != 0u) goto bad;
    }
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        module_plan_entry_t *entry = &s_plan.entry[slot];
        unsigned other;
        if (read_exact(raw, sizeof(raw)) != 0 || raw[0] > 1u ||
                raw[1] != slot || get_u16(raw + 2) != 0u ||
                get_u16(raw + 18) != 0u || get_u32(raw + 28) != 0u) goto bad;
        entry->present = raw[0];
        entry->image_size = get_u32(raw + 4);
        entry->payload_offset = get_u32(raw + 8);
        entry->image_crc32 = get_u32(raw + 12);
        entry->module_crc16 = get_u16(raw + 16);
        if (!entry->present) {
            unsigned i;
            if (entry->image_size || entry->payload_offset ||
                    entry->image_crc32 || entry->module_crc16) goto bad;
            for (i = 20u; i < 28u; i++) if (raw[i] != 0u) goto bad;
            continue;
        }
        count++;
        if (!module_name_valid(raw + 20, entry->name) ||
                entry->image_size <= NATIVE_MODULE_HEADER_SIZE ||
                entry->image_size > NATIVE_MODULE_SLOT_SIZE ||
                entry->payload_offset != expected_offset ||
                entry->image_size > s_plan.total_size - expected_offset) goto bad;
        for (other = 0; other < slot; other++) {
            if (s_plan.entry[other].present &&
                    strcmp(entry->name, s_plan.entry[other].name) == 0) goto bad;
        }
        expected_offset += entry->image_size;
    }
    {
        int close_status = board_lfs_read_close();
        if (count != s_plan.selected_count ||
                expected_offset != s_plan.total_size || close_status < 0)
            return -1;
    }
    return 0;
bad:
    (void)board_lfs_read_close();
    return -1;
}

static int validate_bundle_crc(const char *path)
{
    uint8_t buffer[IO_CHUNK];
    uint32_t offset = 0, crc = 0xFFFFFFFFu;
    if (board_lfs_read_open(path) < 0) return -1;
    while (offset < s_plan.total_size) {
        size_t want = s_plan.total_size - offset;
        size_t i;
        int n;
        if (want > sizeof(buffer)) want = sizeof(buffer);
        n = board_lfs_read_chunk(buffer, want);
        if (n <= 0) goto bad;
        for (i = 0; i < (size_t)n; i++) {
            uint8_t value = buffer[i];
            uint32_t position = offset + (uint32_t)i;
            if (position >= NMUP_CRC_OFFSET && position < NMUP_CRC_OFFSET + 4u)
                value = 0;
            crc = crc32_update(crc, &value, 1u);
        }
        offset += (uint32_t)n;
    }
    {
        int extra = board_lfs_read_chunk(buffer, 1u);
        int close_status = board_lfs_read_close();
        if (extra != 0 || close_status < 0 ||
                (crc ^ 0xFFFFFFFFu) != s_plan.bundle_crc32) return -1;
    }
    return 0;
bad:
    (void)board_lfs_read_close();
    return -1;
}

static int validate_module_payloads(const char *path)
{
    uint8_t buffer[IO_CHUNK];
    uint8_t header[NATIVE_MODULE_HEADER_SIZE];
    unsigned slot;
    if (board_lfs_read_open(path) < 0) return -1;
    if (skip_exact(NMUP_TABLE_SIZE) != 0) goto bad;
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        module_plan_entry_t *entry = &s_plan.entry[slot];
        uint32_t done = 0, crc32 = 0xFFFFFFFFu;
        uint16_t crc16 = 0xFFFFu;
        uintptr_t slot_address = NATIVE_MODULE_SLOT_ADDR +
            (uintptr_t)slot * NATIVE_MODULE_SLOT_SIZE;
        uintptr_t init, deinit;
        if (!entry->present) continue;
        while (done < entry->image_size) {
            size_t n = entry->image_size - done;
            size_t header_copy = 0;
            if (n > sizeof(buffer)) n = sizeof(buffer);
            if (read_exact(buffer, n) != 0) goto bad;
            if (done < sizeof(header)) {
                header_copy = sizeof(header) - done;
                if (header_copy > n) header_copy = n;
                memcpy(header + done, buffer, header_copy);
            }
            crc32 = crc32_update(crc32, buffer, n);
            if (done + n > sizeof(header)) {
                size_t start = done < sizeof(header) ? sizeof(header) - done : 0u;
                crc16 = crc16_update(crc16, buffer + start, n - start);
            }
            done += (uint32_t)n;
        }
        init = (uintptr_t)get_u32(header + 16);
        deinit = (uintptr_t)get_u32(header + 20);
        if ((crc32 ^ 0xFFFFFFFFu) != entry->image_crc32 ||
                crc16 != entry->module_crc16 ||
                get_u32(header) != NATIVE_MODULE_MAGIC ||
                get_u16(header + 4) != NATIVE_MODULE_FORMAT ||
                get_u16(header + 6) != NATIVE_CORE_ABI_VERSION ||
                get_u32(header + 8) != entry->image_size ||
                get_u16(header + 12) != entry->module_crc16 ||
                get_u16(header + 14) != NATIVE_MODULE_HEADER_SIZE ||
                memcmp(header + 24, entry->name, sizeof(entry->name)) != 0 ||
                (init & 1u) == 0u || (init & ~(uintptr_t)1u) <
                    slot_address + NATIVE_MODULE_HEADER_SIZE ||
                (init & ~(uintptr_t)1u) >= slot_address + entry->image_size ||
                (deinit && ((deinit & 1u) == 0u ||
                    (deinit & ~(uintptr_t)1u) <
                        slot_address + NATIVE_MODULE_HEADER_SIZE ||
                    (deinit & ~(uintptr_t)1u) >=
                        slot_address + entry->image_size))) goto bad;
    }
    if (board_lfs_read_close() < 0) return -1;
    return 0;
bad:
    (void)board_lfs_read_close();
    return -1;
}

static int validate_bundle(const char *path)
{
    if (!name_valid(path) || validate_header_and_table(path) != 0) {
        s_error = "header";
        return -1;
    }
    if (validate_bundle_crc(path) != 0) {
        s_error = "bundle-crc";
        return -1;
    }
    if (validate_module_payloads(path) != 0) {
        s_error = "module";
        return -1;
    }
    s_error = "none";
    return 0;
}

static DL_FLASHCTL_COMMAND_STATUS erase_sector(uint32_t address)
{
    DL_FLASHCTL_COMMAND_STATUS status;
    uint32_t primask = __get_PRIMASK();
    __disable_irq();
    DL_FlashCTL_executeClearStatus(FLASHCTL);
    DL_FlashCTL_unprotectSector(
        FLASHCTL, address, DL_FLASHCTL_REGION_SELECT_MAIN);
    status = DL_FlashCTL_eraseMemoryFromRAM(
        FLASHCTL, address, DL_FLASHCTL_COMMAND_SIZE_SECTOR);
    if (!primask) __enable_irq();
    return status;
}

static DL_FLASHCTL_COMMAND_STATUS program_words(
    uint32_t address, uint32_t *data, uint32_t words)
{
    DL_FLASHCTL_COMMAND_STATUS status;
    uint32_t primask = __get_PRIMASK();
    __disable_irq();
    status = DL_FlashCTL_programMemoryBlockingFromRAM64WithECCGenerated(
        FLASHCTL, address, data, words, DL_FLASHCTL_REGION_SELECT_MAIN);
    if (!primask) __enable_irq();
    return status;
}

static int program_slots(const char *path)
{
    uint32_t words[IO_CHUNK / sizeof(uint32_t)];
    uint8_t *buffer = (uint8_t *)words;
    unsigned slot;
    if (board_lfs_read_open(path) < 0) return -1;
    if (skip_exact(NMUP_TABLE_SIZE) != 0) goto bad;
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        module_plan_entry_t *entry = &s_plan.entry[slot];
        uint32_t address = NATIVE_MODULE_SLOT_ADDR +
            slot * NATIVE_MODULE_SLOT_SIZE;
        uint32_t sector, done = 0;
        board_uart_puts("MOD_ERASE ");
        put_u32_dec(slot);
        board_uart_puts("\n");
        for (sector = 0; sector < NATIVE_MODULE_SLOT_SIZE;
                sector += FLASH_SECTOR_SIZE) {
            if (erase_sector(address + sector) !=
                    DL_FLASHCTL_COMMAND_STATUS_PASSED) goto bad;
            board_wdt_feed();
        }
        if (!entry->present) continue;
        board_uart_puts("MOD_WRITE ");
        put_u32_dec(slot);
        board_uart_puts(" ");
        board_uart_puts(entry->name);
        board_uart_puts("\n");
        while (done < entry->image_size) {
            size_t n = entry->image_size - done;
            uint32_t programmed;
            if (n > sizeof(words)) n = sizeof(words);
            memset(buffer, 0xFF, sizeof(words));
            if (read_exact(buffer, n) != 0) goto bad;
            programmed = ((uint32_t)n + 7u) & ~7u;
            if (program_words(address + done, words, programmed / 4u) !=
                    DL_FLASHCTL_COMMAND_STATUS_PASSED) goto bad;
            done += (uint32_t)n;
            board_wdt_feed();
        }
    }
    if (board_lfs_read_close() < 0) return -1;
    return 0;
bad:
    (void)board_lfs_read_close();
    return -1;
}

static int verify_slots(const char *path)
{
    uint8_t buffer[IO_CHUNK];
    unsigned slot;
    if (board_lfs_read_open(path) < 0) return -1;
    if (skip_exact(NMUP_TABLE_SIZE) != 0) goto bad;
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        module_plan_entry_t *entry = &s_plan.entry[slot];
        uintptr_t address = NATIVE_MODULE_SLOT_ADDR +
            (uintptr_t)slot * NATIVE_MODULE_SLOT_SIZE;
        uint32_t done = 0;
        if (entry->present) {
            while (done < entry->image_size) {
                size_t n = entry->image_size - done;
                if (n > sizeof(buffer)) n = sizeof(buffer);
                if (read_exact(buffer, n) != 0 ||
                        memcmp((const void *)(address + done), buffer, n) != 0)
                    goto bad;
                done += (uint32_t)n;
            }
        }
        while (done < NATIVE_MODULE_SLOT_SIZE) {
            if (*(const volatile uint8_t *)(address + done) != 0xFFu) goto bad;
            done++;
        }
        board_wdt_feed();
    }
    if (board_lfs_read_close() < 0) return -1;
    return 0;
bad:
    (void)board_lfs_read_close();
    return -1;
}

int module_update_has_pending(void)
{
    int exists = board_lfs_read_open(PENDING_FILE) == 0;
    if (exists) (void)board_lfs_read_close();
    return exists;
}

static int file_exists(const char *path)
{
    int exists = board_lfs_read_open(path) == 0;
    if (exists) (void)board_lfs_read_close();
    return exists;
}

int module_update_stage(const char *path)
{
    size_t length;
    if (validate_bundle(path) != 0) return -1;
    length = strlen(path);
    (void)board_lfs_remove(PENDING_TEMP);
    if (board_lfs_write_file(PENDING_TEMP, path, length) != (int)length) {
        s_error = "pending-write";
        return -1;
    }
    if (board_lfs_replace(PENDING_TEMP, PENDING_FILE) < 0) {
        s_error = "pending-commit";
        return -1;
    }
    board_uart_puts("MOD_READY ");
    put_u32_dec(s_plan.selected_count);
    board_uart_puts(" ");
    put_u32_dec(s_plan.total_size);
    board_uart_puts("\n");
    return 0;
}

int module_update_apply_pending(void)
{
    char path[32];
    int n = board_lfs_read_file(PENDING_FILE, path, sizeof(path));
    int result = -1;
    uint32_t cache_config;
    if (n <= 0 || n >= (int)sizeof(path) || strlen(path) != (size_t)n ||
            !name_valid(path)) {
        s_error = "pending";
        return -1;
    }
    if (validate_bundle(path) != 0) return -1;
    board_uart_puts("MOD_APPLY ");
    board_uart_puts(path);
    board_uart_puts("\n");
    cache_config = DL_CORE_getInstructionConfig();
    DL_CORE_configInstruction(
        DL_CORE_CACHE_DISABLED, DL_CORE_PREFETCH_DISABLED,
        DL_CORE_LITERAL_CACHE_DISABLED);
    if (program_slots(path) != 0) {
        s_error = "flash";
        goto failed;
    }
    board_uart_puts("MOD_VERIFY\n");
    if (verify_slots(path) != 0) {
        s_error = "verify";
        goto failed;
    }
    result = 0;
failed:
    DL_FlashCTL_protectMainMemory(FLASHCTL);
    CPUSS->CTL = cache_config;
    __DSB();
    __ISB();
    /* Flash commands temporarily mask interrupts. Rebuild the system tick so
     * delay_ms(), soft timers and GPIO IRQs remain usable after a live update. */
    board_irq_init();
    if (result != 0) return -1;
    /* Never boot an entry script built for the previous module layout. The
     * host installs dependencies first and a new main.luac last. */
    if (file_exists("main.luac") && board_lfs_remove("main.luac") < 0) {
        s_error = "script-disable";
        return -1;
    }
    if (board_lfs_remove(PENDING_FILE) < 0) {
        s_error = "pending-clear";
        return -1;
    }
    (void)board_lfs_remove(path);
    board_uart_puts("MOD_DONE ");
    put_u32_dec(s_plan.selected_count);
    board_uart_puts("\n");
    s_error = "none";
    return 0;
}

const char *module_update_error(void)
{
    return s_error;
}

void module_update_report_status(void)
{
    unsigned slot;
    unsigned valid_count = 0;
    uint32_t pending_id = 0;
    uint32_t layout_crc = crc32_update(0xFFFFFFFFu,
        (const uint8_t *)(uintptr_t)NATIVE_MODULE_SLOT_ADDR,
        NATIVE_MODULE_SLOT_COUNT * NATIVE_MODULE_SLOT_SIZE) ^ 0xFFFFFFFFu;
    int pending_status = pending_transaction_id(&pending_id);
    board_uart_puts(module_update_has_pending() ?
        "MOD_STATUS PENDING\n" : "MOD_STATUS IDLE\n");
    board_uart_puts("MOD_CATALOG " FW_RELEASE_CATALOG_SHA256 "\n");
    for (slot = 0; slot < NATIVE_MODULE_SLOT_COUNT; slot++) {
        uintptr_t address = NATIVE_MODULE_SLOT_ADDR +
            (uintptr_t)slot * NATIVE_MODULE_SLOT_SIZE;
        const native_module_header_t *header =
            (const native_module_header_t *)address;
        char name[8];
        if (header->magic == 0xFFFFFFFFu) continue;
        board_uart_puts("MOD_SLOT ");
        put_u32_dec(slot);
        board_uart_puts(" ");
        if (header->magic != NATIVE_MODULE_MAGIC ||
                header->format_version != NATIVE_MODULE_FORMAT ||
                header->abi_version != NATIVE_CORE_ABI_VERSION ||
                header->header_size != NATIVE_MODULE_HEADER_SIZE ||
                header->image_size <= NATIVE_MODULE_HEADER_SIZE ||
                header->image_size > NATIVE_MODULE_SLOT_SIZE ||
                !module_name_valid((const uint8_t *)header->name, name) ||
                board_crc16_modbus((const uint8_t *)(address +
                    NATIVE_MODULE_HEADER_SIZE), header->image_size -
                    NATIVE_MODULE_HEADER_SIZE) != header->payload_crc16) {
            board_uart_puts("BAD\n");
        } else {
            uint32_t crc32 = crc32_update(0xFFFFFFFFu,
                (const uint8_t *)address, header->image_size) ^ 0xFFFFFFFFu;
            valid_count++;
            board_uart_puts(name);
            board_uart_puts(" ");
            put_u32_dec(header->image_size);
            board_uart_puts(" ");
            put_u32_hex(crc32);
            board_uart_puts("\n");
        }
    }
    board_uart_puts("MOD_LAYOUT ");
    put_u32_dec(valid_count);
    board_uart_puts(" ");
    put_u32_hex(layout_crc);
    board_uart_puts("\nMOD_PENDING ");
    if (pending_status > 0) {
        put_u32_hex(pending_id);
    } else {
        board_uart_puts(pending_status == 0 ? "none" : "invalid");
    }
    board_uart_puts("\n");
    board_uart_puts("MOD_STATUS_END\n");
}
