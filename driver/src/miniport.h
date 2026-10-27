#pragma once

// Pin the miniport role and NDIS 6.8 API contract before including the WDK
// headers; ARM64 requires an explicit driver kind and a 6.3+ API selection.
#ifndef NDIS_MINIPORT_DRIVER
#define NDIS_MINIPORT_DRIVER 1
#endif
#ifndef NDIS680_MINIPORT
#define NDIS680_MINIPORT 1
#endif
#ifndef NDIS_SUPPORT_NDIS680
#define NDIS_SUPPORT_NDIS680 1
#endif

// The WDK headers intentionally use anonymous unions; keep /WX enabled for
// project code while allowing this documented MSVC extension in system headers.
#pragma warning(push)
#pragma warning(disable : 4201)
#include <ndis.h>
#include <wdf.h>
#pragma warning(pop)

#include "config_validation.h"
#include "rx_pause_state.h"
#include "tx_packet_queue.h"

#define BALANCIFY_TAG 'laBB' // 'BBal' reversed
#define BALANCIFY_POOL_TAG BALANCIFY_TAG

typedef struct _BALANCIFY_ADAPTER {
    NDIS_HANDLE miniportAdapterHandle;
    NDIS_HANDLE netBufferListPool;
    NDIS_HANDLE netBufferPool;
    WDFDEVICE controlDevice;
    NDIS_SPIN_LOCK lock;
    BALANCIFY_NDIS_PAUSE_STATE pauseState;
    BOOLEAN initialized;
    ULONG mtu;
    ULONG lookaheadSize;
    ULONG packetFilter;
    ULONG64 txPackets;
    ULONG64 rxPackets;
    ULONG64 txErrors;
    ULONG64 txBytes;
    ULONG64 rxBytes;
    BALANCIFY_TX_PACKET_QUEUE txQueue;
} BALANCIFY_ADAPTER, *PBALANCIFY_ADAPTER;

// Stored in each injected receive NBL until NDIS returns it. The packet bytes
// are separately allocated nonpaged memory, never the short-lived WDF request
// system buffer.
typedef struct _BALANCIFY_RX_NBL_CONTEXT {
    PVOID packetBuffer;
    ULONG packetLength;
} BALANCIFY_RX_NBL_CONTEXT, *PBALANCIFY_RX_NBL_CONTEXT;

NTSTATUS BalancifyCreateControlDevice(WDFDRIVER driver, PBALANCIFY_ADAPTER adapter);
