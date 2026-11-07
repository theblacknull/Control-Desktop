use agent_desktop_core::{ProcessId, ProcessIdentity};

pub(super) struct RetainedToken {
    pub(super) owner: Option<ProcessIdentity>,
    pub(super) bytes: Vec<u8>,
}

impl RetainedToken {
    pub(super) fn encode(owner: &ProcessIdentity, bytes: &[u8]) -> String {
        format!(
            "axtok2:{}:{}:{}",
            owner.pid,
            owner.instance,
            encode_hex(bytes)
        )
    }

    pub(super) fn decode(token: &str) -> Option<Self> {
        if let Some(hex) = token.strip_prefix("axtok1:") {
            return Some(Self {
                owner: None,
                bytes: decode_hex(hex)?,
            });
        }
        let body = token.strip_prefix("axtok2:")?;
        let (owner, hex) = body.rsplit_once(':')?;
        let (pid, instance) = owner.split_once(':')?;
        let pid = pid
            .parse::<u32>()
            .ok()
            .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)?;
        Some(Self {
            owner: Some(ProcessIdentity::new(ProcessId::new(pid), instance)),
            bytes: decode_hex(hex)?,
        })
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.is_empty() || !hex.len().is_multiple_of(2) || !hex.is_ascii() {
        return None;
    }
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).ok()?;
            u8::from_str_radix(text, 16).ok()
        })
        .collect()
}

#[cfg(test)]
#[path = "retained_token_tests.rs"]
mod tests;
