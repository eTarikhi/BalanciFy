#ifndef BALANCIFY_CONFIG_VALIDATION_H
#define BALANCIFY_CONFIG_VALIDATION_H

#define BALANCIFY_DEFAULT_MTU 1440u
#define BALANCIFY_MIN_MTU 576u
#define BALANCIFY_MAX_MTU 9000u

/* Validate registry input and preserve a valid fallback for corrupt settings. */
static inline unsigned int BalancifyNormalizeMtu(unsigned int configured, unsigned int fallback) {
    if (configured >= BALANCIFY_MIN_MTU && configured <= BALANCIFY_MAX_MTU) {
        return configured;
    }
    if (fallback >= BALANCIFY_MIN_MTU && fallback <= BALANCIFY_MAX_MTU) {
        return fallback;
    }
    return BALANCIFY_DEFAULT_MTU;
}

#endif /* BALANCIFY_CONFIG_VALIDATION_H */
