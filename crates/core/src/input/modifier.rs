use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Modifier {
    #[serde(alias = "Cmd")]
    Meta,
    Ctrl,
    Alt,
    Shift,
    LeftMeta,
    RightMeta,
    LeftAlt,
    RightAlt,
}

impl Modifier {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "meta" | "cmd" | "command" | "super" | "win" => Self::Meta,
            "ctrl" | "control" => Self::Ctrl,
            "alt" | "option" => Self::Alt,
            "shift" => Self::Shift,
            "lcmd" => Self::LeftMeta,
            "rcmd" => Self::RightMeta,
            "lopt" => Self::LeftAlt,
            "ropt" => Self::RightAlt,
            _ => return None,
        })
    }

    pub fn generic(&self) -> Self {
        match self {
            Self::LeftMeta | Self::RightMeta => Self::Meta,
            Self::LeftAlt | Self::RightAlt => Self::Alt,
            other => other.clone(),
        }
    }
}
