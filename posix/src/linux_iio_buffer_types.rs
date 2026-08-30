//! `<linux/iio/buffer.h>` — Industrial I/O buffer subsystem constants.
//!
//! Constants for the IIO (Industrial I/O) kernel framework's buffer
//! interface — used by ADC, sensor, and DAC userspace consumers to
//! poll and configure ring-buffer-backed sample streams.

// ---------------------------------------------------------------------------
// Buffer access flags
// ---------------------------------------------------------------------------

/// Buffer is enabled (samples are being captured).
pub const IIO_BUFFER_FL_ENABLED: u32 = 0x0001;
/// Buffer can be polled for new data.
pub const IIO_BUFFER_FL_POLLABLE: u32 = 0x0002;
/// Buffer supports DMA-mapped access.
pub const IIO_BUFFER_FL_DMA: u32 = 0x0004;
/// Buffer supports cyclic / repeating output mode.
pub const IIO_BUFFER_FL_CYCLIC: u32 = 0x0008;

// ---------------------------------------------------------------------------
// Buffer direction
// ---------------------------------------------------------------------------

/// Buffer captures data from the device to the host.
pub const IIO_BUFFER_DIRECTION_IN: u32 = 0;
/// Buffer streams data from the host to the device.
pub const IIO_BUFFER_DIRECTION_OUT: u32 = 1;

// ---------------------------------------------------------------------------
// DMA buffer block flags (struct iio_dma_buffer_block.flags)
// ---------------------------------------------------------------------------

/// Block was successfully dequeued.
pub const IIO_BUFFER_BLOCK_FLAG_CYCLIC: u32 = 0x0001;
/// Block contains a timestamp.
pub const IIO_BUFFER_BLOCK_FLAG_TIMESTAMP_VALID: u32 = 0x0002;

// ---------------------------------------------------------------------------
// Per-buffer limits
// ---------------------------------------------------------------------------

/// Maximum number of buffer blocks userspace may enqueue at once.
pub const IIO_BUFFER_BLOCK_MAX: u32 = 64;
/// Default ring length (samples) when none is configured.
pub const IIO_BUFFER_LENGTH_DEFAULT: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_single_bits() {
        for &f in &[
            IIO_BUFFER_FL_ENABLED,
            IIO_BUFFER_FL_POLLABLE,
            IIO_BUFFER_FL_DMA,
            IIO_BUFFER_FL_CYCLIC,
        ] {
            assert!(f.is_power_of_two(), "{f:#x} is not a single bit");
        }
    }

    #[test]
    fn test_flags_distinct() {
        let flags = [
            IIO_BUFFER_FL_ENABLED,
            IIO_BUFFER_FL_POLLABLE,
            IIO_BUFFER_FL_DMA,
            IIO_BUFFER_FL_CYCLIC,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_direction_distinct() {
        assert_ne!(IIO_BUFFER_DIRECTION_IN, IIO_BUFFER_DIRECTION_OUT);
    }

    #[test]
    fn test_block_flags_distinct() {
        assert!(IIO_BUFFER_BLOCK_FLAG_CYCLIC.is_power_of_two());
        assert!(IIO_BUFFER_BLOCK_FLAG_TIMESTAMP_VALID.is_power_of_two());
        assert_ne!(
            IIO_BUFFER_BLOCK_FLAG_CYCLIC,
            IIO_BUFFER_BLOCK_FLAG_TIMESTAMP_VALID
        );
    }

    #[test]
    fn test_limits_sane() {
        assert!(IIO_BUFFER_BLOCK_MAX.is_power_of_two());
        assert!(IIO_BUFFER_BLOCK_MAX >= 2);
        assert!(IIO_BUFFER_LENGTH_DEFAULT >= 1);
    }
}
