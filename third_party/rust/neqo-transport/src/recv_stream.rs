// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// Building a stream of ordered bytes to give the application from a series of
// incoming STREAM frames.

use std::cell::RefCell;
use std::cmp::max;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::mem;
use std::ops::Bound::{Included, Unbounded};
use std::rc::Rc;

use smallvec::SmallVec;

use crate::events::ConnectionEvents;
use crate::flow_mgr::FlowMgr;
use crate::stream_id::StreamId;
use crate::{AppError, Error, Res};
use neqo_common::qtrace;

const RX_STREAM_DATA_WINDOW: u64 = 0x10_0000; // 1MiB

// Export as usize for consistency with SEND_BUFFER_SIZE
pub const RECV_BUFFER_SIZE: usize = RX_STREAM_DATA_WINDOW as usize;

pub(crate) type RecvStreams = BTreeMap<StreamId, RecvStream>;

/// Holds data not yet read by application. Orders and dedupes data ranges
/// from incoming STREAM frames.
#[derive(Debug, Default)]
pub struct RxStreamOrderer {
    data_ranges: BTreeMap<u64, Vec<u8>>, // (start_offset, data)
    retired: u64,                        // Number of bytes the application has read
}

impl RxStreamOrderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process an incoming stream frame off the wire. This may result in data
    /// being available to upper layers if frame is not out of order (ooo) or
    /// if the frame fills a gap.
    pub fn inbound_frame(&mut self, new_start: u64, mut new_data: Vec<u8>) -> Res<()> {
        qtrace!("Inbound data offset={} len={}", new_start, new_data.len());

        // Get entry before where new entry would go, so we can see if we already
        // have the new bytes.
        // Avoid copies and duplicated data.
        let new_end = new_start + new_data.len() as u64;

        if new_end <= self.retired {
            // Range already read by application, this frame is very late and unneeded.
            return Ok(());
        }

        if new_data.is_empty() {
            // No data to insert
            return Ok(());
        }

        let (insert_new, remove_prev) = if let Some((&prev_start, prev_vec)) = self
            .data_ranges
            .range_mut((Unbounded, Included(new_start)))
            .next_back()
        {
            let prev_end = prev_start + prev_vec.len() as u64;

            match (new_start > prev_start, new_end > prev_end) {
                (true, true) => {
                    // PPPPPP    ->  PPPPPP
                    //   NNNNNN            NN
                    // Add a range containing only new data
                    // (In-order frames will take this path, with no overlap)
                    let overlap = prev_end.saturating_sub(new_start);
                    qtrace!(
                        "New frame {}-{} received, overlap: {}",
                        new_start,
                        new_end,
                        overlap
                    );
                    if overlap != 0 {
                        new_data.drain(..overlap as usize);
                        return self.inbound_frame(prev_end, new_data);
                    }
                    (true, None)
                }
                (true, false) => {
                    // PPPPPP    ->  PPPPPP
                    //   NNNN
                    // Do nothing
                    qtrace!(
                        "Dropping frame with already-received range {}-{}",
                        new_start,
                        new_end
                    );
                    (false, None)
                }
                (false, true) => {
                    // PPPP      ->  PPPP
                    // NNNNNN            NN
                    qtrace!(
                        "New frame with {}-{} overlaps with existing {}-{}",
                        new_start,
                        new_end,
                        prev_start,
                        prev_end
                    );
                    let overlap = prev_end.saturating_sub(new_start);
                    new_data.drain(..overlap as usize);
                    return self.inbound_frame(prev_end, new_data);
                }
                (false, false) => {
                    // PPPPPP    ->  PPPPPP
                    // NNNN
                    // Do nothing
                    qtrace!(
                        "Dropping frame with already-received range {}-{}",
                        new_start,
                        new_end
                    );
                    (false, None)
                }
            }
        } else {
            qtrace!("New frame {}-{} received", new_start, new_end);
            (true, None) // Nothing previous
        };

        if let Some(remove_prev) = &remove_prev {
            self.data_ranges.remove(remove_prev);
        }

        if insert_new {
            // Now handle possible overlap with next entries
            let mut to_remove = SmallVec::<[_; 8]>::new();

            for (&next_start, next_data) in self.data_ranges.range_mut(new_start..) {
                let next_end = next_start + next_data.len() as u64;
                let overlap = new_end.saturating_sub(next_start);
                if overlap == 0 {
                    break;
                } else if next_end > new_end {
                    qtrace!(
                        "New frame {}-{} overlaps with next frame by {}, truncating",
                        new_start,
                        new_end,
                        overlap
                    );
                    let truncate_to = new_data.len() - overlap as usize;
                    new_data.truncate(truncate_to);
                    break;
                } else {
                    qtrace!(
                        "New frame {}-{} spans entire next frame {}-{}, replacing",
                        new_start,
                        new_end,
                        next_start,
                        next_end
                    );
                    to_remove.push(next_start);
                }
            }

            for start in to_remove {
                self.data_ranges.remove(&start);
            }

            if !new_data.is_empty() {
                self.data_ranges.insert(new_start, new_data);
            }
        };

        Ok(())
    }

    /// Are any bytes readable?
    pub fn data_ready(&self) -> bool {
        self.data_ranges
            .keys()
            .next()
            .map_or(false, |&start| start <= self.retired)
    }

    /// How many bytes are readable?
    fn bytes_ready(&self) -> usize {
        let mut prev_end = self.retired;
        self.data_ranges
            .iter()
            .map(|(start_offset, data)| {
                // All ranges don't overlap but we could have partially
                // retired some of the first entry's data.
                let data_len = data.len() as u64 - self.retired.saturating_sub(*start_offset);
                (start_offset, data_len)
            })
            .take_while(|(start_offset, data_len)| {
                if **start_offset <= prev_end {
                    prev_end += data_len;
                    true
                } else {
                    false
                }
            })
            .map(|(_, data_len)| data_len as usize)
            .sum()
    }

    /// Bytes read by the application.
    fn retired(&self) -> u64 {
        self.retired
    }

    /// Data bytes buffered. Could be more than bytes_readable if there are
    /// ranges missing.
    fn buffered(&self) -> u64 {
        self.data_ranges
            .iter()
            .map(|(&start, data)| data.len() as u64 - (self.retired.saturating_sub(start)))
            .sum()
    }

    /// Copy received data (if any) into the buffer. Returns bytes copied.
    fn read(&mut self, buf: &mut [u8]) -> usize {
        qtrace!("Reading {} bytes, {} available", buf.len(), self.buffered());
        let mut copied = 0;

        for (&range_start, range_data) in &mut self.data_ranges {
            let mut keep = false;
            if self.retired >= range_start {
                // Frame data has new contiguous bytes.
                let copy_offset =
                    usize::try_from(max(range_start, self.retired) - range_start).unwrap();
                assert!(range_data.len() >= copy_offset);
                let available = range_data.len() - copy_offset;
                let space = buf.len() - copied;
                let copy_bytes = if available > space {
                    keep = true;
                    space
                } else {
                    available
                };

                if copy_bytes > 0 {
                    let copy_slc = &range_data[copy_offset..copy_offset + copy_bytes];
                    buf[copied..copied + copy_bytes].copy_from_slice(copy_slc);
                    copied += copy_bytes;
                    self.retired += u64::try_from(copy_bytes).unwrap();
                }
            } else {
                // The data in the buffer isn't contiguous.
                keep = true;
            }
            if keep {
                let mut keep = self.data_ranges.split_off(&range_start);
                mem::swap(&mut self.data_ranges, &mut keep);
                return copied;
            }
        }

        self.data_ranges.clear();
        copied
    }

    /// Extend the given Vector with any available data.
    pub fn read_to_end(&mut self, buf: &mut Vec<u8>) -> usize {
        let orig_len = buf.len();
        buf.resize(orig_len + self.bytes_ready(), 0);
        self.read(&mut buf[orig_len..])
    }

    fn highest_seen_offset(&self) -> u64 {
        let maybe_ooo_last = self
            .data_ranges
            .iter()
            .next_back()
            .map(|(start, data)| *start + data.len() as u64);
        maybe_ooo_last.unwrap_or(self.retired)
    }
}

