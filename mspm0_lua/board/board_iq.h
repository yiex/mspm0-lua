#ifndef BOARD_IQ_H
#define BOARD_IQ_H

#include <stdint.h>

/*
 * Compact IQ16 fixed-point (Q16.16) for C hot paths.
 * Mirrors TI IQmath *usage* (see SDK iqmath_*_ops_test) without linking
 * the full iqmath.a (tables alone exceed our Flash free margin).
 *
 * 1.0 == 65536. Range roughly ±32767.
 */
typedef int32_t iq16_t;

#define IQ16_ONE   ((iq16_t)65536)
#define IQ16_HALF  ((iq16_t)32768)
#define IQ16_PI    ((iq16_t)205887)   /* pi ≈ 3.14159 */
#define IQ16_2PI   ((iq16_t)411775)

/* Integer / scaled decimal ↔ IQ16 */
iq16_t iq16_from_i(int32_t i);
iq16_t iq16_from_x10(int32_t x10);   /* 15  → 1.5 */
iq16_t iq16_from_x100(int32_t x100); /* 150 → 1.50 */
iq16_t iq16_from_x1000(int32_t x1000);
int32_t iq16_to_i(iq16_t q);         /* trunc toward 0 */
int32_t iq16_to_x10(iq16_t q);       /* rounded */
int32_t iq16_to_x100(iq16_t q);
int32_t iq16_to_x1000(iq16_t q);

iq16_t iq16_mul(iq16_t a, iq16_t b);
iq16_t iq16_div(iq16_t a, iq16_t b);
iq16_t iq16_abs(iq16_t a);
iq16_t iq16_sat(iq16_t a, iq16_t lim); /* clamp to ±lim */

/* Angle helpers: degrees in x10 (IMU style) → sin/cos as IQ16. */
iq16_t iq16_sin_deg_x10(int32_t deg_x10);
iq16_t iq16_cos_deg_x10(int32_t deg_x10);

/* atan2(y,x) → degrees ×10 in [-1800, 1800]. */
int32_t iq16_atan2_deg_x10(iq16_t y, iq16_t x);

#endif
