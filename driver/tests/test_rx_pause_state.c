#include <assert.h>
#include "../src/rx_pause_state.h"

int main(void) {
    BALANCIFY_RX_PAUSE_STATE state = {0u, 0u, 0u};

    /* An idle adapter can pause synchronously and rejects new receives. */
    assert(BalancifyRxBeginPause(&state) != 0);
    assert(state.paused != 0u);
    assert(state.pausePending == 0u);
    assert(BalancifyRxTrackIndication(&state) == 0);

    /* Restart permits indications again. Pause waits for every returned NBL. */
    BalancifyRxRestart(&state);
    assert(BalancifyRxTrackIndication(&state) != 0);
    assert(BalancifyRxTrackIndication(&state) != 0);
    assert(state.outstandingRxNbls == 2u);
    assert(BalancifyRxBeginPause(&state) == 0);
    assert(state.pausePending != 0u);
    assert(BalancifyRxTrackIndication(&state) == 0);
    assert(BalancifyRxReturnNbl(&state) == 0);
    assert(state.outstandingRxNbls == 1u);
    assert(state.pausePending != 0u);
    assert(BalancifyRxReturnNbl(&state) != 0);
    assert(state.outstandingRxNbls == 0u);
    assert(state.pausePending == 0u);
    assert(BalancifyRxReturnNbl(&state) == 0);

    /* Never allow the outstanding-NBL counter to wrap. */
    state.outstandingRxNbls = 0xFFFFFFFFu;
    BalancifyRxRestart(&state);
    assert(BalancifyRxTrackIndication(&state) == 0);
    assert(state.outstandingRxNbls == 0xFFFFFFFFu);

    return 0;
}
