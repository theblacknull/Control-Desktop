#[cfg(target_os = "macos")]
mod imp {
    use crate::tree::AXElement;
    use crate::tree::properties::ax_ipc::{self, AxDeadline};
    use crate::tree::properties::cf_type::{
        borrowed_cf_number, borrowed_cf_string, created_cf_array,
    };
    use accessibility_sys::kAXErrorSuccess;
    use core_foundation::{array::CFArray, base::TCFType, string::CFString};

    const MENU_ITEM_CMD_CHAR: &str = "AXMenuItemCmdChar";
    const MENU_ITEM_CMD_MODIFIERS: &str = "AXMenuItemCmdModifiers";
    const MENU_ITEM_CMD_VIRTUAL_KEY: &str = "AXMenuItemCmdVirtualKey";

    const MODIFIER_SHIFT: i64 = 1 << 0;
    const MODIFIER_OPTION: i64 = 1 << 1;
    const MODIFIER_CONTROL: i64 = 1 << 2;
    const MODIFIER_NO_COMMAND: i64 = 1 << 3;

    /// Reads a menu item's bound key chord from the three `AXMenuItemCmd*`
    /// attributes in one batched accessibility call.
    ///
    /// A failed or partial read contributes nothing: the caller keeps the node
    /// and simply omits `shortcut`. Reading is one round trip regardless of how
    /// many of the three attributes resolve.
    pub(crate) fn read_menu_shortcut(
        element: &AXElement,
        deadline: impl AxDeadline,
    ) -> Option<String> {
        let names = [
            CFString::new(MENU_ITEM_CMD_CHAR),
            CFString::new(MENU_ITEM_CMD_MODIFIERS),
            CFString::new(MENU_ITEM_CMD_VIRTUAL_KEY),
        ];
        let refs = names
            .iter()
            .map(|name| name.as_concrete_TypeRef())
            .collect::<Vec<_>>();
        let array = CFArray::from_copyable(&refs);
        let (error, values) =
            ax_ipc::copy_multiple_attribute_values(element, array.as_concrete_TypeRef(), deadline);
        if error != kAXErrorSuccess {
            return None;
        }
        let values = created_cf_array(values)?;
        let mut items = values.into_iter();
        let char = items
            .next()
            .and_then(|item| borrowed_cf_string(item.as_concrete_TypeRef()))
            .map(|value| value.to_string())
            .unwrap_or_default();
        let modifiers = items
            .next()
            .and_then(|item| borrowed_cf_number(item.as_concrete_TypeRef()))
            .and_then(|value| value.to_i64());
        let virtual_key = items
            .next()
            .and_then(|item| borrowed_cf_number(item.as_concrete_TypeRef()))
            .and_then(|value| value.to_i64());
        format_shortcut(&char, modifiers, virtual_key)
    }

    /// Formats the raw `AXMenuItemCmd*` values into the `+`-joined chord the
    /// `press` command accepts, or `None` when nothing is bound.
    ///
    /// The modifier word order is `ctrl`, `alt`, `shift`, then `cmd`, so the
    /// string reads the same way a person says the chord. Option is spelled
    /// `alt` because that is the name `press` parses; AppKit's own `opt` is
    /// rejected as an unknown modifier. Bit `1 << 3` means "no command key";
    /// without it `cmd` is implicit, which is how macOS reports the common
    /// case of a command-only shortcut.
    pub(crate) fn format_shortcut(
        char: &str,
        modifiers: Option<i64>,
        virtual_key: Option<i64>,
    ) -> Option<String> {
        if char.is_empty() && virtual_key.is_none() {
            return None;
        }
        let modifiers = modifiers.unwrap_or(0);
        let mut parts: Vec<String> = Vec::new();
        if modifiers & MODIFIER_CONTROL != 0 {
            parts.push("ctrl".to_owned());
        }
        if modifiers & MODIFIER_OPTION != 0 {
            parts.push("alt".to_owned());
        }
        if modifiers & MODIFIER_SHIFT != 0 {
            parts.push("shift".to_owned());
        }
        if modifiers & MODIFIER_NO_COMMAND == 0 {
            parts.push("cmd".to_owned());
        }
        let key = match virtual_key.and_then(virtual_key_name) {
            Some(name) => name.to_owned(),
            None if !char.is_empty() => char.to_lowercase(),
            None => return None,
        };
        parts.push(key);
        Some(parts.join("+"))
    }

    /// The readable name of a macOS virtual key code, matching the vocabulary
    /// `press` parses back, so a discovered shortcut is always re-executable.
    pub(crate) fn virtual_key_name(code: i64) -> Option<&'static str> {
        let name = match code {
            0 => "a",
            1 => "s",
            2 => "d",
            3 => "f",
            4 => "h",
            5 => "g",
            6 => "z",
            7 => "x",
            8 => "c",
            9 => "v",
            11 => "b",
            12 => "q",
            13 => "w",
            14 => "e",
            15 => "r",
            16 => "y",
            17 => "t",
            18 => "1",
            19 => "2",
            20 => "3",
            21 => "4",
            22 => "6",
            23 => "5",
            24 => "=",
            25 => "9",
            26 => "7",
            27 => "-",
            28 => "8",
            29 => "0",
            30 => "]",
            31 => "o",
            32 => "u",
            33 => "[",
            34 => "i",
            35 => "p",
            36 => "return",
            37 => "l",
            38 => "j",
            39 => "'",
            40 => "k",
            41 => ";",
            42 => "\\",
            43 => ",",
            44 => "/",
            45 => "n",
            46 => "m",
            47 => ".",
            48 => "tab",
            49 => "space",
            50 => "`",
            51 => "delete",
            53 => "escape",
            96 => "f5",
            97 => "f6",
            98 => "f7",
            99 => "f3",
            100 => "f8",
            101 => "f9",
            103 => "f11",
            105 => "f13",
            107 => "f14",
            109 => "f10",
            111 => "f12",
            113 => "f15",
            115 => "home",
            116 => "pageup",
            117 => "forwarddelete",
            118 => "f4",
            119 => "end",
            120 => "f2",
            121 => "pagedown",
            122 => "f1",
            123 => "left",
            124 => "right",
            125 => "down",
            126 => "up",
            _ => return None,
        };
        Some(name)
    }
}

#[cfg(target_os = "macos")]
pub(crate) use imp::read_menu_shortcut;

#[cfg(all(test, target_os = "macos"))]
#[path = "menu_shortcut_tests.rs"]
mod tests;
