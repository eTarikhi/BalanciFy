#include <assert.h>
#include "../src/rx_pause_state.h"

int main(void) {
    BALANCIFY_NDIS_PAUSE_STATE state = {0u, 0u, 0u, 0u, 0u};

    /* An idle adapter can pause synchronously and rejects new receives. */
    assert(BalancifyBeginPause(&state) != 0);
    assert(state.paused != 0u);
    assert(state.pausePending == 0u);
    assert(BalancifyTrackReceiveIndication(&state) == 0);

    /* Pause waits for receive indications, returned NBLs, and sends. */
    BalancifyRestart(&state);
    assert(BalancifyTrackReceiveIndication(&state) != 0);
    assert(BalancifyTrackSendCallback(&state) != 0);
    assert(state.outstandingRxNbls == 1u);
    assert(state.indicationsInProgress == 1u);
    assert(state.sendCallbacksInProgress == 1u);
    assert(BalancifyBeginPause(&state) == 0);
    assert(state.pausePending != 0u);
    assert(BalancifyTrackReceiveIndication(&state) == 0);

    /* An inline receive return cannot complete pause before Indicate returns. */
    assert(BalancifyReceiveNblReturned(&state) == 0);
    assert(state.outstandingRxNbls == 0u);
    assert(state.indicationsInProgress == 1u);
    assert(BalancifyReceiveIndicationReturned(&state) == 0);
    assert(state.indicationsInProgress == 0u);
    assert(state.pausePending != 0u);
    assert(BalancifySendCallbackReturned(&state) != 0);
    assert(state.sendCallbacksInProgress == 0u);
    assert(state.pausePending == 0u);
    assert(BalancifySendCallbackReturned(&state) == 0);

    /* A rejected send callback arriving while paused also drains before completion. */
    assert(BalancifyTrackSendCallback(&state) != 0);
    assert(BalancifyBeginPause(&state) == 0);
    assert(BalancifySendCallbackReturned(&state) != 0);

    /* Never allow any outstanding-operation counter to wrap. */
    BalancifyRestart(&state);
    state.outstandingRxNbls = 0xFFFFFFFFu;
    state.indicationsInProgress = 0xFFFFFFFFu;
    state.sendCallbacksInProgress = 0xFFFFFFFFu;
    assert(BalancifyTrackReceiveIndication(&state) == 0);
    assert(BalancifyTrackSendCallback(&state) == 0);
    assert(state.outstandingRxNbls == 0xFFFFFFFFu);
    assert(state.indicationsInProgress == 0xFFFFFFFFu);
    assert(state.sendCallbacksInProgress == 0xFFFFFFFFu);

    return 0;
}
