// Balancify NDIS virtual miniport -- Phase 2 S2b skeleton
// This file documents the intended driver layout and compiles on WDK.
// It is not yet wired to the service data path; S2b requires test-signed
// load with Driver Verifier and packet loopback through it.
//
// Build: msbuild driver/balancify.vcxproj /p:Configuration=Release /p:Platform=x64
// Requires WDK 10.0.22621+, SDK 10.0.22621+.
//
// References:
// - NDIS 6.8 miniport driver model (Windows Driver Kit docs)
// - blueprint Sec.6.1 TAP-like virtual adapter, Sec.4.5 pause/resume, Sec.6.6 NAT keepalive
// - No third-party kernel component in data path (C2): only NDIS + WDF

#include "miniport.h"

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

static const ULONG gSupportedPacketFilters =
    NDIS_PACKET_TYPE_DIRECTED | NDIS_PACKET_TYPE_ALL_MULTICAST |
    NDIS_PACKET_TYPE_BROADCAST | NDIS_PACKET_TYPE_ALL_LOCAL |
    NDIS_PACKET_TYPE_ALL_FUNCTIONAL;

static const NDIS_OID gSupportedOids[] = {
    OID_GEN_SUPPORTED_LIST,
    OID_GEN_MEDIA_SUPPORTED,
    OID_GEN_MEDIA_IN_USE,
    OID_GEN_MAXIMUM_FRAME_SIZE,
    OID_GEN_MAXIMUM_TOTAL_SIZE,
    OID_GEN_CURRENT_LOOKAHEAD,
    OID_GEN_CURRENT_PACKET_FILTER,
    OID_GEN_XMIT_OK,
    OID_GEN_RCV_OK,
    OID_GEN_STATISTICS,
};

static ULONG MiniportReadConfiguredMtu(NDIS_HANDLE miniportAdapterHandle) {
    NDIS_CONFIGURATION_OBJECT configurationObject;
    NDIS_HANDLE configurationHandle = NULL;
    PNDIS_CONFIGURATION_PARAMETER parameter = NULL;
    NDIS_STRING mtuKeyword = NDIS_STRING_CONST("Mtu");
    NDIS_STATUS status;
    ULONG mtu = BALANCIFY_DEFAULT_MTU;

    NdisZeroMemory(&configurationObject, sizeof(configurationObject));
    configurationObject.Header.Type = NDIS_OBJECT_TYPE_CONFIGURATION_OBJECT;
    configurationObject.Header.Revision = NDIS_CONFIGURATION_OBJECT_REVISION_1;
    configurationObject.Header.Size = NDIS_SIZEOF_CONFIGURATION_OBJECT_REVISION_1;
    configurationObject.NdisHandle = miniportAdapterHandle;
    configurationObject.Flags = 0;

    status = NdisOpenConfigurationEx(&configurationObject, &configurationHandle);
    if (status != NDIS_STATUS_SUCCESS) return mtu;

    NdisReadConfiguration(&status, &parameter, configurationHandle,
        &mtuKeyword, NdisParameterInteger);
    if (status == NDIS_STATUS_SUCCESS && parameter &&
        parameter->ParameterType == NdisParameterInteger) {
        mtu = BalancifyNormalizeMtu(parameter->ParameterData.IntegerData, mtu);
    }
    NdisCloseConfiguration(configurationHandle);
    return mtu;
}

