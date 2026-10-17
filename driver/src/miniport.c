/*
 * Balancify NDIS 6.x virtual miniport - Phase 2 skeleton (Spike S2b).
 *
 * Goal of the skeleton: pass IP packets between the kernel adapter and the
 * user-mode service ring with the fewest possible transitions, while staying
 * HVCI-clean (no executable pageable memory, no unsafe MDL use).
 *
 * NOT BUILT ON LINUX CI. Requires WDK. See README.md.
 */

#include <ntddk.h>
#include <ndis.h>

#define BALANCIFY_TAG 'FclaB'

typedef struct _BALANCIFY_ADAPTER {
    NDIS_HANDLE          MiniportAdapterHandle;
    NDISEthernetAddress  CurrentAddress;
    BOOLEAN              Halted;
    /* Phase 2: shared-memory ring to the service (Wintun-style, first-party). */
    PVOID                SendRing;     /* TODO(S2b): allocate + map to service */
    PVOID                RecvRing;
} BALANCIFY_ADAPTER, *PBALANCIFY_ADAPTER;

/* Forward declarations (NDIS handler prototypes). */
NDIS_HANDLE g_NdisMiniportDriverHandle;
NDIS_STATUS MiniportInitializeEx(NDIS_HANDLE, NDIS_HANDLE, PNDIS_MINIPORT_INIT_PARAMETERS);
VOID MiniportHaltEx(NDIS_HANDLE, NDIS_HALT_ACTION);
VOID MiniportSendNetBufferLists(NDIS_HANDLE, PNET_BUFFER_LIST, NDIS_PORT_NUMBER, ULONG);
VOID MiniportReturnNetBufferLists(NDIS_HANDLE, PNET_BUFFER_LIST, ULONG);
NDIS_STATUS MiniportResetEx(NDIS_HANDLE, PNDIS_MINIPORT_RESET_PARAMETERS);

DRIVER_ENTRY DriverEntry;

NDIS_STATUS
DriverEntry(
    _In_ PDRIVER_OBJECT  DriverObject,
    _In_ PUNICODE_STRING RegistryPath
    )
{
    NDIS_MINIPORT_DRIVER_CHARACTERISTICS chars;
    NDIS_STATUS status;

    NdisZeroMemory(&chars, sizeof(chars));
    chars.Header.Type      = NDIS_OBJECT_TYPE_MINIPORT_DRIVER_CHARACTERISTICS;
    chars.Header.Revision  = NDIS_MINIPORT_DRIVER_CHARACTERISTICS_REVISION_3;
    chars.Header.Size      = NDIS_SIZEOF_MINIPORT_DRIVER_CHARACTERISTICS_REVISION_3;
    chars.MajorNdisVersion = NDIS_MINIPORT_MAJOR_VERSION;
    chars.MinorNdisVersion = NDIS_MINIPORT_MINOR_VERSION;
    chars.InitializeHandlerEx  = MiniportInitializeEx;
    chars.HaltHandlerEx        = MiniportHaltEx;
    chars.SendNetBufferListsHandler   = MiniportSendNetBufferLists;
    chars.ReturnNetBufferListsHandler = MiniportReturnNetBufferLists;
    chars.ResetHandlerEx       = MiniportResetEx;

    status = NdisMRegisterMiniportDriver(DriverObject, RegistryPath, NULL, &chars,
                                         /* out */ &g_NdisMiniportDriverHandle);
    return status;
}

/* TODO(S2b): implement handlers; keep each under 200 lines, validate all
 * lengths (rule 1), and route packets to/from the rings. */

NDIS_STATUS MiniportInitializeEx(NDIS_HANDLE h, NDIS_HANDLE ctx, PNDIS_MINIPORT_INIT_PARAMETERS p) { (void)h; (void)ctx; (void)p; return NDIS_STATUS_SUCCESS; }
VOID MiniportHaltEx(NDIS_HANDLE h, NDIS_HALT_ACTION a) { (void)h; (void)a; }
VOID MiniportSendNetBufferLists(NDIS_HANDLE h, PNET_BUFFER_LIST nbl, NDIS_PORT_NUMBER port, ULONG flags) { (void)h; (void)nbl; (void)port; (void)flags; }
VOID MiniportReturnNetBufferLists(NDIS_HANDLE h, PNET_BUFFER_LIST nbl, ULONG flags) { (void)h; (void)nbl; (void)flags; }
NDIS_STATUS MiniportResetEx(NDIS_HANDLE h, PNDIS_MINIPORT_RESET_PARAMETERS p) { (void)h; (void)p; return NDIS_STATUS_SUCCESS; }
