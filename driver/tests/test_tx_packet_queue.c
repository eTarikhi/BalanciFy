#include <assert.h>
#include <stddef.h>
#include <stdint.h>

#include "../src/tx_packet_queue.h"

_Static_assert(sizeof(BALANCIFY_TX_PACKET_HEADER) == 16, "TX wire header must stay 16 bytes");

static void test_empty_and_atomic_batch(void) {
    BALANCIFY_TX_PACKET_QUEUE queue;
    BALANCIFY_TX_PACKET_SLOT *first;
    BALANCIFY_TX_PACKET_SLOT *second;
    const BALANCIFY_TX_PACKET_SLOT *head;

    BalancifyTxQueueInitialize(&queue);
    assert(queue.count == 0);
    assert(BalancifyTxQueuePeek(&queue) == NULL);
    assert(BalancifyTxQueueCanEnqueue(&queue, 2));

    first = BalancifyTxQueueReserveSlot(&queue, 0, 2);
    second = BalancifyTxQueueReserveSlot(&queue, 1, 2);
    assert(first != NULL && second != NULL);
    first->length = 2;
    first->bytes[0] = 0x45;
    first->bytes[1] = 0x01;
    first->sequence = queue.nextSequence;
    second->length = 1;
    second->bytes[0] = 0x60;
    second->sequence = queue.nextSequence + 1;

    // Reserved slots do not become visible until the entire batch commits.
    assert(BalancifyTxQueuePeek(&queue) == NULL);
    assert(BalancifyTxQueueCommit(&queue, 2));
    assert(queue.count == 2);
    head = BalancifyTxQueuePeek(&queue);
    assert(head != NULL && head->sequence == 1 && head->length == 2);
    assert(head->bytes[0] == 0x45 && head->bytes[1] == 0x01);
    assert(BalancifyTxQueuePop(&queue));
    head = BalancifyTxQueuePeek(&queue);
    assert(head != NULL && head->sequence == 2 && head->length == 1);
    assert(head->bytes[0] == 0x60);
    assert(BalancifyTxQueuePop(&queue));
    assert(BalancifyTxQueuePeek(&queue) == NULL);
    assert(!BalancifyTxQueuePop(&queue));
}

static void test_capacity_and_fifo_wraparound(void) {
    BALANCIFY_TX_PACKET_QUEUE queue;
    size_t index;

    BalancifyTxQueueInitialize(&queue);
    for (index = 0; index < BALANCIFY_TX_QUEUE_CAPACITY; index++) {
        BALANCIFY_TX_PACKET_SLOT *slot = BalancifyTxQueueReserveSlot(&queue, 0, 1);
        assert(slot != NULL);
        slot->length = 1;
        slot->bytes[0] = (unsigned char)index;
        slot->sequence = queue.nextSequence;
        assert(BalancifyTxQueueCommit(&queue, 1));
    }
    assert(queue.count == BALANCIFY_TX_QUEUE_CAPACITY);
    assert(!BalancifyTxQueueCanEnqueue(&queue, 1));
    assert(BalancifyTxQueueReserveSlot(&queue, 0, 1) == NULL);

    for (index = 0; index < BALANCIFY_TX_QUEUE_CAPACITY / 2; index++) {
        const BALANCIFY_TX_PACKET_SLOT *head = BalancifyTxQueuePeek(&queue);
        assert(head != NULL);
        assert(head->sequence == (uint64_t)(index + 1));
        assert(head->bytes[0] == (unsigned char)index);
        assert(BalancifyTxQueuePop(&queue));
    }

    for (index = BALANCIFY_TX_QUEUE_CAPACITY;
         index < BALANCIFY_TX_QUEUE_CAPACITY + BALANCIFY_TX_QUEUE_CAPACITY / 2;
         index++) {
        BALANCIFY_TX_PACKET_SLOT *slot = BalancifyTxQueueReserveSlot(&queue, 0, 1);
        assert(slot != NULL);
        slot->length = 1;
        slot->bytes[0] = (unsigned char)index;
        slot->sequence = queue.nextSequence;
        assert(BalancifyTxQueueCommit(&queue, 1));
    }
    assert(queue.count == BALANCIFY_TX_QUEUE_CAPACITY);

    for (index = BALANCIFY_TX_QUEUE_CAPACITY / 2;
         index < BALANCIFY_TX_QUEUE_CAPACITY + BALANCIFY_TX_QUEUE_CAPACITY / 2;
         index++) {
        const BALANCIFY_TX_PACKET_SLOT *head = BalancifyTxQueuePeek(&queue);
        assert(head != NULL);
        assert(head->sequence == (uint64_t)(index + 1));
        assert(head->bytes[0] == (unsigned char)index);
        assert(BalancifyTxQueuePop(&queue));
    }
    assert(queue.count == 0);
}

static void test_reject_invalid_batch_sizes(void) {
    BALANCIFY_TX_PACKET_QUEUE queue;
    BalancifyTxQueueInitialize(&queue);
    assert(!BalancifyTxQueueCanEnqueue(&queue, BALANCIFY_TX_QUEUE_CAPACITY + 1u));
    assert(BalancifyTxQueueReserveSlot(&queue, 0, BALANCIFY_TX_QUEUE_CAPACITY + 1u) == NULL);
    assert(!BalancifyTxQueueCommit(&queue, BALANCIFY_TX_QUEUE_CAPACITY + 1u));
    assert(queue.count == 0);
}

static void test_sequence_exhaustion_fails_closed(void) {
    BALANCIFY_TX_PACKET_QUEUE queue;
    BALANCIFY_TX_PACKET_SLOT *slot;
    const BALANCIFY_TX_PACKET_SLOT *head;

    BalancifyTxQueueInitialize(&queue);
    queue.nextSequence = UINT64_MAX - 1u;
    assert(!BalancifyTxQueueCanEnqueue(&queue, 2));
    assert(!BalancifyTxQueueCommit(&queue, 2));
    assert(BalancifyTxQueueCanEnqueue(&queue, 1));
    slot = BalancifyTxQueueReserveSlot(&queue, 0, 1);
    assert(slot != NULL);
    slot->length = 1;
    slot->bytes[0] = 0x45;
    slot->sequence = queue.nextSequence;
    assert(BalancifyTxQueueCommit(&queue, 1));
    head = BalancifyTxQueuePeek(&queue);
    assert(head != NULL && head->sequence == UINT64_MAX - 1u);
    assert(queue.nextSequence == UINT64_MAX);

    // Exhaustion must reject new records rather than wrap IDs back to zero.
    assert(!BalancifyTxQueueCanEnqueue(&queue, 1));
    assert(BalancifyTxQueueReserveSlot(&queue, 0, 1) == NULL);
    assert(!BalancifyTxQueueCommit(&queue, 1));
    assert(queue.count == 1);
    assert(BalancifyTxQueuePop(&queue));
    assert(queue.count == 0);
    assert(!BalancifyTxQueueCanEnqueue(&queue, 1));

    // A zero next-sequence value is an invalid/corrupt queue state as well.
    queue.nextSequence = 0;
    assert(!BalancifyTxQueueCanEnqueue(&queue, 1));
}

int main(void) {
    test_empty_and_atomic_batch();
    test_capacity_and_fifo_wraparound();
    test_reject_invalid_batch_sizes();
    test_sequence_exhaustion_fails_closed();
    return 0;
}
