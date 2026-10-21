#ifndef BALANCIFY_RX_PAUSE_STATE_H
#define BALANCIFY_RX_PAUSE_STATE_H

typedef struct _BALANCIFY_RX_PAUSE_STATE {
    unsigned int outstandingRxNbls;
    unsigned int paused;
    unsigned int pausePending;
} BALANCIFY_RX_PAUSE_STATE;

/* Call only while the adapter lock is held. */
static inline int BalancifyRxTrackIndication(BALANCIFY_RX_PAUSE_STATE *state) {
    if (state->paused || state->outstandingRxNbls == 0xFFFFFFFFu) return 0;
    state->outstandingRxNbls++;
    return 1;
}

/* Return nonzero when MiniportPause can complete synchronously. */
static inline int BalancifyRxBeginPause(BALANCIFY_RX_PAUSE_STATE *state) {
    state->paused = 1;
    if (state->outstandingRxNbls != 0) {
        state->pausePending = 1;
        return 0;
    }
    state->pausePending = 0;
    return 1;
}

/* Return nonzero exactly once when a pending pause becomes quiescent. */
static inline int BalancifyRxReturnNbl(BALANCIFY_RX_PAUSE_STATE *state) {
    if (state->outstandingRxNbls != 0) state->outstandingRxNbls--;
    if (state->paused && state->pausePending && state->outstandingRxNbls == 0) {
        state->pausePending = 0;
        return 1;
    }
    return 0;
}

static inline void BalancifyRxRestart(BALANCIFY_RX_PAUSE_STATE *state) {
    state->paused = 0;
    state->pausePending = 0;
}

#endif /* BALANCIFY_RX_PAUSE_STATE_H */
