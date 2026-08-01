#include "board_resource.h"
#include "board_pins.h"

#include <string.h>

static uint8_t s_owner[BOARD_RES_COUNT];

void board_resource_init(void)
{
    memset(s_owner, 0, sizeof(s_owner));
    s_owner[BOARD_RES_TIMG0] = PIN_OWN_SYS;
}

int board_resource_claim(board_resource_t resource, uint8_t owner)
{
    uint8_t current;
    if ((unsigned)resource >= BOARD_RES_COUNT || owner == PIN_OWN_FREE) {
        return -1;
    }
    current = s_owner[resource];
    if (current == PIN_OWN_FREE || current == owner) {
        s_owner[resource] = owner;
        return 0;
    }
    return current == PIN_OWN_SYS ? -2 : -3;
}

void board_resource_release(board_resource_t resource, uint8_t owner)
{
    if ((unsigned)resource < BOARD_RES_COUNT &&
            s_owner[resource] == owner && owner != PIN_OWN_SYS) {
        s_owner[resource] = PIN_OWN_FREE;
    }
}

void board_resource_release_owner(uint8_t owner)
{
    unsigned i;
    if (owner == PIN_OWN_FREE || owner == PIN_OWN_SYS) {
        return;
    }
    for (i = 0; i < BOARD_RES_COUNT; i++) {
        if (s_owner[i] == owner) {
            s_owner[i] = PIN_OWN_FREE;
        }
    }
}

void board_resource_reset_app(void)
{
    unsigned i;
    for (i = 0; i < BOARD_RES_COUNT; i++) {
        if (s_owner[i] != PIN_OWN_SYS) {
            s_owner[i] = PIN_OWN_FREE;
        }
    }
}

uint8_t board_resource_owner(board_resource_t resource)
{
    return (unsigned)resource < BOARD_RES_COUNT
        ? s_owner[resource] : PIN_OWN_SYS;
}
