#ifndef BALANCIFY_RX_PAUSE_STATE_H
#define BALANCIFY_RX_PAUSE_STATE_H

typedef struct _BALANCIFY_NDIS_PAUSE_STATE {
    unsigned int outstandingRxNbls;
    unsigned int indicationsInProgress;
    unsigned int sendCallbacksInProgress;
    unsigned int paused;
    unsigned int pausePending;
} BALANCIFY_NDIS_PAUSE_STATE;

static inline int BalancifyPauseIsDrained(const BALANCIFY_NDIS_PAUSE_STATE *state) {
    return state->outstandingRxNbls == 0 &&
        state->indicationsInProgress == 0 &&
        state->sendCallbacksInProgress == 0;
}

static inline int BalancifyPauseCompleteIfDrained(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->paused && state->pausePending && BalancifyPauseIsDrained(state)) {
        state->pausePending = 0;
        return 1;
    }
    return 0;
}

/* Call all state transition helpers only while the adapter lock is held. */
static inline int BalancifyTrackReceiveIndication(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->paused || state->outstandingRxNbls == 0xFFFFFFFFu ||
        state->indicationsInProgress == 0xFFFFFFFFu) return 0;
    state->outstandingRxNbls++;
    state->indicationsInProgress++;
    return 1;
}

static inline int BalancifyReceiveIndicationReturned(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->indicationsInProgress != 0) state->indicationsInProgress--;
    return BalancifyPauseCompleteIfDrained(state);
}

static inline int BalancifyReceiveNblReturned(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->outstandingRxNbls != 0) state->outstandingRxNbls--;
    return BalancifyPauseCompleteIfDrained(state);
}

static inline int BalancifyTrackSendCallback(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->sendCallbacksInProgress == 0xFFFFFFFFu) return 0;
    state->sendCallbacksInProgress++;
    return 1;
}

static inline int BalancifySendCallbackReturned(BALANCIFY_NDIS_PAUSE_STATE *state) {
    if (state->sendCallbacksInProgress != 0) state->sendCallbacksInProgress--;
    return BalancifyPauseCompleteIfDrained(state);
}

/* Return nonzero when MiniportPause can complete synchronously. */
static inline int BalancifyBeginPause(BALANCIFY_NDIS_PAUSE_STATE *state) {
    state->paused = 1;
    if (BalancifyPauseIsDrained(state)) {
        state->pausePending = 0;
        return 1;
    }
    state->pausePending = 1;
    return 0;
}

static inline void BalancifyRestart(BALANCIFY_NDIS_PAUSE_STATE *state) {
    state->paused = 0;
    state->pausePending = 0;
}

#endif /* BALANCIFY_RX_PAUSE_STATE_H */
