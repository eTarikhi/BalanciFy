// Balancify NDIS virtual miniport — Phase 2 S2b skeleton
// This file documents the intended driver layout and compiles on WDK.
// It is not yet wired to the service data path; S2b requires test-signed
// load with Driver Verifier and packet loopback through it.
//
// Build: msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64
// Requires WDK 10.0.22621+, SDK 10.0.22621+.
//
// References:
// - NDIS 6.8 miniport driver model (Windows Driver Kit docs)
// - blueprint §6.1 TAP-like virtual adapter, §4.5 pause/resume, §6.6 NAT keepalive
// - No third-party kernel component in data path (C2): only NDIS + WDF

#include <ndis.h>
#include <wdf.h>

#define BALANCIFY_TAG 'laBB' // 'BBal' reversed
#define BALANCIFY_POOL_TAG BALANCIFY_TAG

// Adapter context — one per virtual NIC
typedef struct _BALANCIFY_ADAPTER {
    NDIS_HANDLE miniportAdapterHandle;
    WDFDEVICE controlDevice; // for IOCTLs from service (WDF)
    NDIS_SPIN_LOCK lock;
    BOOLEAN paused;
    ULONG mtu; // §6.1 adapter MTU, follows narrowest live path
    // Stats for OID_GEN_STATISTICS
    ULONG64 txPackets;
    ULONG64 rxPackets;
    ULONG64 txBytes;
    ULONG64 rxBytes;
    // Service ring: shared memory for packet exchange (planned)
    // For now, packets are looped back in MiniportSendNetBufferLists
} BALANCIFY_ADAPTER, *PBALANCIFY_ADAPTER;

// Forward decls
DRIVER_INITIALIZE DriverEntry;
DRIVER_UNLOAD DriverUnload;
MINIPORT_INITIALIZE MiniportInitializeEx;
MINIPORT_HALT MiniportHaltEx;
MINIPORT_PAUSE MiniportPause;
MINIPORT_RESTART MiniportRestart;
MINIPORT_SEND_NET_BUFFER_LISTS MiniportSendNetBufferLists;
MINIPORT_RETURN_NET_BUFFER_LISTS MiniportReturnNetBufferLists;
MINIPORT_OID_REQUEST MiniportOidRequest;
MINIPORT_SHUTDOWN MiniportShutdownEx;
MINIPORT_DEVICE_PNP_EVENT_NOTIFY MiniportDevicePnPEventNotify;

static NDIS_HANDLE gDriverHandle = NULL;
static NDIS_HANDLE gNdisDriverContext = NULL;

// Miniport characteristics
static NDIS_MINIPORT_DRIVER_CHARACTERISTICS gCharacteristics = {
    .Header = { .Type = NDIS_OBJECT_TYPE_MINIPORT_DRIVER_CHARACTERISTICS,
                .Revision = NDIS_MINIPORT_DRIVER_CHARACTERISTICS_REVISION_2,
                .Size = NDIS_SIZEOF_MINIPORT_DRIVER_CHARACTERISTICS_REVISION_2 },
    .MajorNdisVersion = 6,
    .MinorNdisVersion = 80,
    .MajorDriverVersion = 0,
    .MinorDriverVersion = 1,
    .InitializeHandlerEx = MiniportInitializeEx,
    .HaltHandlerEx = MiniportHaltEx,
    .PauseHandler = MiniportPause,
    .RestartHandler = MiniportRestart,
    .OidRequestHandler = MiniportOidRequest,
    .SendNetBufferListsHandler = MiniportSendNetBufferLists,
    .ReturnNetBufferListsHandler = MiniportReturnNetBufferLists,
    .ShutdownHandlerEx = MiniportShutdownEx,
    .DevicePnPEventNotifyHandler = MiniportDevicePnPEventNotify,
    .CancelSendHandler = NULL,
    .CheckForHangHandlerEx = NULL,
    .ResetHandlerEx = NULL,
};

