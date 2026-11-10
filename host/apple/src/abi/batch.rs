//! Batched input and appearance updates.
use super::*;

impl<D: DataSource> Bridge<D> {
    /// The input buffer's first `len` bytes are a list of
    /// events, each `view`, `kind`, `flags` and payload length (u32, little
    /// endian) then its payload; they are dispatched in order and committed
    /// once ([`crate::host::Host::dispatch_many`]). Flag 1: the entry is on a
    /// propagation path that `stopPropagation` ends; flag 2,
    /// echoed input whose non-path entries are skipped after `preventDefault`. A malformed entry or an
    /// event refused before dispatch refuses the whole list.
    pub fn dispatch_many(&mut self, len: usize, now_ms: f64) -> u32 {
        let list = self.input[..len.min(self.input.len())].to_vec();
        let mut events = Vec::new();
        let mut at = 0;
        let word = |at: usize| {
            list.get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        while at < list.len() {
            let (Some(view), Some(kind), Some(flags), Some(n)) =
                (word(at), word(at + 4), word(at + 8), word(at + 12))
            else {
                return self.refuse("malformed event list");
            };
            let start = at + 16;
            let Some(payload) = list.get(start..start + n as usize) else {
                return self.refuse("malformed event list");
            };
            self.input.clear();
            self.input.extend_from_slice(payload);
            match self.event_of(kind, payload.len()) {
                Ok(event) => events.push((view, event, flags)),
                Err(out) => return out,
            }
            at = start + n as usize;
        }
        let out = match self.host.as_mut() {
            Some(h) => h.dispatch_many(events, now_ms),
            None => not_booted(),
        };
        self.emit(out)
    }

    /// Several views' own appearances from the input buffer's first `len`
    /// bytes: LE records of (u32 view, u32 dark), one batch.
    pub fn view_schemes(&mut self, len: usize) -> u32 {
        let bytes = self.input.get(..len).unwrap_or(&[]);
        let views: Vec<(u32, bool)> = bytes
            .chunks_exact(8)
            .map(|r| {
                let view = u32::from_le_bytes([r[0], r[1], r[2], r[3]]);
                let dark = u32::from_le_bytes([r[4], r[5], r[6], r[7]]) != 0;
                (view, dark)
            })
            .collect();
        let out = self
            .host
            .as_mut()
            .map_or_else(not_booted, |h| h.set_view_schemes(&views));
        self.emit(out)
    }
}
