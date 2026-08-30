ALTER TABLE agent_metric_reports
    ADD COLUMN aggregated_at TEXT;

CREATE INDEX monitored_hosts_latest_report_retention
    ON monitored_hosts(latest_report_id)
    WHERE latest_report_id IS NOT NULL;

CREATE INDEX agent_metric_reports_retention_pending
    ON agent_metric_reports(collected_at, report_id)
    WHERE aggregated_at IS NULL;

CREATE INDEX agent_metric_reports_retention_delete
    ON agent_metric_reports(aggregated_at, report_id)
    WHERE aggregated_at IS NOT NULL;

CREATE TABLE agent_metric_hourly_aggregates (
    host_id                                       TEXT NOT NULL REFERENCES monitored_hosts(host_id) ON DELETE CASCADE,
    bucket_start                                  TEXT NOT NULL,
    interval_start                                TEXT NOT NULL,
    interval_end                                  TEXT NOT NULL,
    sample_count                                  INTEGER NOT NULL CHECK (sample_count > 0),

    cpu_usage_percent_count                       INTEGER NOT NULL,
    cpu_usage_percent_min                         REAL,
    cpu_usage_percent_max                         REAL,
    cpu_usage_percent_avg                         REAL,

    memory_usage_percent_count                    INTEGER NOT NULL,
    memory_usage_percent_min                      REAL,
    memory_usage_percent_max                      REAL,
    memory_usage_percent_avg                      REAL,

    network_received_bytes_per_second_count       INTEGER NOT NULL,
    network_received_bytes_per_second_min         REAL,
    network_received_bytes_per_second_max         REAL,
    network_received_bytes_per_second_avg         REAL,

    network_transmitted_bytes_per_second_count    INTEGER NOT NULL,
    network_transmitted_bytes_per_second_min      REAL,
    network_transmitted_bytes_per_second_max      REAL,
    network_transmitted_bytes_per_second_avg      REAL,

    disk_read_bytes_per_second_count              INTEGER NOT NULL,
    disk_read_bytes_per_second_min                REAL,
    disk_read_bytes_per_second_max                REAL,
    disk_read_bytes_per_second_avg                REAL,

    disk_written_bytes_per_second_count           INTEGER NOT NULL,
    disk_written_bytes_per_second_min             REAL,
    disk_written_bytes_per_second_max             REAL,
    disk_written_bytes_per_second_avg             REAL,

    max_temperature_celsius_count                 INTEGER NOT NULL,
    max_temperature_celsius_min                   REAL,
    max_temperature_celsius_max                   REAL,
    max_temperature_celsius_avg                   REAL,

    gpu_utilization_percent_count                 INTEGER NOT NULL,
    gpu_utilization_percent_min                   REAL,
    gpu_utilization_percent_max                   REAL,
    gpu_utilization_percent_avg                   REAL,

    gpu_memory_usage_percent_count                INTEGER NOT NULL,
    gpu_memory_usage_percent_min                  REAL,
    gpu_memory_usage_percent_max                  REAL,
    gpu_memory_usage_percent_avg                  REAL,

    updated_at                                    TEXT NOT NULL,
    PRIMARY KEY (host_id, bucket_start),
    CHECK (bucket_start <= interval_start),
    CHECK (interval_start <= interval_end),
    CHECK (cpu_usage_percent_count BETWEEN 0 AND sample_count),
    CHECK (memory_usage_percent_count BETWEEN 0 AND sample_count),
    CHECK (network_received_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (network_transmitted_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (disk_read_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (disk_written_bytes_per_second_count BETWEEN 0 AND sample_count),
    CHECK (max_temperature_celsius_count BETWEEN 0 AND sample_count),
    CHECK (gpu_utilization_percent_count BETWEEN 0 AND sample_count),
    CHECK (gpu_memory_usage_percent_count BETWEEN 0 AND sample_count)
);

CREATE INDEX agent_metric_hourly_aggregates_retention
    ON agent_metric_hourly_aggregates(interval_end, host_id, bucket_start);
