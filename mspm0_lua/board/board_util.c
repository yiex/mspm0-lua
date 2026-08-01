#include "board_util.h"

int32_t board_clamp(int32_t v, int32_t lo, int32_t hi)
{
    if (lo > hi) {
        int32_t t = lo;
        lo = hi;
        hi = t;
    }
    if (v < lo) {
        return lo;
    }
    if (v > hi) {
        return hi;
    }
    return v;
}

int32_t board_deadzone(int32_t v, int32_t dz)
{
    if (dz < 0) {
        dz = -dz;
    }
    if (v > -dz && v < dz) {
        return 0;
    }
    return v;
}

int32_t board_map(int32_t x, int32_t in_lo, int32_t in_hi,
    int32_t out_lo, int32_t out_hi)
{
    int64_t den, num;
    if (in_hi == in_lo) {
        return out_lo;
    }
    den = (int64_t)in_hi - (int64_t)in_lo;
    num = (int64_t)(x - in_lo) * ((int64_t)out_hi - (int64_t)out_lo);
    return out_lo + (int32_t)(num / den);
}

int32_t board_med3(int32_t a, int32_t b, int32_t c)
{
    if (a > b) {
        int32_t t = a;
        a = b;
        b = t;
    }
    if (b > c) {
        int32_t t = b;
        b = c;
        c = t;
    }
    if (a > b) {
        int32_t t = a;
        a = b;
        b = t;
    }
    return b;
}

int32_t board_avg_n(const int32_t *x, int n)
{
    int64_t s = 0;
    int i;
    if (!x || n < 1) {
        return 0;
    }
    if (n > 32) {
        n = 32;
    }
    for (i = 0; i < n; i++) {
        s += x[i];
    }
    return (int32_t)(s / n);
}

int32_t board_slew(int32_t cur, int32_t tgt, int32_t rate)
{
    int32_t d;
    if (rate < 1) {
        rate = 1;
    }
    d = tgt - cur;
    if (d > rate) {
        return cur + rate;
    }
    if (d < -rate) {
        return cur - rate;
    }
    return tgt;
}

int32_t board_sign(int32_t v)
{
    if (v > 0) {
        return 1;
    }
    if (v < 0) {
        return -1;
    }
    return 0;
}
