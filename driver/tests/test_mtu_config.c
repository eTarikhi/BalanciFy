#include <assert.h>
#include "../src/config_validation.h"

int main(void) {
    assert(BalancifyNormalizeMtu(BALANCIFY_MIN_MTU, BALANCIFY_DEFAULT_MTU) == BALANCIFY_MIN_MTU);
    assert(BalancifyNormalizeMtu(BALANCIFY_MAX_MTU, BALANCIFY_DEFAULT_MTU) == BALANCIFY_MAX_MTU);
    assert(BalancifyNormalizeMtu(1440u, BALANCIFY_DEFAULT_MTU) == 1440u);
    assert(BalancifyNormalizeMtu(0u, BALANCIFY_DEFAULT_MTU) == BALANCIFY_DEFAULT_MTU);
    assert(BalancifyNormalizeMtu(BALANCIFY_MIN_MTU - 1u, BALANCIFY_DEFAULT_MTU) == BALANCIFY_DEFAULT_MTU);
    assert(BalancifyNormalizeMtu(BALANCIFY_MAX_MTU + 1u, BALANCIFY_DEFAULT_MTU) == BALANCIFY_DEFAULT_MTU);
    assert(BalancifyNormalizeMtu(1200u, 0u) == 1200u);
    assert(BalancifyNormalizeMtu(0u, 0u) == BALANCIFY_DEFAULT_MTU);
    return 0;
}
