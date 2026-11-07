use super::RetainedToken;
use agent_desktop_core::{ProcessId, ProcessIdentity};

#[test]
fn owner_and_native_bytes_roundtrip_without_using_the_host_pid() {
    let owner = ProcessIdentity::new(ProcessId::new(42), "macos-proc-v1:1700000000:12345");
    let encoded = RetainedToken::encode(&owner, &[0, 1, 127, 255]);
    assert!(agent_desktop_core::refs::ref_identity::is_element_token(
        &encoded
    ));
    let decoded = RetainedToken::decode(&encoded).unwrap();
    assert_eq!(decoded.owner, Some(owner));
    assert_eq!(decoded.bytes, [0, 1, 127, 255]);
}

#[test]
fn shipped_tokens_keep_the_host_process_check() {
    let decoded = RetainedToken::decode("axtok1:00abff").unwrap();
    assert_eq!(decoded.owner, None);
    assert_eq!(decoded.bytes, [0, 171, 255]);
}

#[test]
fn malformed_tokens_fail_closed_without_panicking_on_unicode() {
    for token in [
        "axtok1:",
        "axtok1:f",
        "axtok1:zz",
        "axtok1:é",
        "axtok2:",
        "axtok2:0:macos-proc-v1:1:2:00",
        "axtok2:4294967295:macos-proc-v1:1:2:00",
        "axtok2:no:macos-proc-v1:1:2:00",
        "axtok2:1:macos-proc-v1:1:2:é",
        "other:00",
    ] {
        assert!(RetainedToken::decode(token).is_none(), "{token}");
    }
}

#[test]
fn a_dead_or_reused_owner_cannot_resolve_a_token() {
    let pid = ProcessId::new(std::process::id());
    let owner = ProcessIdentity::new(pid, "macos-proc-v1:0:0");
    let encoded = RetainedToken::encode(&owner, &[0, 1]);
    assert!(matches!(
        super::super::retained::resolve_token(&encoded).unwrap(),
        super::super::retained::TokenLookup::Dead
    ));
}

#[test]
fn a_malformed_owner_instance_cannot_resolve_a_token() {
    let owner = ProcessIdentity::new(ProcessId::new(std::process::id()), "broken");
    let encoded = RetainedToken::encode(&owner, &[0, 1]);
    assert!(matches!(
        super::super::retained::resolve_token(&encoded).unwrap(),
        super::super::retained::TokenLookup::Dead
    ));
}