/// QUIC receiving states, based on -transport 3.2.
#[derive(Debug)]
#[allow(dead_code)]
// Because a dead_code warning is easier than clippy::unused_self, see https://github.com/rust-lang/rust/issues/68408
enum RecvStreamState {
    Recv {
        recv_buf: RxStreamOrderer,
        max_bytes: u64, // Maximum size of recv_buf
        max_stream_data: u64,
    },
    SizeKnown {
        recv_buf: RxStreamOrderer,
        final_size: u64,
    },
    DataRecvd {
        recv_buf: RxStreamOrderer,
    },
    DataRead,
    ResetRecvd,
    // Defined by spec but we don't use it: ResetRead
}

impl RecvStreamState {
    fn new(max_bytes: u64) -> Self {
        Self::Recv {
            recv_buf: RxStreamOrderer::new(),
            max_bytes,
            max_stream_data: max_bytes,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Recv { .. } => "Recv",
            Self::SizeKnown { .. } => "SizeKnown",
            Self::DataRecvd { .. } => "DataRecvd",
            Self::DataRead => "DataRead",
            Self::ResetRecvd => "ResetRecvd",
        }
    }

    fn recv_buf(&self) -> Option<&RxStreamOrderer> {
        match self {
            Self::Recv { recv_buf, .. }
            | Self::SizeKnown { recv_buf, .. }
            | Self::DataRecvd { recv_buf } => Some(recv_buf),
            Self::DataRead | Self::ResetRecvd => None,
        }
    }

