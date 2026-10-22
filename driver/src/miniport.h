#pragma once

// The WDK headers intentionally use anonymous unions; keep /WX enabled for
// project code while allowing this documented MSVC extension in system headers.
#pragma warning(push)
#pragma warning(disable : 4201)
#include <ndis.h>
#include <wdf.h>
#pragma warning(pop)

#include "config_validation.h"
#include "rx_pause_state.h"

#define BALANCIFY_TAG 'laBB' // 'BBal' reversed
#define BALANCIFY_POOL_TAG BALANCIFY_TAG

typedef struct _BALANCIFY_ADAPTER {
    NDIS_HANDLE miniportAdapterHandle;
    NDIS_HANDLE netBufferListPool;
    NDIS_HANDLE netBufferPool;
    WDFDEVICE controlDevice;
    NDIS_SPIN_LOCK lock;
    BALANCIFY_NDIS_PAUSE_STATE pauseState;
    ULONG mtu;
    ULONG lookaheadSize;
    ULONG packetFilter;
    ULONG64 txPackets;
    ULONG64 rxPackets;
    ULONG64 txErrors;
    ULONG64 txBytes;
    ULONG64 rxBytes;
} BALANCIFY_ADAPTER, *PBALANCIFY_ADAPTER;

// Stored in each injected receive NBL until NDIS returns it. The packet bytes
// are separately allocated nonpaged memory, never the short-lived WDF request
// system buffer.
typedef struct _BALANCIFY_RX_NBL_CONTEXT {
    PVOID packetBuffer;
    ULONG packetLength;
} BALANCIFY_RX_NBL_CONTEXT, *PBALANCIFY_RX_NBL_CONTEXT;

NTSTATUS BalancifyCreateControlDevice(WDFDRIVER driver, PBALANCIFY_ADAPTER adapter);
