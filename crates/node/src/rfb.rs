use anyhow::{Context, Result, bail};
use mycelium_proto::link::EnvironmentInputAction;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_GAP: Duration = Duration::from_millis(12);
const MAX_SCROLL_STEPS: u32 = 50;

const BUTTON_LEFT: u8 = 1;
const BUTTON_MIDDLE: u8 = 2;
const BUTTON_RIGHT: u8 = 4;
const WHEEL_UP: u8 = 8;
const WHEEL_DOWN: u8 = 16;
const WHEEL_LEFT: u8 = 32;
const WHEEL_RIGHT: u8 = 64;

const KEYSYM_RETURN: u32 = 0xff0d;
const KEYSYM_TAB: u32 = 0xff09;

pub fn pointer_mask(button: &str) -> Result<u8> {
    match button.to_ascii_lowercase().as_str() {
        "" | "left" => Ok(BUTTON_LEFT),
        "middle" => Ok(BUTTON_MIDDLE),
        "right" => Ok(BUTTON_RIGHT),
        other => bail!("{other} is not a mouse button. Use left, middle or right."),
    }
}

pub fn scroll_mask(direction: &str) -> Result<u8> {
    match direction.to_ascii_lowercase().as_str() {
        "up" => Ok(WHEEL_UP),
        "down" => Ok(WHEEL_DOWN),
        "left" => Ok(WHEEL_LEFT),
        "right" => Ok(WHEEL_RIGHT),
        other => bail!("{other} is not a scroll direction. Use up, down, left or right."),
    }
}

pub fn pointer_event(mask: u8, x: u32, y: u32) -> Vec<u8> {
    let x = x.min(u16::MAX as u32) as u16;
    let y = y.min(u16::MAX as u32) as u16;
    let mut message = vec![5, mask];
    message.extend_from_slice(&x.to_be_bytes());
    message.extend_from_slice(&y.to_be_bytes());

    message
}

pub fn key_event(down: bool, keysym: u32) -> Vec<u8> {
    let mut message = vec![4, u8::from(down), 0, 0];
    message.extend_from_slice(&keysym.to_be_bytes());

    message
}

pub fn keysym_for_char(character: char) -> Option<u32> {
    let code = character as u32;

    match character {
        '\n' | '\r' => Some(KEYSYM_RETURN),
        '\t' => Some(KEYSYM_TAB),
        _ if code < 0x20 || (0x7f..0xa0).contains(&code) => None,
        _ if code <= 0xff => Some(code),
        _ => Some(0x0100_0000 + code),
    }
}

fn modifier_keysym(name: &str) -> Option<u32> {
    match name {
        "ctrl" | "control" => Some(0xffe3),
        "shift" => Some(0xffe1),
        "alt" => Some(0xffe9),
        "super" | "meta" | "win" | "cmd" => Some(0xffeb),
        _ => None,
    }
}

fn named_key_keysym(name: &str) -> Option<u32> {
    match name {
        "enter" | "return" => Some(KEYSYM_RETURN),
        "tab" => Some(KEYSYM_TAB),
        "escape" | "esc" => Some(0xff1b),
        "backspace" => Some(0xff08),
        "delete" | "del" => Some(0xffff),
        "space" => Some(0x20),
        "home" => Some(0xff50),
        "left" => Some(0xff51),
        "up" => Some(0xff52),
        "right" => Some(0xff53),
        "down" => Some(0xff54),
        "pageup" | "prior" => Some(0xff55),
        "pagedown" | "next" => Some(0xff56),
        "end" => Some(0xff57),
        _ => function_key_keysym(name),
    }
}

fn function_key_keysym(name: &str) -> Option<u32> {
    let number: u32 = name.strip_prefix('f')?.parse().ok()?;

    (1..=12).contains(&number).then(|| 0xffbe + number - 1)
}