    fn final_size(&self) -> Option<u64> {
        match self {
            Self::SizeKnown { final_size, .. } => Some(*final_size),
            _ => None,
        }
    }

    fn max_stream_data(&self) -> Option<u64> {
        match self {
            Self::Recv {
                max_stream_data, ..
            } => Some(*max_stream_data),
            _ => None,
        }
    }
}

/// Implement a QUIC receive stream.
#[derive(Debug)]
pub struct RecvStream {
    stream_id: StreamId,
    state: RecvStreamState,
    flow_mgr: Rc<RefCell<FlowMgr>>,
    conn_events: ConnectionEvents,
}

impl RecvStream {
    pub fn new(
        stream_id: StreamId,
        max_stream_data: u64,
        flow_mgr: Rc<RefCell<FlowMgr>>,
        conn_events: ConnectionEvents,
    ) -> Self {
        Self {
            stream_id,
            state: RecvStreamState::new(max_stream_data),
            flow_mgr,
            conn_events,
        }
    }

    fn set_state(&mut self, new_state: RecvStreamState) {
        debug_assert_ne!(
            mem::discriminant(&self.state),
            mem::discriminant(&new_state)
        );
        qtrace!(
            "RecvStream {} state {} -> {}",
            self.stream_id.as_u64(),
            self.state.name(),
            new_state.name()
        );

        if let RecvStreamState::Recv { .. } = &self.state {
            self.flow_mgr
                .borrow_mut()
                .clear_max_stream_data(self.stream_id)
        }

        if let RecvStreamState::DataRead = new_state {
            self.conn_events.recv_stream_complete(self.stream_id);
        }

        self.state = new_state;
    }

    pub fn inbound_stream_frame(&mut self, fin: bool, offset: u64, data: Vec<u8>) -> Res<()> {
        let new_end = offset + data.len() as u64;

        // Send final size errors even if stream is closed
        if let Some(final_size) = self.state.final_size() {
            if new_end > final_size || (fin && new_end != final_size) {
                return Err(Error::FinalSizeError);
            }
        }

        match &mut self.state {
            RecvStreamState::Recv {
                recv_buf,
                max_stream_data,
                ..
            } => {
                if new_end > *max_stream_data {
                    qtrace!("Stream RX window {} exceeded: {}", max_stream_data, new_end);
                    return Err(Error::FlowControlError);
                }

                if fin {
                    let final_size = offset + data.len() as u64;
                    if final_size < recv_buf.highest_seen_offset() {
                        return Err(Error::FinalSizeError);
                    }
                    recv_buf.inbound_frame(offset, data)?;

                    let buf = mem::replace(recv_buf, RxStreamOrderer::new());
                    if final_size == buf.retired() + buf.bytes_ready() as u64 {
                        self.set_state(RecvStreamState::DataRecvd { recv_buf: buf });
                    } else {
                        self.set_state(RecvStreamState::SizeKnown {
                            recv_buf: buf,
                            final_size,
                        });
                    }
                } else {
                    recv_buf.inbound_frame(offset, data)?;
                }
            }
            RecvStreamState::SizeKnown {
                recv_buf,
                final_size,
            } => {
                recv_buf.inbound_frame(offset, data)?;
                if *final_size == recv_buf.retired() + recv_buf.bytes_ready() as u64 {
                    let buf = mem::replace(recv_buf, RxStreamOrderer::new());
                    self.set_state(RecvStreamState::DataRecvd { recv_buf: buf });
                }
            }
            RecvStreamState::DataRecvd { .. }
            | RecvStreamState::DataRead
            | RecvStreamState::ResetRecvd => {
                qtrace!("data received when we are in state {}", self.state.name())
            }
        }

        if self.data_ready() || self.needs_to_inform_app_about_fin() {
            self.conn_events.recv_stream_readable(self.stream_id)
        }

        Ok(())
    }

    pub fn reset(&mut self, application_error_code: AppError) {
        match self.state {
            RecvStreamState::Recv { .. } | RecvStreamState::SizeKnown { .. } => {
                self.conn_events
                    .recv_stream_reset(self.stream_id, application_error_code);
                self.set_state(RecvStreamState::ResetRecvd);
            }
            _ => {
                // Ignore reset if in DataRecvd, DataRead, or ResetRecvd
            }
        }
    }