static VOID MiniportFreeAdapter(PBALANCIFY_ADAPTER adapter) {
    if (!adapter) return;
    if (adapter->controlDevice) {
        WDFQUEUE queue = WdfDeviceGetDefaultQueue(adapter->controlDevice);
        // Halt and failed initialization run at PASSIVE_LEVEL. Stop dispatch and
        // wait for any active control request before releasing its adapter.
        if (queue) WdfIoQueuePurgeSynchronously(queue);
        WdfObjectDelete(adapter->controlDevice);
        adapter->controlDevice = NULL;
    }
    if (adapter->netBufferListPool) {
        NdisFreeNetBufferListPool(adapter->netBufferListPool);
        adapter->netBufferListPool = NULL;
    }
    NdisFreeSpinLock(&adapter->lock);
    NdisFreeMemory(adapter, sizeof(BALANCIFY_ADAPTER), 0);
}

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
    WDFDRIVER wdfDriver;
    UNREFERENCED_PARAMETER(RegistryPath);

    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: DriverEntry\n"));

    WDF_DRIVER_CONFIG_INIT(&wdfConfig, WDF_NO_EVENT_CALLBACK);
    wdfConfig.DriverInitFlags |= WdfDriverInitNonPnpDriver;
    wdfConfig.EvtDriverUnload = DriverUnload;

    status = WdfDriverCreate(DriverObject, RegistryPath, WDF_NO_OBJECT_ATTRIBUTES, &wdfConfig, &wdfDriver);
    if (!NT_SUCCESS(status)) {
        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL, "Balancify: WdfDriverCreate failed 0x%x\n", status));
        return status;
    }

    // Now register NDIS miniport
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
    PBALANCIFY_ADAPTER adapter;
    NDIS_STATUS status;
    NDIS_MINIPORT_ADAPTER_REGISTRATION_ATTRIBUTES registration;
    NDIS_MINIPORT_ADAPTER_GENERAL_ATTRIBUTES general;
    ULONG linkSpeed = 1000000000UL;

    UNREFERENCED_PARAMETER(miniportDriverContext);
    UNREFERENCED_PARAMETER(miniportInitParameters);

    adapter = (PBALANCIFY_ADAPTER)NdisAllocateMemoryWithTagPriority(
        miniportAdapterHandle, sizeof(BALANCIFY_ADAPTER), BALANCIFY_POOL_TAG, NormalPoolPriority);
    if (!adapter) return NDIS_STATUS_RESOURCES;

    NdisZeroMemory(adapter, sizeof(BALANCIFY_ADAPTER));
    adapter->miniportAdapterHandle = miniportAdapterHandle;
    adapter->mtu = BALANCIFY_DEFAULT_MTU;
    adapter->lookaheadSize = adapter->mtu;
    // NDIS requires the receive filter to start disabled; protocols enable it
    // through OID_GEN_CURRENT_PACKET_FILTER after adapter initialization.
    adapter->packetFilter = 0;
    NdisAllocateSpinLock(&adapter->lock);

    // NDIS requires the registration attributes before general attributes.
    // Publish the adapter context first so callbacks have a valid context once
    // registration succeeds. Any subsequent initialization failure is unwound
    // here because NDIS does not call Halt for a failed initialization.
    NdisZeroMemory(&registration, sizeof(registration));
    registration.Header.Type = NDIS_OBJECT_TYPE_MINIPORT_ADAPTER_REGISTRATION_ATTRIBUTES;
    registration.Header.Revision = NDIS_MINIPORT_ADAPTER_REGISTRATION_ATTRIBUTES_REVISION_1;
    registration.Header.Size = NDIS_SIZEOF_MINIPORT_ADAPTER_REGISTRATION_ATTRIBUTES_REVISION_1;
    registration.MiniportAdapterContext = adapter;
    registration.AttributeFlags = NDIS_MINIPORT_ATTRIBUTES_SURPRISE_REMOVE_OK;
    registration.CheckForHangTimeInSeconds = 0;
    registration.InterfaceType = NdisInterfaceInternal;

    status = NdisMSetMiniportAttributes(
        miniportAdapterHandle,
        (PNDIS_MINIPORT_ADAPTER_ATTRIBUTES)&registration);
    if (status != NDIS_STATUS_SUCCESS) {
        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL,
            "Balancify: registration attributes failed 0x%x\n", status));
        MiniportFreeAdapter(adapter);
        return status;
    }

    // Read adapter parameters only after registration attributes have been set.
    adapter->mtu = MiniportReadConfiguredMtu(miniportAdapterHandle);
    adapter->lookaheadSize = adapter->mtu;

    // Create the receive NBL pool used by the controlled INJECT_NBL path.
    {
        NET_BUFFER_LIST_POOL_PARAMETERS nblParams;
        NdisZeroMemory(&nblParams, sizeof(nblParams));
        nblParams.Header.Type = NDIS_OBJECT_TYPE_DEFAULT;
        nblParams.Header.Revision = NET_BUFFER_LIST_POOL_PARAMETERS_REVISION_1;
        nblParams.Header.Size = NDIS_SIZEOF_NET_BUFFER_LIST_POOL_PARAMETERS_REVISION_1;
        nblParams.ProtocolId = NDIS_PROTOCOL_ID_DEFAULT;
        nblParams.fAllocateNetBuffer = TRUE;
        nblParams.ContextSize = sizeof(BALANCIFY_RX_NBL_CONTEXT);
        nblParams.PoolTag = BALANCIFY_POOL_TAG;
        nblParams.DataSize = 0;
        adapter->netBufferListPool = NdisAllocateNetBufferListPool(miniportAdapterHandle, &nblParams);
        if (!adapter->netBufferListPool) {
            KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL,
                "Balancify: NdisAllocateNetBufferListPool failed\n"));
            status = NDIS_STATUS_RESOURCES;
            goto InitializeFailure;
        }
    }

    // Create the privileged WDF control device before exposing the miniport.
    {
        NTSTATUS wdfStatus = BalancifyCreateControlDevice(WdfGetDriver(), adapter);
        if (!NT_SUCCESS(wdfStatus)) {
            KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL,
                "Balancify: Control device creation failed 0x%x\n", wdfStatus));
            status = NDIS_STATUS_FAILURE;
            goto InitializeFailure;
        }
    }

    NdisZeroMemory(&general, sizeof(general));
    general.Header.Type = NDIS_OBJECT_TYPE_MINIPORT_ADAPTER_GENERAL_ATTRIBUTES;
    general.Header.Revision = NDIS_MINIPORT_ADAPTER_GENERAL_ATTRIBUTES_REVISION_2;
    general.Header.Size = NDIS_SIZEOF_MINIPORT_ADAPTER_GENERAL_ATTRIBUTES_REVISION_2;
    // NdisMediumIP makes the interface contract match the raw IP packets
    // accepted by INJECT_NBL; unlike Ethernet, these NBLs have no L2 header.
    general.MediaType = NdisMediumIP;
    general.PhysicalMediumType = NdisPhysicalMediumUnspecified;
    general.MtuSize = adapter->mtu;
    general.MaxXmitLinkSpeed = (ULONG64)linkSpeed;
    general.XmitLinkSpeed = (ULONG64)linkSpeed;
    general.MaxRcvLinkSpeed = (ULONG64)linkSpeed;
    general.RcvLinkSpeed = (ULONG64)linkSpeed;
    general.MediaConnectState = MediaConnectStateConnected;
    general.MediaDuplexState = MediaDuplexStateFull;
    general.LookaheadSize = adapter->lookaheadSize;
    general.MacOptions = NDIS_MAC_OPTION_COPY_LOOKAHEAD_DATA |
        NDIS_MAC_OPTION_TRANSFERS_NOT_PEND | NDIS_MAC_OPTION_NO_LOOPBACK;
    general.SupportedPacketFilters = gSupportedPacketFilters;
    general.MaxMulticastListSize = 0;
    general.MacAddressLength = 0;
    general.AccessType = NET_IF_ACCESS_BROADCAST;
    general.DirectionType = NET_IF_DIRECTION_SENDRECEIVE;
    general.ConnectionType = NET_IF_CONNECTION_DEDICATED;
    // Keep the NDIS interface type aligned with *IfType in the INF.
    general.IfType = IF_TYPE_PROP_VIRTUAL;
    general.IfConnectorPresent = FALSE;
    general.SupportedStatistics = NDIS_STATISTICS_FLAGS_VALID_BYTES_RCV |
        NDIS_STATISTICS_FLAGS_VALID_BYTES_XMIT;
    general.SupportedPauseFunctions = NdisPauseFunctionsUnsupported;
    general.DataBackFillSize = 0;
    general.ContextBackFillSize = 0;
    general.SupportedOidList = (PNDIS_OID)gSupportedOids;
    general.SupportedOidListLength = sizeof(gSupportedOids);

    status = NdisMSetMiniportAttributes(
        miniportAdapterHandle,
        (PNDIS_MINIPORT_ADAPTER_ATTRIBUTES)&general);
    if (status != NDIS_STATUS_SUCCESS) {
        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_ERROR_LEVEL,
            "Balancify: general attributes failed 0x%x\n", status));
        goto InitializeFailure;
    }

    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL,
        "Balancify: MiniportInitializeEx mtu=%lu\n", adapter->mtu));
    return NDIS_STATUS_SUCCESS;

