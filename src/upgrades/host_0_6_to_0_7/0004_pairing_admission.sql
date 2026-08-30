CREATE INDEX agent_pairing_requests_pending_device
    ON agent_pairing_requests(requested_host_id, expires_at)
    WHERE status = 'pending';