_Use_decl_annotations_
NDIS_STATUS DriverEntry(PDRIVER_OBJECT DriverObject, PUNICODE_STRING RegistryPath) {
    NDIS_STATUS status;
    WDF_DRIVER_CONFIG wdfConfig;
    UNREFERENCED_PARAMETER(RegistryPath);

    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: DriverEntry\n"));

    WDF_DRIVER_CONFIG_INIT(&wdfConfig, WDF_NO_EVENT_CALLBACK);
    wdfConfig.DriverInitFlags |= WdfDriverInitNonPnpDriver;
    wdfConfig.EvtDriverUnload = DriverUnload;

    // In real driver, WdfDriverCreate + NdisMRegisterMiniportDriver
    // For skeleton, just register NDIS
    status = NdisMRegisterMiniportDriver(
        DriverObject, RegistryPath, gNdisDriverContext,
        &gCharacteristics, &gDriverHandle);
    if (status != NDIS_STATUS_SUCCESS) {
        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL, "Balancify: NdisMRegisterMiniportDriver failed 0x%x\n", status));
        return status;
    }

    return NDIS_STATUS_SUCCESS;
}

_Use_decl_annotations_
VOID DriverUnload(PDRIVER_OBJECT DriverObject) {
    UNREFERENCED_PARAMETER(DriverObject);
    if (gDriverHandle) {
        NdisMDeregisterMiniportDriver(gDriverHandle);
        gDriverHandle = NULL;
    }
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: DriverUnload\n"));
}

_Use_decl_annotations_
NDIS_STATUS MiniportInitializeEx(
    NDIS_HANDLE miniportAdapterHandle,
    NDIS_HANDLE miniportDriverContext,
    PNDIS_MINIPORT_INIT_PARAMETERS miniportInitParameters) {
    PBALANCIFY_ADAPTER adapter = NULL;
    NDIS_STATUS status = NDIS_STATUS_SUCCESS;
    UNREFERENCED_PARAMETER(miniportDriverContext);
    UNREFERENCED_PARAMETER(miniportInitParameters);

    adapter = (PBALANCIFY_ADAPTER)NdisAllocateMemoryWithTagPriority(
        miniportAdapterHandle, sizeof(BALANCIFY_ADAPTER), BALANCIFY_POOL_TAG, NormalPoolPriority);
    if (!adapter) return NDIS_STATUS_RESOURCES;

    NdisZeroMemory(adapter, sizeof(BALANCIFY_ADAPTER));
    adapter->miniportAdapterHandle = miniportAdapterHandle;
    adapter->mtu = 1440; // §6.1 default, updated from service via IOCTL
    NdisAllocateSpinLock(&adapter->lock);

    NdisMSetMiniportAttributes(miniportAdapterHandle,
        (PNDIS_MINIPORT_ADAPTER_ATTRIBUTES)NULL); // TODO: set registration attributes

    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportInitializeEx mtu=%lu\n", adapter->mtu));
    return status;
}

_Use_decl_annotations_
VOID MiniportHaltEx(NDIS_HANDLE miniportAdapterContext, NDIS_HALT_ACTION haltAction) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    UNREFERENCED_PARAMETER(haltAction);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportHaltEx\n"));
    if (adapter) {
        NdisFreeSpinLock(&adapter->lock);
        NdisFreeMemory(adapter, sizeof(BALANCIFY_ADAPTER), 0);
    }
}

_Use_decl_annotations_
NDIS_STATUS MiniportPause(NDIS_HANDLE miniportAdapterContext, PNDIS_MINIPORT_PAUSE_PARAMETERS pauseParameters) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    UNREFERENCED_PARAMETER(pauseParameters);
    NdisAcquireSpinLock(&adapter->lock);
    adapter->paused = TRUE;
    NdisReleaseSpinLock(&adapter->lock);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportPause (§4.5 sleep)\n"));
    return NDIS_STATUS_SUCCESS;
}

_Use_decl_annotations_
NDIS_STATUS MiniportRestart(
    NDIS_HANDLE miniportAdapterContext,
    PNDIS_MINIPORT_RESTART_PARAMETERS restartParameters) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    UNREFERENCED_PARAMETER(restartParameters);
    NdisAcquireSpinLock(&adapter->lock);
    adapter->paused = FALSE;
    NdisReleaseSpinLock(&adapter->lock);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportRestart (§4.5 resume)\n"));
    return NDIS_STATUS_SUCCESS;
}