InitializeFailure:
    MiniportFreeAdapter(adapter);
    return status;
}

_Use_decl_annotations_
VOID MiniportHaltEx(NDIS_HANDLE miniportAdapterContext, NDIS_HALT_ACTION haltAction) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    UNREFERENCED_PARAMETER(haltAction);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportHaltEx\n"));
    MiniportFreeAdapter(adapter);
}

_Use_decl_annotations_
NDIS_STATUS MiniportPause(NDIS_HANDLE miniportAdapterContext, PNDIS_MINIPORT_PAUSE_PARAMETERS pauseParameters) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;
    UNREFERENCED_PARAMETER(pauseParameters);
    NdisAcquireSpinLock(&adapter->lock);
    adapter->paused = TRUE;
    NdisReleaseSpinLock(&adapter->lock);
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportPause (Sec.4.5 sleep)\n"));
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
    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: MiniportRestart (Sec.4.5 resume)\n"));
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
        // Each NET_BUFFER is one packet; an NBL may carry multiple NBs.
        for (PNET_BUFFER nb = NET_BUFFER_LIST_FIRST_NB(curr); nb; nb = NET_BUFFER_NEXT_NB(nb)) {
            adapter->txPackets++;
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
        // These NBLs are the private receive allocations created by INJECT_NBL.
        // Keep the packet copy and MDL alive until NDIS returns the NBL.
        PNET_BUFFER nb = NET_BUFFER_LIST_FIRST_NB(curr);
        if (nb) {
            PMDL mdl = NET_BUFFER_FIRST_MDL(nb);
            PMDL nextMdl = NULL;
            for (PMDL m = mdl; m; m = nextMdl) {
                nextMdl = m->Next;
                NdisFreeMdl(m);
            }
        }
        PBALANCIFY_RX_NBL_CONTEXT context =
            (PBALANCIFY_RX_NBL_CONTEXT)NET_BUFFER_LIST_CONTEXT_DATA_START(curr);
        if (context && context->packetBuffer) {
            NdisFreeMemory(context->packetBuffer, context->packetLength, 0);
            context->packetBuffer = NULL;
            context->packetLength = 0;
        }
        NdisFreeNetBufferList(curr);
        curr = next;
    }
    UNREFERENCED_PARAMETER(adapter);
    UNREFERENCED_PARAMETER(returnFlags);
}

