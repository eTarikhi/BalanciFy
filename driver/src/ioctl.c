// Balancify WDF control device + IOCTLs -- Phase 2 S2b
// Minimal control device: set MTU, inject NBL, dequeue TX packets, query stats
// Length-validated, fuzzed same gate as tunnel (no third-party kernel component in data path)
// CI build kit: WDK 10.0.26100.6584. Test signing and Driver Verifier are separate validation steps.
// References: blueprint Sec.6.1 TAP-like adapter, Sec.4.5 pause/resume, Sec.8.2 diagnostics, C2

#include "miniport.h"
#include "ip_packet_validation.h"

#define BALANCIFY_DEVICE_NAME L"\\Device\\Balancify"
#define BALANCIFY_DOS_NAME L"\\DosDevices\\Balancify"

// IOCTLs (METHOD_BUFFERED, explicit caller access rights, custom codes 0x800-0x8FF)
#define IOCTL_BALANCIFY_SET_MTU CTL_CODE(FILE_DEVICE_NETWORK, 0x800, METHOD_BUFFERED, FILE_WRITE_ACCESS)
#define IOCTL_BALANCIFY_GET_STATS CTL_CODE(FILE_DEVICE_NETWORK, 0x801, METHOD_BUFFERED, FILE_READ_ACCESS)
#define IOCTL_BALANCIFY_INJECT_NBL CTL_CODE(FILE_DEVICE_NETWORK, 0x802, METHOD_BUFFERED, FILE_WRITE_ACCESS)
#define IOCTL_BALANCIFY_SET_QUANTA CTL_CODE(FILE_DEVICE_NETWORK, 0x803, METHOD_BUFFERED, FILE_WRITE_ACCESS)
#define IOCTL_BALANCIFY_DEQUEUE_TX CTL_CODE(FILE_DEVICE_NETWORK, 0x804, METHOD_BUFFERED, FILE_READ_ACCESS)

// Input for SET_MTU: ULONG mtu
// Input for GET_STATS: none, output: BALANCIFY_STATS
// Input for INJECT_NBL: raw IP packet (max 1500) to inject as NBL into NDIS stack
// Input for SET_QUANTA: array of (u8 path, u64 quanta)

typedef struct _BALANCIFY_STATS {
    ULONG64 txPackets;
    ULONG64 rxPackets;
    ULONG64 txErrors;
    ULONG64 txBytes;
    ULONG64 rxBytes;
    ULONG mtu;
    BOOLEAN paused;
} BALANCIFY_STATS, *PBALANCIFY_STATS;

typedef struct _BALANCIFY_QUANTA {
    UCHAR path;
    ULONGLONG quanta;
} BALANCIFY_QUANTA, *PBALANCIFY_QUANTA;

// Control device context
typedef struct _BALANCIFY_CONTROL {
    PBALANCIFY_ADAPTER adapter; // associated miniport adapter (one for now)
} BALANCIFY_CONTROL, *PBALANCIFY_CONTROL;

WDF_DECLARE_CONTEXT_TYPE_WITH_NAME(BALANCIFY_CONTROL, BalancifyControlGetData)

EVT_WDF_DEVICE_FILE_CREATE BalancifyEvtDeviceFileCreate;
EVT_WDF_FILE_CLOSE BalancifyEvtFileClose;
EVT_WDF_IO_QUEUE_IO_DEVICE_CONTROL BalancifyEvtIoDeviceControl;

