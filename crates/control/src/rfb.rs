const HANDSHAKE_LENGTH: usize = 14;

pub struct ClientFilter {
    pending: Vec<u8>,
    handshake_remaining: usize,
    lost_sync: bool,
}

enum Message {
    Incomplete,
    Unknown,
    Known { length: usize, input: bool },
}

fn read_u16(bytes: &[u8], at: usize) -> usize {
    u16::from_be_bytes([bytes[at], bytes[at + 1]]) as usize
}

fn read_u32(bytes: &[u8], at: usize) -> usize {
    u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]) as usize
}

fn next_message(bytes: &[u8]) -> Message {
    let Some(kind) = bytes.first() else {
        return Message::Incomplete;
    };
    let needs = |length: usize| if bytes.len() < length { None } else { Some(length) };
    let known = |length: Option<usize>, input: bool| match length {
        Some(length) => Message::Known { length, input },
        None => Message::Incomplete,
    };

    match kind {
        0 => known(needs(20), false),
        2 => match needs(4) {
            Some(_) => known(needs(4 + 4 * read_u16(bytes, 2)), false),
            None => Message::Incomplete,
        },
        3 => known(needs(10), false),
        4 => known(needs(8), true),
        5 => known(needs(6), true),
        6 => match needs(8) {
            Some(_) => known(needs(8 + read_u32(bytes, 4)), true),
            None => Message::Incomplete,
        },
        150 => known(needs(10), false),
        248 => match needs(9) {
            Some(_) => known(needs(9 + bytes[8] as usize), false),
            None => Message::Incomplete,
        },
        250 => known(needs(4), false),
        251 => match needs(8) {
            Some(_) => known(needs(8 + 16 * read_u16(bytes, 6)), true),
            None => Message::Incomplete,
        },
        255 => match bytes.get(1) {
            None => Message::Incomplete,
            Some(0) => known(needs(12), true),
            Some(1) => known(needs(4), false),
            Some(_) => Message::Unknown,
        },
        _ => Message::Unknown,
    }
}

impl Default for ClientFilter {
    fn default() -> ClientFilter {
        ClientFilter { pending: Vec::new(), handshake_remaining: HANDSHAKE_LENGTH, lost_sync: false }
    }
}

impl ClientFilter {
    pub fn pass(&mut self, received: &[u8], person_controls: bool) -> Vec<u8> {
        if self.lost_sync {
            return if person_controls { received.to_vec() } else { Vec::new() };
        }

        self.pending.extend_from_slice(received);

        let mut allowed = Vec::new();

        let handshake = self.handshake_remaining.min(self.pending.len());

        allowed.extend(self.pending.drain(..handshake));
        self.handshake_remaining -= handshake;

        loop {
            match next_message(&self.pending) {
                Message::Incomplete => break,
                Message::Unknown => {
                    self.lost_sync = true;

                    if person_controls {
                        allowed.append(&mut self.pending);
                    }

                    self.pending.clear();

                    break;
                }
                Message::Known { length, input } => {
                    let message: Vec<u8> = self.pending.drain(..length).collect();

                    if person_controls || !input {
                        allowed.extend(message);
                    }
                }
            }
        }

        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_EVENT: [u8; 8] = [4, 1, 0, 0, 0, 0, 0, 97];
    const POINTER_EVENT: [u8; 6] = [5, 1, 0, 10, 0, 10];
    const REFRESH_REQUEST: [u8; 10] = [3, 1, 0, 0, 0, 0, 0, 100, 0, 100];
    const CONTINUOUS_UPDATES_REQUEST: [u8; 10] = [150, 1, 0, 0, 0, 0, 0, 100, 0, 100];

    fn shaken() -> ClientFilter {
        let mut filter = ClientFilter::default();

        assert_eq!(filter.pass(&[0; HANDSHAKE_LENGTH], false).len(), HANDSHAKE_LENGTH);

        filter
    }

    #[test]
    fn the_handshake_is_forwarded_as_it_arrives_whoever_controls() {
        let mut filter = ClientFilter::default();

        assert_eq!(filter.pass(b"RFB 003.008\n", false), b"RFB 003.008\n".to_vec());
        assert_eq!(filter.pass(&[1], false), vec![1]);
        assert_eq!(filter.pass(&[1], false), vec![1]);
    }

    #[test]
    fn input_is_dropped_while_the_agent_controls_but_the_view_keeps_updating() {
        let mut filter = shaken();
        let batch = [REFRESH_REQUEST.to_vec(), KEY_EVENT.to_vec(), POINTER_EVENT.to_vec()].concat();

        assert_eq!(filter.pass(&batch, false), REFRESH_REQUEST.to_vec());
    }

    #[test]
    fn a_continuous_updates_request_keeps_the_view_updating_while_the_agent_controls() {
        let mut filter = shaken();
        let batch = [CONTINUOUS_UPDATES_REQUEST.to_vec(), KEY_EVENT.to_vec(), REFRESH_REQUEST.to_vec()].concat();

        assert_eq!(filter.pass(&batch, false), [CONTINUOUS_UPDATES_REQUEST.to_vec(), REFRESH_REQUEST.to_vec()].concat());
    }

    #[test]
    fn input_passes_while_the_person_controls() {
        let mut filter = shaken();

        assert_eq!(filter.pass(&KEY_EVENT, true), KEY_EVENT.to_vec());
        assert_eq!(filter.pass(&POINTER_EVENT, true), POINTER_EVENT.to_vec());
    }

    #[test]
    fn a_message_split_across_frames_is_judged_whole() {
        let mut filter = shaken();

        assert!(filter.pass(&KEY_EVENT[..3], false).is_empty());
        assert!(filter.pass(&KEY_EVENT[3..], false).is_empty());
        assert_eq!(filter.pass(&POINTER_EVENT, true), POINTER_EVENT.to_vec());
    }

    #[test]
    fn an_unrecognised_message_is_dropped_for_the_agent_and_forwarded_for_the_person() {
        let mut agent = shaken();
        let mut person = shaken();

        assert!(agent.pass(&[200, 1, 2], false).is_empty());
        assert!(agent.pass(&KEY_EVENT, false).is_empty());
        assert_eq!(person.pass(&[200, 1, 2], true), vec![200, 1, 2]);
    }
}