_Use_decl_annotations_
NDIS_STATUS MiniportOidRequest(
    NDIS_HANDLE miniportAdapterContext,
    PNDIS_OID_REQUEST oidRequest) {
    PBALANCIFY_ADAPTER adapter = (PBALANCIFY_ADAPTER)miniportAdapterContext;

    if (oidRequest->RequestType == NdisRequestSetInformation) {
        NDIS_OID oid = oidRequest->DATA.SET_INFORMATION.Oid;
        PVOID buffer = oidRequest->DATA.SET_INFORMATION.InformationBuffer;
        ULONG bufferLength = oidRequest->DATA.SET_INFORMATION.InformationBufferLength;
        oidRequest->DATA.SET_INFORMATION.BytesRead = 0;
        oidRequest->DATA.SET_INFORMATION.BytesNeeded = 0;

        if (oid == OID_GEN_CURRENT_PACKET_FILTER) {
            ULONG packetFilter;
            if (bufferLength < sizeof(ULONG)) {
                oidRequest->DATA.SET_INFORMATION.BytesNeeded = sizeof(ULONG);
                return NDIS_STATUS_BUFFER_TOO_SHORT;
            }
            packetFilter = *(PULONG)buffer;
            if (packetFilter & ~gSupportedPacketFilters) return NDIS_STATUS_NOT_SUPPORTED;
            NdisAcquireSpinLock(&adapter->lock);
            adapter->packetFilter = packetFilter;
            NdisReleaseSpinLock(&adapter->lock);
            oidRequest->DATA.SET_INFORMATION.BytesRead = sizeof(ULONG);
            return NDIS_STATUS_SUCCESS;
        }
        if (oid == OID_GEN_CURRENT_LOOKAHEAD) {
            ULONG lookahead;
            ULONG mtu;
            if (bufferLength != sizeof(ULONG)) {
                oidRequest->DATA.SET_INFORMATION.BytesNeeded = sizeof(ULONG);
                return NDIS_STATUS_INVALID_LENGTH;
            }
            lookahead = *(PULONG)buffer;
            NdisAcquireSpinLock(&adapter->lock);
            mtu = adapter->mtu;
            if (lookahead <= mtu) adapter->lookaheadSize = lookahead;
            NdisReleaseSpinLock(&adapter->lock);
            if (lookahead > mtu) return NDIS_STATUS_INVALID_DATA;
            oidRequest->DATA.SET_INFORMATION.BytesRead = sizeof(ULONG);
            return NDIS_STATUS_SUCCESS;
        }
        return NDIS_STATUS_NOT_SUPPORTED;
    }

    if (oidRequest->RequestType != NdisRequestQueryInformation &&
        oidRequest->RequestType != NdisRequestQueryStatistics) {
        return NDIS_STATUS_NOT_SUPPORTED;
    }

    {
        NDIS_OID oid = oidRequest->DATA.QUERY_INFORMATION.Oid;
        PVOID buffer = oidRequest->DATA.QUERY_INFORMATION.InformationBuffer;
        ULONG bufferLength = oidRequest->DATA.QUERY_INFORMATION.InformationBufferLength;
        ULONG bytesWritten = 0;
        ULONG bytesNeeded = 0;
        ULONG mtu;
        ULONG lookaheadSize;
        ULONG packetFilter;
        ULONG64 rxBytes;
        ULONG64 txBytes;
        ULONG64 rxPackets;
        ULONG64 txPackets;

        oidRequest->DATA.QUERY_INFORMATION.BytesWritten = 0;
        oidRequest->DATA.QUERY_INFORMATION.BytesNeeded = 0;
        NdisAcquireSpinLock(&adapter->lock);
        mtu = adapter->mtu;
        lookaheadSize = adapter->lookaheadSize;
        packetFilter = adapter->packetFilter;
        rxBytes = adapter->rxBytes;
        txBytes = adapter->txBytes;
        rxPackets = adapter->rxPackets;
        txPackets = adapter->txPackets;
        NdisReleaseSpinLock(&adapter->lock);

        switch (oid) {
        case OID_GEN_SUPPORTED_LIST:
            bytesNeeded = sizeof(gSupportedOids);
            if (bufferLength >= bytesNeeded) {
                NdisMoveMemory(buffer, gSupportedOids, bytesNeeded);
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_MEDIA_SUPPORTED:
        case OID_GEN_MEDIA_IN_USE:
            bytesNeeded = sizeof(NDIS_MEDIUM);
            if (bufferLength >= bytesNeeded) {
                *(PNDIS_MEDIUM)buffer = NdisMediumIP;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_MAXIMUM_FRAME_SIZE:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = mtu;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_MAXIMUM_TOTAL_SIZE:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = mtu;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_CURRENT_LOOKAHEAD:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = lookaheadSize;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_XMIT_OK:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = (ULONG)txPackets;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_RCV_OK:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = (ULONG)rxPackets;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_CURRENT_PACKET_FILTER:
            bytesNeeded = sizeof(ULONG);
            if (bufferLength >= bytesNeeded) {
                *(PULONG)buffer = packetFilter;
                bytesWritten = bytesNeeded;
            }
            break;
        case OID_GEN_STATISTICS: {
            PNDIS_STATISTICS_INFO stats;
            bytesNeeded = sizeof(NDIS_STATISTICS_INFO);
            if (bufferLength < bytesNeeded) break;
            stats = (PNDIS_STATISTICS_INFO)buffer;
            NdisZeroMemory(stats, sizeof(*stats));
            stats->Header.Type = NDIS_OBJECT_TYPE_DEFAULT;
            stats->Header.Revision = NDIS_STATISTICS_INFO_REVISION_1;
            stats->Header.Size = NDIS_SIZEOF_STATISTICS_INFO_REVISION_1;
            stats->SupportedStatistics = NDIS_STATISTICS_FLAGS_VALID_BYTES_RCV |
                NDIS_STATISTICS_FLAGS_VALID_BYTES_XMIT;
            stats->ifHCInOctets = rxBytes;
            stats->ifHCOutOctets = txBytes;
            bytesWritten = sizeof(*stats);
            break;
        }
        default:
            return NDIS_STATUS_NOT_SUPPORTED;
        }

        if (bytesWritten != 0) {
            oidRequest->DATA.QUERY_INFORMATION.BytesWritten = bytesWritten;
            return NDIS_STATUS_SUCCESS;
        }
        oidRequest->DATA.QUERY_INFORMATION.BytesNeeded = bytesNeeded;
        return NDIS_STATUS_BUFFER_TOO_SHORT;
    }
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