// Create control device in MiniportInitializeEx (called from miniport.c)
NTSTATUS BalancifyCreateControlDevice(WDFDRIVER driver, PBALANCIFY_ADAPTER adapter) {
    NTSTATUS status;
    WDFDEVICE device;
    PWDFDEVICE_INIT deviceInit;
    WDF_OBJECT_ATTRIBUTES deviceAttributes;
    WDF_IO_QUEUE_CONFIG queueConfig;
    WDF_FILEOBJECT_CONFIG fileConfig;
    UNICODE_STRING deviceName, dosName, deviceSddl;
    PBALANCIFY_CONTROL control;

    RtlInitUnicodeString(&deviceName, BALANCIFY_DEVICE_NAME);
    RtlInitUnicodeString(&dosName, BALANCIFY_DOS_NAME);
    // Only SYSTEM and Administrators may open the driver control device. The
    // unprivileged desktop shell must use the service's authenticated IPC.
    RtlInitUnicodeString(&deviceSddl, L"D:P(A;;GA;;;SY)(A;;GA;;;BA)");

    deviceInit = WdfControlDeviceInitAllocate(driver, &deviceSddl);
    if (!deviceInit) {
        return STATUS_INSUFFICIENT_RESOURCES;
    }

    WdfDeviceInitSetDeviceType(deviceInit, FILE_DEVICE_NETWORK);
    WdfDeviceInitSetCharacteristics(deviceInit, FILE_DEVICE_SECURE_OPEN, FALSE);
    status = WdfDeviceInitAssignName(deviceInit, &deviceName);
    if (!NT_SUCCESS(status)) {
        WdfDeviceInitFree(deviceInit);
        return status;
    }

    WDF_FILEOBJECT_CONFIG_INIT(&fileConfig, BalancifyEvtDeviceFileCreate, BalancifyEvtFileClose, WDF_NO_EVENT_CALLBACK);
    WdfDeviceInitSetFileObjectConfig(deviceInit, &fileConfig, WDF_NO_OBJECT_ATTRIBUTES);

    WDF_OBJECT_ATTRIBUTES_INIT_CONTEXT_TYPE(&deviceAttributes, BALANCIFY_CONTROL);
    status = WdfDeviceCreate(&deviceInit, &deviceAttributes, &device);
    if (!NT_SUCCESS(status)) {
        WdfDeviceInitFree(deviceInit);
        return status;
    }

    control = BalancifyControlGetData(device);
    control->adapter = adapter;

    WDF_IO_QUEUE_CONFIG_INIT_DEFAULT_QUEUE(&queueConfig, WdfIoQueueDispatchParallel);
    queueConfig.EvtIoDeviceControl = BalancifyEvtIoDeviceControl;

    status = WdfIoQueueCreate(device, &queueConfig, WDF_NO_OBJECT_ATTRIBUTES, WDF_NO_HANDLE);
    if (!NT_SUCCESS(status)) {
        WdfObjectDelete(device);
        return status;
    }

    status = WdfDeviceCreateSymbolicLink(device, &dosName);
    if (!NT_SUCCESS(status)) {
        WdfObjectDelete(device);
        return status;
    }

    WdfControlFinishInitializing(device);
    adapter->controlDevice = device;

    KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: Control device created %wZ\n", &deviceName));
    return STATUS_SUCCESS;
}

VOID BalancifyEvtDeviceFileCreate(WDFDEVICE device, WDFREQUEST request, WDFFILEOBJECT fileObject) {
    UNREFERENCED_PARAMETER(device);
    UNREFERENCED_PARAMETER(fileObject);
    WdfRequestComplete(request, STATUS_SUCCESS);
}

VOID BalancifyEvtFileClose(WDFFILEOBJECT fileObject) {
    UNREFERENCED_PARAMETER(fileObject);
}