_Use_decl_annotations_
VOID MiniportSendNetBufferLists(
    NDIS_HANDLE miniportAdapterContext,
    PNET_BUFFER_LIST netBufferLists,
    NDIS_PORT_NUMBER portNumber,
    ULONG sendFlags) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    PNET_BUFFER_LIST curr = netBufferLists;
    UNREFERENCED_PARAMETER(portNumber);

    while (curr) {
        PNET_BUFFER_LIST next = NET_BUFFER_LIST_NEXT_NBL(curr);
        NET_BUFFER_LIST_NEXT_NBL(curr) = NULL;

        // For skeleton: count and complete immediately.
        // Real implementation: copy to service ring, service does tunnel send,
        // then NdisMSendNetBufferListsComplete.
        NdisAcquireSpinLock(&adapter->lock);
        adapter->txPackets++;
        // Approximate bytes: sum NET_BUFFER data length
        for (PNET_BUFFER nb = NET_BUFFER_LIST_FIRST_NB(curr); nb; nb = NET_BUFFER_NEXT_NB(nb)) {
            adapter->txBytes += NET_BUFFER_DATA_LENGTH(nb);
        }
        NdisReleaseSpinLock(&adapter->lock);

        if (sendFlags & NDIS_SEND_FLAGS_DISPATCH_LEVEL) {
            NdisMSendNetBufferListsComplete(adapter->miniportAdapterHandle, curr, NDIS_SEND_COMPLETE_FLAGS_DISPATCH_LEVEL);
        } else {
            NdisMSendNetBufferListsComplete(adapter->miniportAdapterHandle, curr, 0);
        }
        curr = next;
    }
}

_Use_decl_annotations_
VOID MiniportReturnNetBufferLists(
    NDIS_HANDLE miniportAdapterContext,
    PNET_BUFFER_LIST netBufferLists,
    ULONG returnFlags) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    PNET_BUFFER_LIST curr = netBufferLists;
    while (curr) {
        PNET_BUFFER_LIST next = NET_BUFFER_LIST_NEXT_NBL(curr);
        NdisAcquireSpinLock(&adapter->lock);
        adapter->rxPackets++;
        NdisReleaseSpinLock(&adapter->lock);
        NdisFreeNetBufferList(curr);
        curr = next;
    }
    UNREFERENCED_PARAMETER(returnFlags);
}

_Use_decl_annotations_
NDIS_STATUS MiniportOidRequest(
    NDIS_HANDLE miniportAdapterContext,
    PNDIS_OID_REQUEST oidRequest) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    NDIS_STATUS status = NDIS_STATUS_NOT_SUPPORTED;

    switch (oidRequest->DATA.QUERY_INFORMATION.Oid) {
    case OID_GEN_MAXIMUM_FRAME_SIZE:
        if (oidRequest->DATA.QUERY_INFORMATION.InformationBufferLength >= sizeof(ULONG)) {
            *(PULONG)oidRequest->DATA.QUERY_INFORMATION.InformationBuffer = adapter->mtu;
            oidRequest->DATA.QUERY_INFORMATION.BytesWritten = sizeof(ULONG);
            status = NDIS_STATUS_SUCCESS;
        }
        break;
    case OID_GEN_STATISTICS: {
        // Return minimal stats
        if (oidRequest->DATA.QUERY_INFORMATION.InformationBufferLength >= sizeof(NDIS_STATISTICS_INFO)) {
            PNDIS_STATISTICS_INFO stats = (PNDIS_STATISTICS_INFO)oidRequest->DATA.QUERY_INFORMATION.InformationBuffer;
            NdisZeroMemory(stats, sizeof(NDIS_STATISTICS_INFO));
            stats->Header.Type = NDIS_OBJECT_TYPE_DEFAULT;
            stats->Header.Revision = NDIS_STATISTICS_INFO_REVISION_1;
            stats->Header.Size = NDIS_SIZEOF_STATISTICS_INFO_REVISION_1;
            status = NDIS_STATUS_SUCCESS;
        }
        break;
    }
    default:
        status = NDIS_STATUS_NOT_SUPPORTED;
        break;
    }
    return status;
}

_Use_decl_annotations_
VOID MiniportShutdownEx(NDIS_HANDLE miniportAdapterContext, NDIS_SHUTDOWN_ACTION shutdownAction) {
    UNREFERENCED_PARAMETER(miniportAdapterContext);
    UNREFERENCED_PARAMETER(shutdownAction);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportShutdownEx\n"));
}

_Use_decl_annotations_
VOID MiniportDevicePnPEventNotify(
    NDIS_HANDLE miniportAdapterContext,
    PNET_DEVICE_PNP_EVENT netDevicePnPEvent) {
    UNREFERENCED_PARAMETER(miniportAdapterContext);
    UNREFERENCED_PARAMETER(netDevicePnPEvent);
    // Handle surprise removal, etc.
}