pub fn parse_chord(chord: &str) -> Result<Vec<u32>> {
    let parts: Vec<&str> = chord.split('+').collect();
    let Some((key, modifiers)) = parts.split_last() else {
        bail!("Name a key to press.");
    };
    let mut keysyms = Vec::new();

    for modifier in modifiers {
        let lowered = modifier.to_ascii_lowercase();
        let Some(keysym) = modifier_keysym(&lowered) else {
            bail!("{modifier} is not a modifier key. Use ctrl, shift, alt or super before the key.");
        };
        keysyms.push(keysym);
    }

    let lowered = key.to_ascii_lowercase();
    let mut characters = key.chars();
    let single = match (characters.next(), characters.next()) {
        (Some(only), None) => keysym_for_char(only),
        _ => None,
    };
    let Some(keysym) = single.or_else(|| named_key_keysym(&lowered)).or_else(|| modifier_keysym(&lowered)) else {
        bail!("{key} is not a key this environment knows. Use a single character or a name such as Enter, Tab, Escape, BackSpace, Left or F5.");
    };
    keysyms.push(keysym);

    Ok(keysyms)
}

pub fn parse_keys(keys: &str) -> Result<Vec<Vec<u32>>> {
    let chords = keys.split_whitespace().map(parse_chord).collect::<Result<Vec<_>>>()?;

    if chords.is_empty() {
        bail!("Name a key to press.");
    }

    Ok(chords)
}

pub fn chord_events(chord: &[u32]) -> Vec<Vec<u8>> {
    let presses = chord.iter().map(|keysym| key_event(true, *keysym));
    let releases = chord.iter().rev().map(|keysym| key_event(false, *keysym));

    presses.chain(releases).collect()
}

pub fn input_messages(action: &EnvironmentInputAction) -> Result<Vec<Vec<u8>>> {
    match action {
        EnvironmentInputAction::Click { x, y, button } => {
            let mask = pointer_mask(button.as_deref().unwrap_or("left"))?;

            Ok(vec![pointer_event(0, *x, *y), pointer_event(mask, *x, *y), pointer_event(0, *x, *y)])
        }
        EnvironmentInputAction::Type { text } => {
            let mut messages = Vec::new();

            for character in text.chars() {
                let Some(keysym) = keysym_for_char(character) else {
                    continue;
                };
                messages.extend(chord_events(&[keysym]));
            }

            Ok(messages)
        }
        EnvironmentInputAction::Scroll { direction, amount, x, y } => {
            let mask = scroll_mask(direction)?;
            let (x, y) = (x.unwrap_or(0), y.unwrap_or(0));
            let mut messages = Vec::new();

            if x != 0 || y != 0 {
                messages.push(pointer_event(0, x, y));
            }

            for _ in 0..(*amount).clamp(1, MAX_SCROLL_STEPS) {
                messages.push(pointer_event(mask, x, y));
                messages.push(pointer_event(0, x, y));
            }

            Ok(messages)
        }
        EnvironmentInputAction::Key { keys } => Ok(parse_keys(keys)?.iter().flat_map(|chord| chord_events(chord)).collect()),
    }
}

async fn handshake(stream: &mut TcpStream) -> Result<()> {
    let mut greeting = [0u8; 12];
    stream.read_exact(&mut greeting).await.context("The environment's screen server did not greet us")?;
    stream.write_all(b"RFB 003.008\n").await?;

    let count = stream.read_u8().await?;

    if count == 0 {
        bail!("The environment's screen server refused the connection.");
    }

    let mut kinds = vec![0u8; count as usize];
    stream.read_exact(&mut kinds).await?;

    if !kinds.contains(&1) {
        bail!("The environment's screen server asks for a password, which this node does not send.");
    }

    stream.write_u8(1).await?;
    let result = stream.read_u32().await?;

    if result != 0 {
        bail!("The environment's screen server rejected the connection.");
    }

    stream.write_u8(1).await?;
    let mut size = [0u8; 4];
    stream.read_exact(&mut size).await?;
    let mut format = [0u8; 16];
    stream.read_exact(&mut format).await?;
    let name_length = stream.read_u32().await?;
    let mut name = vec![0u8; name_length.min(4096) as usize];
    stream.read_exact(&mut name).await?;

    Ok(())
}