VOID BalancifyEvtIoDeviceControl(
    WDFQUEUE queue,
    WDFREQUEST request,
    size_t outputBufferLength,
    size_t inputBufferLength,
    ULONG ioControlCode) {
    NTSTATUS status = STATUS_NOT_SUPPORTED;
    WDFDEVICE device = WdfIoQueueGetDevice(queue);
    PBALANCIFY_CONTROL control = BalancifyControlGetData(device);
    PBALANCIFY_ADAPTER adapter = control->adapter;
    size_t bytesReturned = 0;
    PVOID inputBuffer = NULL;
    PVOID outputBuffer = NULL;

    // Length validation: all IOCTLs use METHOD_BUFFERED, so buffers are in request
    switch (ioControlCode) {
    case IOCTL_BALANCIFY_SET_MTU: {
        if (inputBufferLength < sizeof(ULONG)) {
            status = STATUS_BUFFER_TOO_SMALL;
            break;
        }
        status = WdfRequestRetrieveInputBuffer(request, sizeof(ULONG), &inputBuffer, NULL);
        if (!NT_SUCCESS(status)) break;
        ULONG mtu = *(PULONG)inputBuffer;
        if (mtu < BALANCIFY_MIN_MTU || mtu > BALANCIFY_MAX_MTU) {
            status = STATUS_INVALID_PARAMETER;
            break;
        }
        // GeneralAttributes (including MtuSize) are registered during adapter
        // initialization and NDIS caches the maximum-frame-size value. Until a
        // supported restart/reconfiguration path exists, this IOCTL can confirm
        // the configured MTU but must not change only the driver's local view.
        BOOLEAN mtuMatches;
        NdisAcquireSpinLock(&adapter->lock);
        mtuMatches = (adapter->mtu == mtu) ? TRUE : FALSE;
        NdisReleaseSpinLock(&adapter->lock);
        if (!mtuMatches) {
            status = STATUS_NOT_SUPPORTED;
            break;
        }
        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: SET_MTU confirmed %lu\n", mtu));
        status = STATUS_SUCCESS;
        break;
    }
    case IOCTL_BALANCIFY_GET_STATS: {
        if (outputBufferLength < sizeof(BALANCIFY_STATS)) {
            status = STATUS_BUFFER_TOO_SMALL;
            break;
        }
        status = WdfRequestRetrieveOutputBuffer(request, sizeof(BALANCIFY_STATS), &outputBuffer, NULL);
        if (!NT_SUCCESS(status)) break;
        PBALANCIFY_STATS stats = (PBALANCIFY_STATS)outputBuffer;
        NdisAcquireSpinLock(&adapter->lock);
        stats->txPackets = adapter->txPackets;
        stats->rxPackets = adapter->rxPackets;
        stats->txErrors = adapter->txErrors;
        stats->txBytes = adapter->txBytes;
        stats->rxBytes = adapter->rxBytes;
        stats->mtu = adapter->mtu;
        stats->paused = (BOOLEAN)adapter->pauseState.paused;
        NdisReleaseSpinLock(&adapter->lock);
        bytesReturned = sizeof(BALANCIFY_STATS);
        status = STATUS_SUCCESS;
        break;
    }
    case IOCTL_BALANCIFY_INJECT_NBL: {
        ULONG mtu;
        BOOLEAN paused;
        BOOLEAN canIndicate;

        // The public miniport MTU is the maximum raw IP packet length. Snapshot
        // it under the same lock used by SET_MTU and the miniport OID callbacks.
        NdisAcquireSpinLock(&adapter->lock);
        mtu = adapter->mtu;
        paused = (BOOLEAN)adapter->pauseState.paused;
        NdisReleaseSpinLock(&adapter->lock);
        if (paused) {
            status = STATUS_DEVICE_NOT_READY;
            break;
        }
        if (inputBufferLength < 20 || inputBufferLength > mtu || inputBufferLength > BALANCIFY_MAX_MTU) {
            status = STATUS_INVALID_PARAMETER;
            break;
        }
        status = WdfRequestRetrieveInputBuffer(request, inputBufferLength, &inputBuffer, NULL);
        if (!NT_SUCCESS(status)) break;
        if (!BalancifyIsValidIpPacket((const unsigned char *)inputBuffer, inputBufferLength, mtu)) {
            status = STATUS_INVALID_PARAMETER;
            break;
        }

        // Copy out of the METHOD_BUFFERED request before completing it. NDIS may
        // retain the indicated NBL after this IOCTL returns.
        PVOID packetCopy = NdisAllocateMemoryWithTagPriority(
            adapter->miniportAdapterHandle,
            (ULONG)inputBufferLength,
            BALANCIFY_POOL_TAG,
            NormalPoolPriority);
        if (!packetCopy) {
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }
        NdisMoveMemory(packetCopy, inputBuffer, inputBufferLength);

        PMDL mdl = NdisAllocateMdl(
            adapter->miniportAdapterHandle,
            packetCopy,
            (ULONG)inputBufferLength);
        if (!mdl) {
            NdisFreeMemory(packetCopy, (ULONG)inputBufferLength, 0);
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }

        // Allocate the NBL/NB from the miniport pool, which reserves a private
        // context slot for packetCopy until MiniportReturnNetBufferLists.
        PNET_BUFFER_LIST nbl = NULL;
        if (adapter->netBufferListPool) {
            nbl = NdisAllocateNetBufferAndNetBufferList(
                adapter->netBufferListPool,
                0, 0,
                mdl,
                0,
                (ULONG)inputBufferLength);
        }
        if (!nbl) {
            NdisFreeMdl(mdl);
            NdisFreeMemory(packetCopy, (ULONG)inputBufferLength, 0);
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }

        PBALANCIFY_RX_NBL_CONTEXT nblContext =
            (PBALANCIFY_RX_NBL_CONTEXT)NET_BUFFER_LIST_CONTEXT_DATA_START(nbl);
        if (!nblContext) {
            NdisFreeNetBufferList(nbl);
            NdisFreeMdl(mdl);
            NdisFreeMemory(packetCopy, (ULONG)inputBufferLength, 0);
            status = STATUS_INSUFFICIENT_RESOURCES;
            break;
        }
        nblContext->packetBuffer = packetCopy;
        nblContext->packetLength = (ULONG)inputBufferLength;

        NdisAcquireSpinLock(&adapter->lock);
        paused = (BOOLEAN)adapter->pauseState.paused;
        canIndicate = BalancifyTrackReceiveIndication(&adapter->pauseState) ? TRUE : FALSE;
        if (canIndicate) {
            adapter->rxPackets++;
            adapter->rxBytes += inputBufferLength;
        }
        NdisReleaseSpinLock(&adapter->lock);

        if (!canIndicate) {
            NdisFreeNetBufferList(nbl);
            NdisFreeMdl(mdl);
            NdisFreeMemory(packetCopy, (ULONG)inputBufferLength, 0);
            status = paused ? STATUS_DEVICE_NOT_READY : STATUS_INSUFFICIENT_RESOURCES;
            break;
        }

        // The WDF callback may run below DISPATCH_LEVEL. Set the NDIS flag only
        // when it is actually executing at DISPATCH_LEVEL.
        ULONG receiveFlags = (KeGetCurrentIrql() == DISPATCH_LEVEL)
            ? NDIS_RECEIVE_FLAGS_DISPATCH_LEVEL
            : 0;
        NdisMIndicateReceiveNetBufferLists(
            adapter->miniportAdapterHandle,
            nbl,
            NDIS_DEFAULT_PORT_NUMBER,
            1,
            receiveFlags);

        BOOLEAN completePause;
        NdisAcquireSpinLock(&adapter->lock);
        completePause = BalancifyReceiveIndicationReturned(&adapter->pauseState) ? TRUE : FALSE;
        NdisReleaseSpinLock(&adapter->lock);
        if (completePause) NdisMPauseComplete(adapter->miniportAdapterHandle);

        KdPrintEx((DPFLTR_IHVDRIVER_ID, DPFLTR_INFO_LEVEL, "Balancify: INJECT_NBL %zu bytes indicated\n", inputBufferLength));
        status = STATUS_SUCCESS;
        break;
    }
    case IOCTL_BALANCIFY_DEQUEUE_TX: {
        const size_t minimumOutputLength = sizeof(BALANCIFY_TX_PACKET_HEADER) + BALANCIFY_TX_PACKET_MAX_BYTES;
        const BALANCIFY_TX_PACKET_SLOT *slot;
        BALANCIFY_TX_PACKET_HEADER header;
        unsigned char *destination;

        if (inputBufferLength != 0) {
            status = STATUS_INVALID_PARAMETER;
            break;
        }
        // Require the fixed maximum buffer so a queued packet is never popped
        // and then lost because a caller supplied a short output buffer.
        if (outputBufferLength < minimumOutputLength) {
            status = STATUS_BUFFER_TOO_SMALL;
            break;
        }
        status = WdfRequestRetrieveOutputBuffer(
            request, minimumOutputLength, &outputBuffer, NULL);
        if (!NT_SUCCESS(status)) break;

        NdisAcquireSpinLock(&adapter->lock);
        slot = BalancifyTxQueuePeek(&adapter->txQueue);
        if (!slot) {
            NdisReleaseSpinLock(&adapter->lock);
            // Empty queue is a successful poll with no output bytes.
            status = STATUS_SUCCESS;
            break;
        }
        header.version = BALANCIFY_TX_PACKET_VERSION;
        header.packetLength = (unsigned int)slot->length;
        header.sequence = slot->sequence;
        destination = (unsigned char *)outputBuffer;
        NdisMoveMemory(destination, &header, sizeof(header));
        NdisMoveMemory(destination + sizeof(header), slot->bytes, slot->length);
        bytesReturned = sizeof(header) + slot->length;
        (void)BalancifyTxQueuePop(&adapter->txQueue);
        NdisReleaseSpinLock(&adapter->lock);
        status = STATUS_SUCCESS;
        break;
    }
    case IOCTL_BALANCIFY_SET_QUANTA: {
        // Set DRR quanta from service (Sec.6.4 rule 2)
        // Input: array of BALANCIFY_QUANTA, count = inputBufferLength / sizeof(BALANCIFY_QUANTA)
        if (inputBufferLength == 0 || inputBufferLength % sizeof(BALANCIFY_QUANTA) != 0) {
            status = STATUS_INVALID_PARAMETER;
            break;
        }
        if (inputBufferLength > 256 * sizeof(BALANCIFY_QUANTA)) {
            status = STATUS_INVALID_PARAMETER; // max 256 paths
            break;
        }
        // No driver scheduler consumes these weights yet. Returning success
        // would falsely claim that the requested policy was applied.
        status = STATUS_NOT_SUPPORTED;
        break;
    }
    default:
        status = STATUS_NOT_SUPPORTED;
        break;
    }

    WdfRequestCompleteWithInformation(request, status, bytesReturned);
}
