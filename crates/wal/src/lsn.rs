/// Log Sequence Number — a monotonically increasing 64-bit counter that
/// uniquely identifies every WAL record.
pub type Lsn = u64;

/// Sentinel value meaning "no LSN" / "before the beginning of the log".
pub const INVALID_LSN: Lsn = 0;