pub async fn send_input(port: u16, action: &EnvironmentInputAction) -> Result<()> {
    let messages = input_messages(action)?;
    let mut stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        handshake(&mut stream).await?;

        anyhow::Ok(stream)
    })
    .await
    .context("The environment's screen server took too long to answer")??;

    for message in messages {
        stream.write_all(&message).await?;
        tokio::time::sleep(EVENT_GAP).await;
    }

    stream.flush().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_one_characters_map_to_their_code_point() {
        assert_eq!(keysym_for_char('a'), Some(0x61));
        assert_eq!(keysym_for_char('A'), Some(0x41));
        assert_eq!(keysym_for_char('é'), Some(0xe9));
    }

    #[test]
    fn characters_beyond_latin_one_use_the_unicode_keysym_range() {
        assert_eq!(keysym_for_char('€'), Some(0x0100_20ac));
    }

    #[test]
    fn newline_and_tab_map_to_the_return_and_tab_keys() {
        assert_eq!(keysym_for_char('\n'), Some(0xff0d));
        assert_eq!(keysym_for_char('\t'), Some(0xff09));
    }

    #[test]
    fn control_characters_have_no_keysym() {
        assert_eq!(keysym_for_char('\u{7}'), None);
    }

    #[test]
    fn a_chord_presses_modifiers_first_and_releases_them_in_reverse() {
        let chord = parse_chord("ctrl+l").unwrap();

        assert_eq!(chord, vec![0xffe3, 0x6c]);
        assert_eq!(chord_events(&chord), vec![key_event(true, 0xffe3), key_event(true, 0x6c), key_event(false, 0x6c), key_event(false, 0xffe3)]);
    }

    #[test]
    fn named_keys_are_case_insensitive() {
        assert_eq!(parse_chord("alt+Left").unwrap(), vec![0xffe9, 0xff51]);
        assert_eq!(parse_chord("ENTER").unwrap(), vec![0xff0d]);
        assert_eq!(parse_chord("BackSpace").unwrap(), vec![0xff08]);
        assert_eq!(parse_chord("F5").unwrap(), vec![0xffc2]);
    }

    #[test]
    fn several_chords_separated_by_spaces_are_pressed_in_order() {
        assert_eq!(parse_keys("Tab Tab Enter").unwrap(), vec![vec![0xff09], vec![0xff09], vec![0xff0d]]);
    }

    #[test]
    fn an_unknown_key_is_explained() {
        let error = parse_chord("ctrl+Banana").unwrap_err().to_string();

        assert!(error.contains("Banana is not a key"));
    }

    #[test]
    fn an_unknown_modifier_is_explained() {
        let error = parse_chord("hyper+l").unwrap_err().to_string();

        assert!(error.contains("hyper is not a modifier"));
    }

    #[test]
    fn buttons_map_to_rfb_mask_bits() {
        assert_eq!(pointer_mask("left").unwrap(), 1);
        assert_eq!(pointer_mask("middle").unwrap(), 2);
        assert_eq!(pointer_mask("Right").unwrap(), 4);
        assert!(pointer_mask("fourth").is_err());
    }

    #[test]
    fn scroll_directions_map_to_wheel_bits() {
        assert_eq!(scroll_mask("up").unwrap(), 8);
        assert_eq!(scroll_mask("down").unwrap(), 16);
    }

    #[test]
    fn a_pointer_event_is_big_endian() {
        assert_eq!(pointer_event(1, 0x0102, 0x0304), vec![5, 1, 1, 2, 3, 4]);
    }

    #[test]
    fn a_key_event_carries_the_keysym_big_endian() {
        assert_eq!(key_event(true, 0xff0d), vec![4, 1, 0, 0, 0, 0, 0xff, 0x0d]);
    }

    #[test]
    fn a_click_moves_then_presses_then_releases() {
        let action = EnvironmentInputAction::Click { x: 10, y: 20, button: Some("right".into()) };

        assert_eq!(input_messages(&action).unwrap(), vec![pointer_event(0, 10, 20), pointer_event(4, 10, 20), pointer_event(0, 10, 20)]);
    }

    #[test]
    fn scrolling_repeats_the_wheel_button_amount_times() {
        let action = EnvironmentInputAction::Scroll { direction: "down".into(), amount: 3, x: Some(5), y: Some(6) };
        let messages = input_messages(&action).unwrap();

        assert_eq!(messages.len(), 1 + 3 * 2);
        assert_eq!(messages[1], pointer_event(16, 5, 6));
    }

    #[test]
    fn typing_presses_and_releases_each_character() {
        let action = EnvironmentInputAction::Type { text: "hi".into() };

        assert_eq!(input_messages(&action).unwrap(), vec![key_event(true, 0x68), key_event(false, 0x68), key_event(true, 0x69), key_event(false, 0x69)]);
    }
}
