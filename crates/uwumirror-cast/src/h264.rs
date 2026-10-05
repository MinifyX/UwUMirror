//! Just enough H.264 to send it: where NAL units start, which are key
//! frames, and SPS and PPS in front of every key frame.

const IDR: u8 = 5;
const SPS: u8 = 7;
const PPS: u8 = 8;

/// The NAL units of an Annex B access unit, each without its start code.
pub fn nal_units(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(n, &start)| {
            let mut end = starts.get(n + 1).map_or(data.len(), |next| next - 3);
            // A four-byte start code's leading zero belongs to it.
            if n + 1 < starts.len() && end > start && data[end - 1] == 0 {
                end -= 1;
            }
            &data[start..end.max(start)]
        })
        .collect()
}

fn kind(unit: &[u8]) -> u8 {
    unit.first().map_or(0, |b| b & 0x1f)
}

/// The SPS and PPS in `data`, each behind a four-byte start code.
pub fn parameter_sets(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in nal_units(data) {
        if matches!(kind(unit), SPS | PPS) {
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(unit);
        }
    }
    out
}

/// Readies one encoded access unit for the wire: whether it is a key frame,
/// and the data with SPS and PPS in front if it is one and they aren't there
/// yet. `sets` keeps the latest parameter sets seen. `None` when it isn't
/// Annex B at all.
pub fn prepare(data: Vec<u8>, sets: &mut Vec<u8>) -> Option<(bool, Vec<u8>)> {
    if !crate::protocol::is_annex_b(&data) {
        return None;
    }
    let kinds: Vec<u8> = nal_units(&data).into_iter().map(kind).collect();
    let key = kinds.contains(&IDR);
    if kinds.contains(&SPS) {
        let found = parameter_sets(&data);
        if !found.is_empty() {
            *sets = found;
        }
        return Some((key, data));
    }
    if key && !sets.is_empty() {
        return Some((key, [sets.as_slice(), &data].concat()));
    }
    Some((key, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_on_both_start_codes() {
        let data = [
            0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4,
        ];
        let units = nal_units(&data);
        assert_eq!(units, vec![&[0x67, 1, 2][..], &[0x68, 3], &[0x65, 4]]);
    }

    #[test]
    fn key_frames_get_their_parameter_sets() {
        let mut sets = Vec::new();
        let first = vec![
            0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 0, 1, 0x65, 3,
        ];
        let (key, data) = prepare(first.clone(), &mut sets).unwrap();
        assert!(key);
        assert_eq!(data, first, "already there: left alone");
        assert_eq!(sets, vec![0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2]);

        let (key, data) = prepare(vec![0, 0, 0, 1, 0x41, 9], &mut sets).unwrap();
        assert!(!key);
        assert_eq!(data, vec![0, 0, 0, 1, 0x41, 9]);

        let (key, data) = prepare(vec![0, 0, 1, 0x65, 7], &mut sets).unwrap();
        assert!(key);
        assert_eq!(
            data,
            vec![0, 0, 0, 1, 0x67, 1, 0, 0, 0, 1, 0x68, 2, 0, 0, 1, 0x65, 7]
        );

        assert!(prepare(vec![0, 0, 0, 5, 0x65], &mut sets).is_none());
    }
}