    /// If we should tell the sender they have more credit, return an offset
    pub fn maybe_send_flowc_update(&mut self) {
        // Only ever needed if actively receiving and not in SizeKnown state
        if let RecvStreamState::Recv {
            max_bytes,
            max_stream_data,
            recv_buf,
        } = &mut self.state
        {
            // Algo: send an update if app has consumed more than half
            // the data in the current window
            // TODO(agrover@mozilla.com): This algo is not great but
            // should prevent Silly Window Syndrome. Spec refers to using
            // highest seen offset somehow? RTT maybe?
            let maybe_new_max = recv_buf.retired() + *max_bytes;
            if maybe_new_max > (*max_bytes / 2) + *max_stream_data {
                *max_stream_data = maybe_new_max;
                self.flow_mgr
                    .borrow_mut()
                    .max_stream_data(self.stream_id, maybe_new_max)
            }
        }
    }

    pub fn max_stream_data(&self) -> Option<u64> {
        self.state.max_stream_data()
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            RecvStreamState::ResetRecvd | RecvStreamState::DataRead
        )
    }

    // App got all data but did not get the fin signal.
    fn needs_to_inform_app_about_fin(&self) -> bool {
        matches!(self.state, RecvStreamState::DataRecvd { .. })
    }

    fn data_ready(&self) -> bool {
        self.state
            .recv_buf()
            .map_or(false, RxStreamOrderer::data_ready)
    }

    /// # Errors
    /// `NoMoreData` if data and fin bit were previously read by the application.
    pub fn read(&mut self, buf: &mut [u8]) -> Res<(usize, bool)> {
        let res = match &mut self.state {
            RecvStreamState::Recv { recv_buf, .. }
            | RecvStreamState::SizeKnown { recv_buf, .. } => Ok((recv_buf.read(buf), false)),
            RecvStreamState::DataRecvd { recv_buf } => {
                let bytes_read = recv_buf.read(buf);
                let fin_read = recv_buf.buffered() == 0;
                if fin_read {
                    self.set_state(RecvStreamState::DataRead);
                }
                Ok((bytes_read, fin_read))
            }
            RecvStreamState::DataRead | RecvStreamState::ResetRecvd => Err(Error::NoMoreData),
        };
        self.maybe_send_flowc_update();
        res
    }

    pub fn stop_sending(&mut self, err: AppError) {
        qtrace!("stop_sending called when in state {}", self.state.name());
        match &self.state {
            RecvStreamState::Recv { .. } | RecvStreamState::SizeKnown { .. } => {
                self.set_state(RecvStreamState::ResetRecvd);
                self.flow_mgr.borrow_mut().stop_sending(self.stream_id, err)
            }
            RecvStreamState::DataRecvd { .. } => self.set_state(RecvStreamState::DataRead),
            RecvStreamState::DataRead | RecvStreamState::ResetRecvd => {
                // Already in terminal state
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Frame;
    use std::ops::Range;

    fn recv_ranges(ranges: &[Range<u64>], available: usize) {
        const ZEROES: &[u8] = &[0; 100];

        let mut s = RxStreamOrderer::default();
        for r in ranges {
            let data = ZEROES[..usize::try_from(r.end - r.start).unwrap()].to_vec();
            s.inbound_frame(r.start, data).unwrap();
        }

        let mut buf = vec![0xff; 100];
        let mut total_recvd = 0;
        loop {
            let recvd = s.read(&mut buf);
            total_recvd += recvd;
            if recvd == 0 {
                assert_eq!(total_recvd, available);
                break;
            }
        }
    }

    #[test]
    fn recv_noncontiguous() {
        // Non-contiguous with the start, no data available.
        recv_ranges(&[10..20], 0);
    }

    /// Overlaps with the start of a 10..20 range of bytes.
    #[test]
    fn recv_overlap_start() {
        // Overlap the start, with a larger new value.
        // More overlap than not.
        recv_ranges(&[10..20, 4..18, 0..4], 20);
        // Overlap the start, with a larger new value.
        // Less overlap than not.
        recv_ranges(&[10..20, 2..15, 0..2], 20);
        // Overlap the start, with a smaller new value.
        // More overlap than not.
        recv_ranges(&[10..20, 8..14, 0..8], 20);
        // Overlap the start, with a smaller new value.
        // Less overlap than not.
        recv_ranges(&[10..20, 6..13, 0..6], 20);

        // Again with some of the first range split in two.
        recv_ranges(&[10..11, 11..20, 4..18, 0..4], 20);
        recv_ranges(&[10..11, 11..20, 2..15, 0..2], 20);
        recv_ranges(&[10..11, 11..20, 8..14, 0..8], 20);
        recv_ranges(&[10..11, 11..20, 6..13, 0..6], 20);

        // Again with a gap in the first range.
        recv_ranges(&[10..11, 12..20, 4..18, 0..4], 20);
        recv_ranges(&[10..11, 12..20, 2..15, 0..2], 20);
        recv_ranges(&[10..11, 12..20, 8..14, 0..8], 20);
        recv_ranges(&[10..11, 12..20, 6..13, 0..6], 20);
    }

    /// Overlaps with the end of a 10..20 range of bytes.
    #[test]
    fn recv_overlap_end() {
        // Overlap the end, with a larger new value.
        // More overlap than not.
        recv_ranges(&[10..20, 12..25, 0..10], 25);
        // Overlap the end, with a larger new value.
        // Less overlap than not.
        recv_ranges(&[10..20, 17..33, 0..10], 33);
        // Overlap the end, with a smaller new value.
        // More overlap than not.
        recv_ranges(&[10..20, 15..21, 0..10], 21);
        // Overlap the end, with a smaller new value.
        // Less overlap than not.
        recv_ranges(&[10..20, 17..25, 0..10], 25);

        // Again with some of the first range split in two.
        recv_ranges(&[10..19, 19..20, 12..25, 0..10], 25);
        recv_ranges(&[10..19, 19..20, 17..33, 0..10], 33);
        recv_ranges(&[10..19, 19..20, 15..21, 0..10], 21);
        recv_ranges(&[10..19, 19..20, 17..25, 0..10], 25);

        // Again with a gap in the first range.
        recv_ranges(&[10..18, 19..20, 12..25, 0..10], 25);
        recv_ranges(&[10..18, 19..20, 17..33, 0..10], 33);
        recv_ranges(&[10..18, 19..20, 15..21, 0..10], 21);
        recv_ranges(&[10..18, 19..20, 17..25, 0..10], 25);
    }

    /// Complete overlaps with the start of a 10..20 range of bytes.
    #[test]
    fn recv_overlap_complete() {
        // Complete overlap, more at the end.
        recv_ranges(&[10..20, 9..23, 0..9], 23);
        // Complete overlap, more at the start.
        recv_ranges(&[10..20, 3..23, 0..3], 23);
        // Complete overlap, to end.
        recv_ranges(&[10..20, 5..20, 0..5], 20);
        // Complete overlap, from start.
        recv_ranges(&[10..20, 10..27, 0..10], 27);
        // Complete overlap, from 0 and more.
        recv_ranges(&[10..20, 0..23], 23);

        // Again with the first range split in two.
        recv_ranges(&[10..14, 14..20, 9..23, 0..9], 23);
        recv_ranges(&[10..14, 14..20, 3..23, 0..3], 23);
        recv_ranges(&[10..14, 14..20, 5..20, 0..5], 20);
        recv_ranges(&[10..14, 14..20, 10..27, 0..10], 27);
        recv_ranges(&[10..14, 14..20, 0..23], 23);

        // Again with the a gap in the first range.
        recv_ranges(&[10..13, 14..20, 9..23, 0..9], 23);
        recv_ranges(&[10..13, 14..20, 3..23, 0..3], 23);
        recv_ranges(&[10..13, 14..20, 5..20, 0..5], 20);
        recv_ranges(&[10..13, 14..20, 10..27, 0..10], 27);
        recv_ranges(&[10..13, 14..20, 0..23], 23);
    }

    /// An overlap with no new bytes.
    #[test]
    fn recv_overlap_duplicate() {
        recv_ranges(&[10..20, 11..12, 0..10], 20);
        recv_ranges(&[10..20, 10..15, 0..10], 20);
        recv_ranges(&[10..20, 14..20, 0..10], 20);
        // Now with the first range split.
        recv_ranges(&[10..14, 14..20, 10..15, 0..10], 20);
        recv_ranges(&[10..15, 16..20, 21..25, 10..25, 0..10], 25);
    }

    /// Reading exactly one chunk works, when the next chunk starts immediately.
    #[test]
    fn stop_reading_at_chunk() {
        const CHUNK_SIZE: usize = 10;
        const EXTRA_SIZE: usize = 3;
        let mut s = RxStreamOrderer::new();

        // Add three chunks.
        s.inbound_frame(0, vec![0; CHUNK_SIZE]).unwrap();
        let offset = u64::try_from(CHUNK_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();
        let offset = u64::try_from(CHUNK_SIZE + EXTRA_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();

        // Read, providing only enough space for the first.
        let mut buf = vec![0; 100];
        let count = s.read(&mut buf[..CHUNK_SIZE]);
        assert_eq!(count, CHUNK_SIZE);
        let count = s.read(&mut buf[..]);
        assert_eq!(count, EXTRA_SIZE * 2);
    }

    #[test]
    fn recv_overlap_while_reading() {
        let mut s = RxStreamOrderer::new();

        // Add a chunk
        s.inbound_frame(0, vec![0; 150]).unwrap();
        assert_eq!(s.data_ranges.get(&0).unwrap().len(), 150);
        // Read, providing only enough space for the first.
        let mut buf = vec![0; 100];
        let count = s.read(&mut buf);
        assert_eq!(count, 100);
        assert_eq!(s.retired, 100);

        // Add a second frame that overlaps.
        // This shouldn't truncate the first frame, as we're already
        // Reading from it.
        s.inbound_frame(120, vec![0; 60]).unwrap();
        assert_eq!(s.data_ranges.get(&0).unwrap().len(), 150);
        assert_eq!(s.data_ranges.get(&150).unwrap().len(), 30);
        // Read second part of first frame and all of the second frame
        let count = s.read(&mut buf);
        assert_eq!(count, 80);
    }

    /// Reading exactly one chunk works, when there is a gap.
    #[test]
    fn stop_reading_at_gap() {
        const CHUNK_SIZE: usize = 10;
        const EXTRA_SIZE: usize = 3;
        let mut s = RxStreamOrderer::new();

        // Add three chunks.
        s.inbound_frame(0, vec![0; CHUNK_SIZE]).unwrap();
        let offset = u64::try_from(CHUNK_SIZE + EXTRA_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();

        // Read, providing only enough space for the first chunk.
        let mut buf = vec![0; 100];
        let count = s.read(&mut buf[..CHUNK_SIZE]);
        assert_eq!(count, CHUNK_SIZE);

        // Now fill the gap and ensure that everything can be read.
        let offset = u64::try_from(CHUNK_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();
        let count = s.read(&mut buf[..]);
        assert_eq!(count, EXTRA_SIZE * 2);
    }

    /// Reading exactly one chunk works, when there is a gap.
    #[test]
    fn stop_reading_in_chunk() {
        const CHUNK_SIZE: usize = 10;
        const EXTRA_SIZE: usize = 3;
        let mut s = RxStreamOrderer::new();

        // Add two chunks.
        s.inbound_frame(0, vec![0; CHUNK_SIZE]).unwrap();
        let offset = u64::try_from(CHUNK_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();

        // Read, providing only enough space for some of the first chunk.
        let mut buf = vec![0; 100];
        let count = s.read(&mut buf[..CHUNK_SIZE - EXTRA_SIZE]);
        assert_eq!(count, CHUNK_SIZE - EXTRA_SIZE);

        let count = s.read(&mut buf[..]);
        assert_eq!(count, EXTRA_SIZE * 2);
    }

    /// Read one byte at a time.
    #[test]
    fn read_byte_at_a_time() {
        const CHUNK_SIZE: usize = 10;
        const EXTRA_SIZE: usize = 3;
        let mut s = RxStreamOrderer::new();

        // Add two chunks.
        s.inbound_frame(0, vec![0; CHUNK_SIZE]).unwrap();
        let offset = u64::try_from(CHUNK_SIZE).unwrap();
        s.inbound_frame(offset, vec![0; EXTRA_SIZE]).unwrap();

        let mut buf = vec![0; 1];
        for _ in 0..CHUNK_SIZE + EXTRA_SIZE {
            let count = s.read(&mut buf[..]);
            assert_eq!(count, 1);
        }
        assert_eq!(0, s.read(&mut buf[..]));
    }

    #[test]
    fn test_stream_rx() {
        let flow_mgr = Rc::new(RefCell::new(FlowMgr::default()));
        let conn_events = ConnectionEvents::default();

        let mut s = RecvStream::new(567.into(), 1024, Rc::clone(&flow_mgr), conn_events);

        // test receiving a contig frame and reading it works
        s.inbound_stream_frame(false, 0, vec![1; 10]).unwrap();
        assert_eq!(s.data_ready(), true);
        let mut buf = vec![0u8; 100];
        assert_eq!(s.read(&mut buf).unwrap(), (10, false));
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 0);

        // test receiving a noncontig frame
        s.inbound_stream_frame(false, 12, vec![2; 12]).unwrap();
        assert_eq!(s.data_ready(), false);
        assert_eq!(s.read(&mut buf).unwrap(), (0, false));
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 12);

        // another frame that overlaps the first
        s.inbound_stream_frame(false, 14, vec![3; 8]).unwrap();
        assert_eq!(s.data_ready(), false);
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 12);

        // fill in the gap, but with a FIN
        s.inbound_stream_frame(true, 10, vec![4; 6]).unwrap_err();
        assert_eq!(s.data_ready(), false);
        assert_eq!(s.read(&mut buf).unwrap(), (0, false));
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 12);

        // fill in the gap
        s.inbound_stream_frame(false, 10, vec![5; 10]).unwrap();
        assert_eq!(s.data_ready(), true);
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 14);

        // a legit FIN
        s.inbound_stream_frame(true, 24, vec![6; 18]).unwrap();
        assert_eq!(s.state.recv_buf().unwrap().retired(), 10);
        assert_eq!(s.state.recv_buf().unwrap().buffered(), 32);
        assert_eq!(s.data_ready(), true);
        assert_eq!(s.read(&mut buf).unwrap(), (32, true));

        // Stream now no longer readable (is in DataRead state)
        s.read(&mut buf).unwrap_err();
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_stream_rx_dedupe() {
        let flow_mgr = Rc::new(RefCell::new(FlowMgr::default()));
        let conn_events = ConnectionEvents::default();

        let mut s = RecvStream::new(3.into(), 1024, Rc::clone(&flow_mgr), conn_events);

        let mut buf = vec![0u8; 100];

        // test receiving a contig frame and reading it works
        s.inbound_stream_frame(false, 0, vec![1; 6]).unwrap();

        // See inbound_frame(). Test (true, true) case
        s.inbound_stream_frame(false, 2, vec![2; 6]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 0);
            assert_eq!(item.1.len(), 6);
            let item = i.next().unwrap();
            assert_eq!(*item.0, 6);
            assert_eq!(item.1.len(), 2);
        }

        // Test (true, false) case
        s.inbound_stream_frame(false, 4, vec![3; 4]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 0);
            assert_eq!(item.1.len(), 6);
            let item = i.next().unwrap();
            assert_eq!(*item.0, 6);
            assert_eq!(item.1.len(), 2);
        }

        // Test (false, true) case
        s.inbound_stream_frame(false, 2, vec![4; 8]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 0);
            assert_eq!(item.1.len(), 6);
            let item = i.next().unwrap();
            assert_eq!(*item.0, 6);
            assert_eq!(item.1.len(), 2);
        }

        // Test (false, false) case
        s.inbound_stream_frame(false, 2, vec![5; 2]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 0);
            assert_eq!(item.1.len(), 6);
            let item = i.next().unwrap();
            assert_eq!(*item.0, 6);
            assert_eq!(item.1.len(), 2);
        }

        assert_eq!(s.read(&mut buf).unwrap(), (10, false));
        assert_eq!(buf[..10], [1, 1, 1, 1, 1, 1, 2, 2, 4, 4]);

        // Test truncation/span-drop on insert
        s.inbound_stream_frame(false, 100, vec![6; 6]).unwrap();
        // a. insert where new frame gets truncated
        s.inbound_stream_frame(false, 99, vec![7; 6]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 99);
            assert_eq!(item.1.len(), 1);
            let item = i.next().unwrap();
            assert_eq!(*item.0, 100);
            assert_eq!(item.1.len(), 6);
            assert_eq!(i.next(), None);
        }

        // b. insert where new frame spans next frame
        s.inbound_stream_frame(false, 98, vec![8; 10]).unwrap();
        {
            let mut i = s.state.recv_buf().unwrap().data_ranges.iter();
            let item = i.next().unwrap();
            assert_eq!(*item.0, 98);
            assert_eq!(item.1.len(), 10);
            assert_eq!(i.next(), None);
        }
    }

    #[test]
    fn test_stream_flowc_update() {
        let flow_mgr = Rc::default();
        let conn_events = ConnectionEvents::default();

        let frame1 = vec![0; RX_STREAM_DATA_WINDOW as usize];

        let mut s = RecvStream::new(
            4.into(),
            RX_STREAM_DATA_WINDOW,
            Rc::clone(&flow_mgr),
            conn_events,
        );

        let mut buf = vec![0u8; RX_STREAM_DATA_WINDOW as usize * 4]; // Make it overlarge

        s.maybe_send_flowc_update();
        assert_eq!(s.flow_mgr.borrow().peek(), None);
        s.inbound_stream_frame(false, 0, frame1).unwrap();
        s.maybe_send_flowc_update();
        assert_eq!(s.flow_mgr.borrow().peek(), None);
        assert_eq!(
            s.read(&mut buf).unwrap(),
            (RX_STREAM_DATA_WINDOW as usize, false)
        );
        assert_eq!(s.data_ready(), false);
        s.maybe_send_flowc_update();

        // flow msg generated!
        assert!(s.flow_mgr.borrow().peek().is_some());

        // consume it
        s.flow_mgr.borrow_mut().next().unwrap();

        // it should be gone
        s.maybe_send_flowc_update();
        assert_eq!(s.flow_mgr.borrow().peek(), None);
    }

    #[test]
    fn test_stream_max_stream_data() {
        let flow_mgr = Rc::new(RefCell::new(FlowMgr::default()));
        let conn_events = ConnectionEvents::default();

        let frame1 = vec![0; RX_STREAM_DATA_WINDOW as usize];

        let mut s = RecvStream::new(
            67.into(),
            RX_STREAM_DATA_WINDOW,
            Rc::clone(&flow_mgr),
            conn_events,
        );

        s.maybe_send_flowc_update();
        assert_eq!(s.flow_mgr.borrow().peek(), None);
        s.inbound_stream_frame(false, 0, frame1).unwrap();
        s.inbound_stream_frame(false, RX_STREAM_DATA_WINDOW, vec![1; 1])
            .unwrap_err();
    }

    #[test]
    fn test_stream_orderer_bytes_ready() {
        let mut rx_ord = RxStreamOrderer::new();

        let mut buf = vec![0u8; 100];

        rx_ord.inbound_frame(0, vec![1; 6]).unwrap();
        assert_eq!(rx_ord.bytes_ready(), 6);
        assert_eq!(rx_ord.buffered(), 6);
        assert_eq!(rx_ord.retired(), 0);
        // read some so there's an offset into the first frame
        rx_ord.read(&mut buf[..2]);
        assert_eq!(rx_ord.bytes_ready(), 4);
        assert_eq!(rx_ord.buffered(), 4);
        assert_eq!(rx_ord.retired(), 2);
        // an overlapping frame
        rx_ord.inbound_frame(5, vec![2; 6]).unwrap();
        assert_eq!(rx_ord.bytes_ready(), 9);
        assert_eq!(rx_ord.buffered(), 9);
        assert_eq!(rx_ord.retired(), 2);
        // a noncontig frame
        rx_ord.inbound_frame(20, vec![3; 6]).unwrap();
        assert_eq!(rx_ord.bytes_ready(), 9);
        assert_eq!(rx_ord.buffered(), 15);
        assert_eq!(rx_ord.retired(), 2);
        // an old frame
        rx_ord.inbound_frame(0, vec![4; 2]).unwrap();
        assert_eq!(rx_ord.bytes_ready(), 9);
        assert_eq!(rx_ord.buffered(), 15);
        assert_eq!(rx_ord.retired(), 2);
    }

    #[test]
    fn no_stream_flowc_event_after_exiting_recv() {
        let flow_mgr = Rc::new(RefCell::new(FlowMgr::default()));
        let conn_events = ConnectionEvents::default();

        let frame1 = vec![0; RX_STREAM_DATA_WINDOW as usize];

        let mut s = RecvStream::new(
            67.into(),
            RX_STREAM_DATA_WINDOW,
            Rc::clone(&flow_mgr),
            conn_events,
        );

        s.inbound_stream_frame(false, 0, frame1).unwrap();
        flow_mgr.borrow_mut().max_stream_data(67.into(), 100);
        assert!(matches!(s.flow_mgr.borrow().peek().unwrap(), Frame::MaxStreamData{..}));
        s.inbound_stream_frame(true, RX_STREAM_DATA_WINDOW, vec![])
            .unwrap();
        assert!(matches!(s.flow_mgr.borrow().peek(), None));
    }

    #[test]
    fn resend_flowc_if_lost() {
        let flow_mgr = Rc::new(RefCell::new(FlowMgr::default()));
        let conn_events = ConnectionEvents::default();

        let frame1 = vec![0; RX_STREAM_DATA_WINDOW as usize];

        let mut s = RecvStream::new(
            67.into(),
            RX_STREAM_DATA_WINDOW,
            Rc::clone(&flow_mgr),
            conn_events,
        );

        // A flow control update is queued
        s.inbound_stream_frame(false, 0, frame1).unwrap();
        flow_mgr.borrow_mut().max_stream_data(67.into(), 100);
        // Generates frame
        assert!(matches!(
            s.flow_mgr.borrow_mut().next().unwrap(),
            Frame::MaxStreamData { .. }
        ));
        // Nothing else queued
        assert!(matches!(s.flow_mgr.borrow().peek(), None));
        // Asking for another one won't get you one
        s.maybe_send_flowc_update();
        assert!(matches!(s.flow_mgr.borrow().peek(), None));
        // But if lost, another frame is generated
        flow_mgr.borrow_mut().max_stream_data(67.into(), 100);
        assert!(matches!(s.flow_mgr.borrow_mut().next().unwrap(), Frame::MaxStreamData{..}));
    }
}
