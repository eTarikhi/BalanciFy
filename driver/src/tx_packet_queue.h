#ifndef BALANCIFY_TX_PACKET_QUEUE_H
#define BALANCIFY_TX_PACKET_QUEUE_H

#include <stddef.h>
#include <stdint.h>

#define BALANCIFY_TX_QUEUE_CAPACITY 64u
#define BALANCIFY_TX_PACKET_MAX_BYTES 9000u
#define BALANCIFY_TX_PACKET_VERSION 1u

typedef struct _BALANCIFY_TX_PACKET_HEADER {
    uint32_t version;
    uint32_t packetLength;
    uint64_t sequence;
} BALANCIFY_TX_PACKET_HEADER;

typedef struct _BALANCIFY_TX_PACKET_SLOT {
    size_t length;
    uint64_t sequence;
    unsigned char bytes[BALANCIFY_TX_PACKET_MAX_BYTES];
} BALANCIFY_TX_PACKET_SLOT;

typedef struct _BALANCIFY_TX_PACKET_QUEUE {
    size_t head;
    size_t count;
    uint64_t nextSequence;
    BALANCIFY_TX_PACKET_SLOT slots[BALANCIFY_TX_QUEUE_CAPACITY];
} BALANCIFY_TX_PACKET_QUEUE;

/* All queue operations must be serialized by the adapter lock in the driver. */
static inline void BalancifyTxQueueInitialize(BALANCIFY_TX_PACKET_QUEUE *queue) {
    if (!queue) return;
    queue->head = 0;
    queue->count = 0;
    queue->nextSequence = 1;
}

static inline int BalancifyTxQueueCanEnqueue(
    const BALANCIFY_TX_PACKET_QUEUE *queue,
    size_t packetCount) {
    return queue && queue->head < BALANCIFY_TX_QUEUE_CAPACITY &&
        queue->count <= BALANCIFY_TX_QUEUE_CAPACITY &&
        packetCount <= BALANCIFY_TX_QUEUE_CAPACITY - queue->count;
}

static inline BALANCIFY_TX_PACKET_SLOT *BalancifyTxQueueReserveSlot(
    BALANCIFY_TX_PACKET_QUEUE *queue,
    size_t offset,
    size_t packetCount) {
    size_t index;
    if (!BalancifyTxQueueCanEnqueue(queue, packetCount) || offset >= packetCount) return NULL;
    index = (queue->head + queue->count + offset) % BALANCIFY_TX_QUEUE_CAPACITY;
    return &queue->slots[index];
}

/* Make a previously reserved batch visible atomically after all bytes validate. */
static inline int BalancifyTxQueueCommit(
    BALANCIFY_TX_PACKET_QUEUE *queue,
    size_t packetCount) {
    if (!BalancifyTxQueueCanEnqueue(queue, packetCount)) return 0;
    queue->count += packetCount;
    queue->nextSequence += (uint64_t)packetCount;
    return 1;
}

static inline const BALANCIFY_TX_PACKET_SLOT *BalancifyTxQueuePeek(
    const BALANCIFY_TX_PACKET_QUEUE *queue) {
    if (!queue || queue->head >= BALANCIFY_TX_QUEUE_CAPACITY || queue->count == 0 ||
        queue->count > BALANCIFY_TX_QUEUE_CAPACITY) return NULL;
    return &queue->slots[queue->head];
}

static inline int BalancifyTxQueuePop(BALANCIFY_TX_PACKET_QUEUE *queue) {
    if (!BalancifyTxQueuePeek(queue)) return 0;
    queue->head = (queue->head + 1) % BALANCIFY_TX_QUEUE_CAPACITY;
    queue->count--;
    return 1;
}

#endif /* BALANCIFY_TX_PACKET_QUEUE_H */
