import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MINIPORT = (ROOT / "driver/src/miniport.c").read_text(encoding="utf-8")
MINIPORT_HEADER = (ROOT / "driver/src/miniport.h").read_text(encoding="utf-8")
IOCTL = (ROOT / "driver/src/ioctl.c").read_text(encoding="utf-8")


class DriverControlLifecycleTests(unittest.TestCase):
    def test_control_requests_are_gated_until_general_attributes_succeed(self):
        initialize = MINIPORT.split("NDIS_STATUS MiniportInitializeEx(", 1)[1].split(
            "VOID MiniportHaltEx(", 1
        )[0]
        control_device_creation = initialize.index("BalancifyCreateControlDevice(")
        general_attributes = initialize.rindex("status = NdisMSetMiniportAttributes(")
        publish_ready = initialize.index("adapter->initialized = TRUE", general_attributes)
        initialize_success = initialize.index("return NDIS_STATUS_SUCCESS;", publish_ready)

        self.assertIn("BOOLEAN initialized;", MINIPORT_HEADER)
        self.assertLess(control_device_creation, general_attributes)
        self.assertLess(general_attributes, publish_ready)
        self.assertLess(publish_ready, initialize_success)
        self.assertIn("if (status != NDIS_STATUS_SUCCESS)", initialize[general_attributes:publish_ready])
        self.assertIn("goto InitializeFailure;", initialize[general_attributes:publish_ready])

        callback = IOCTL.split("VOID BalancifyEvtIoDeviceControl(", 1)[1].split(
            "WdfRequestCompleteWithInformation(request, status, bytesReturned);", 1
        )[0]
        gate = callback.index("if (!adapter->initialized)")
        dispatch = callback.index("switch (ioControlCode)")
        self.assertLess(gate, dispatch)
        self.assertIn("NdisAcquireSpinLock(&adapter->lock);", callback[:gate])
        self.assertIn("NdisReleaseSpinLock(&adapter->lock);", callback[gate:dispatch])
        self.assertIn("STATUS_DEVICE_NOT_READY", callback[gate:dispatch])

    def test_adapter_is_marked_unavailable_before_control_queue_purge(self):
        free_adapter = MINIPORT.split("static VOID MiniportFreeAdapter(", 1)[1].split(
            "// Miniport characteristics", 1
        )[0]
        mark_unavailable = free_adapter.index("adapter->initialized = FALSE")
        purge_queue = free_adapter.index("WdfIoQueuePurgeSynchronously(queue)")
        self.assertLess(mark_unavailable, purge_queue)
        self.assertIn("NdisAcquireSpinLock(&adapter->lock);", free_adapter[:mark_unavailable])
        self.assertIn("NdisReleaseSpinLock(&adapter->lock);", free_adapter[mark_unavailable:purge_queue])

    def test_untracked_send_callback_cannot_publish_packets(self):
        send = MINIPORT.split("VOID MiniportSendNetBufferLists(", 1)[1].split(
            "VOID MiniportReturnNetBufferLists(", 1
        )[0]
        queue_send = send.index("MiniportQueueNetBufferList(adapter, curr)")
        guard = send.rfind("if (tracked)", 0, queue_send)
        rejected_status = send.index("NET_BUFFER_LIST_STATUS(curr) = NDIS_STATUS_RESOURCES", guard)
        self.assertGreaterEqual(guard, 0)
        self.assertLess(guard, queue_send)
        self.assertLess(rejected_status, send.index("NdisMSendNetBufferListsComplete", guard))

    def test_receive_injection_honors_disabled_packet_filter(self):
        inject = IOCTL.split("case IOCTL_BALANCIFY_INJECT_NBL:", 1)[1].split(
            "case IOCTL_BALANCIFY_DEQUEUE_TX:", 1
        )[0]
        self.assertIn("packetFilter = adapter->packetFilter;", inject)
        self.assertIn("if (paused || packetFilter == 0)", inject)
        self.assertLess(
            inject.index("if (paused || packetFilter == 0)"),
            inject.index("NdisAllocateMemoryWithTagPriority("),
        )
        self.assertIn(
            "packetFilter != 0 &&\n            BalancifyTrackReceiveIndication(&adapter->pauseState)",
            inject,
        )
        self.assertIn("(paused || packetFilter == 0)", inject)


if __name__ == "__main__":
    unittest.main()
