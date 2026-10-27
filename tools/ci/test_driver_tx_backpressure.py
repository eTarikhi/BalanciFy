import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SERVICE_BINARY = (ROOT / "crates/service/src/bin/balancify-service.rs").read_text(encoding="utf-8")
DRIVER_DEVICE = (ROOT / "crates/service/src/driver_device.rs").read_text(encoding="utf-8")


class DriverTxBackpressureTests(unittest.TestCase):
    def test_dequeue_retry_policy_covers_unscheduled_outcomes(self):
        self.assertIn("pub fn should_retry_driver_tx(actions: &[super::Action]) -> bool", DRIVER_DEVICE)
        self.assertIn("!actions.iter().any(|action| matches!(action, super::Action::Send { .. }))", DRIVER_DEVICE)
        self.assertIn("driver_tx_retries_when_the_service_did_not_schedule_a_send", DRIVER_DEVICE)
        for outcome in ("Action::Hold", "Action::Refused", "Action::Send"):
            with self.subTest(outcome=outcome):
                self.assertIn(outcome, DRIVER_DEVICE)

    def test_service_retries_pending_packet_before_dequeuing_more(self):
        self.assertIn("let mut pending_driver_tx: Option<Vec<u8>> = None;", SERVICE_BINARY)
        polling = SERVICE_BINARY.split("// Poll a bounded batch from the miniport's driver-owned TX queue", 1)[1]
        polling = polling.split("// Optional synthetic raw-IP traffic", 1)[0]
        self.assertLess(polling.index("pending_driver_tx.take()"), polling.index("device.dequeue_tx()"))
        self.assertIn("if should_retry_driver_tx(&actions)", polling)
        self.assertIn("pending_driver_tx = Some(packet);", polling)
        self.assertIn("if pending_driver_tx.is_none()", polling)
        self.assertIn("let mut budget = 64;", polling)

    def test_not_ready_driver_receive_is_queued_in_order_and_backpressures_reads(self):
        self.assertIn("is_driver_not_ready", DRIVER_DEVICE)
        self.assertIn("const MAX_PENDING_DRIVER_RX: usize = 1024;", SERVICE_BINARY)
        self.assertIn("let mut pending_driver_rx: VecDeque<Vec<u8>> = VecDeque::new();", SERVICE_BINARY)
        self.assertIn("queue.push_back(packet);", SERVICE_BINARY)
        self.assertIn("queue.pop_front()", SERVICE_BINARY)
        self.assertIn("pending_driver_rx.push_front(packet);", SERVICE_BINARY)
        self.assertIn("bounded inbound driver retry queue is full", SERVICE_BINARY)
        polling = SERVICE_BINARY.split("// Retry retained payloads in FIFO order", 1)[1]
        polling = polling.split("// Retry only after the full failure-detection interval", 1)[0]
        self.assertLess(polling.index("pending_driver_rx.pop_front()"), polling.index("sock.recv_from"))
        self.assertIn("queue_pending_driver_rx(&mut pending_driver_rx, data)?;", polling)
        self.assertIn("Err(error) if is_driver_not_ready(&error)", polling)
        self.assertIn("pending_driver_rx.len() >= MAX_PENDING_DRIVER_RX", polling)
        self.assertIn("fn pending_driver_receive_queue_preserves_fifo_and_fails_closed_at_capacity()", SERVICE_BINARY)


if __name__ == "__main__":
    unittest.main()
