use crate::tree::AXElement;
use agent_desktop_core::{AdapterError, IdentifierEvidence};
use core_foundation::{
    base::TCFType,
    data::{CFData, CFDataRef},
};
use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

use super::retained_token::RetainedToken;
use crate::system::process::process_identity::ProcessIdentity;

const RTLD_DEFAULT: *mut c_void = -2_isize as *mut c_void;
const RTLD_LAZY: i32 = 0x1;
const APPLICATION_SERVICES: &CStr =
    c"/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices";

type TokenCreate = unsafe extern "C" fn(accessibility_sys::AXUIElementRef) -> CFDataRef;
type TokenResolve = unsafe extern "C" fn(CFDataRef) -> accessibility_sys::AXUIElementRef;

/// What a saved remote token says about the element it names in this process.
pub(crate) enum TokenLookup {
    NotAToken,
    Unavailable,
    Dead,
    Live(AXElement, Option<ProcessIdentity>),
}

struct Symbols {
    create: TokenCreate,
    resolve: TokenResolve,
}

/// Saves the element token with its owning process instance. A hosted control
/// can belong to a different process from the app whose tree contains it.
pub(crate) fn capture(
    element: &AXElement,
    deadline: std::time::Instant,
) -> Result<Option<String>, AdapterError> {
    if element.0.is_null() {
        return Err(AdapterError::stale_ref_because(
            "Cannot take a native object token from a null element",
        ));
    }
    let Some(symbols) = symbols() else {
        return Ok(None);
    };
    let pid = crate::tree::properties::ax_ipc::pid(element, deadline)
        .map_err(|error| crate::tree::surface_read::map_error("AXPid", error))?;
    let Some(owner) = ProcessIdentity::capture(pid)? else {
        return Ok(None);
    };
    let data = unsafe { (symbols.create)(element.0) };
    if data.is_null() {
        return Ok(None);
    }
    let data = unsafe { CFData::wrap_under_create_rule(data) };
    if !owner.still_matches()? {
        return Ok(None);
    }
    let owner = agent_desktop_core::ProcessIdentity::new(
        crate::system::process::process_identity::from_pid_t(pid)?,
        owner.token(),
    );
    Ok(Some(RetainedToken::encode(&owner, data.bytes())))
}

/// Resolves a saved token to the element it names, or says why it could not.
pub(crate) fn resolve_token(token: &str) -> Result<TokenLookup, AdapterError> {
    if !agent_desktop_core::refs::ref_identity::is_element_token(token) {
        return Ok(TokenLookup::NotAToken);
    }
    let Some(symbols) = symbols() else {
        return Ok(TokenLookup::Unavailable);
    };
    let Some(token) = RetainedToken::decode(token) else {
        return Ok(TokenLookup::Dead);
    };
    let owner = match token.owner {
        Some(owner) => {
            let pid = crate::system::process::process_identity::to_pid_t(owner.pid)?;
            match ProcessIdentity::capture(pid)?.filter(|current| current.token() == owner.instance)
            {
                Some(current) => Some(current),
                None => return Ok(TokenLookup::Dead),
            }
        }
        None => None,
    };
    let data = CFData::from_buffer(&token.bytes);
    let element = unsafe { (symbols.resolve)(data.as_concrete_TypeRef()) };
    if element.is_null() {
        return Ok(TokenLookup::Dead);
    }
    Ok(TokenLookup::Live(AXElement(element), owner))
}

pub(crate) fn evidence(
    element: &AXElement,
    identifiers: IdentifierEvidence,
    deadline: std::time::Instant,
) -> IdentifierEvidence {
    match capture(element, deadline) {
        Ok(Some(token)) => identifiers.with_retained_object(token),
        _ => identifiers,
    }
}

/// Whether this macOS build can create and resolve element tokens.
pub(crate) fn tokens_available() -> bool {
    symbols().is_some()
}

fn symbols() -> Option<&'static Symbols> {
    static SYMBOLS: OnceLock<Option<Symbols>> = OnceLock::new();
    SYMBOLS.get_or_init(load_symbols).as_ref()
}

fn load_symbols() -> Option<Symbols> {
    lookup_symbols(RTLD_DEFAULT).or_else(|| {
        let handle = unsafe { dlopen(APPLICATION_SERVICES.as_ptr(), RTLD_LAZY) };
        if handle.is_null() {
            None
        } else {
            lookup_symbols(handle)
        }
    })
}

fn lookup_symbols(handle: *mut c_void) -> Option<Symbols> {
    let create = symbol(handle, c"_AXUIElementRemoteTokenCreate")?;
    let resolve = symbol(handle, c"_AXUIElementCreateWithRemoteToken")?;
    Some(Symbols {
        create: unsafe { std::mem::transmute::<*mut c_void, TokenCreate>(create) },
        resolve: unsafe { std::mem::transmute::<*mut c_void, TokenResolve>(resolve) },
    })
}

fn symbol(handle: *mut c_void, name: &CStr) -> Option<*mut c_void> {
    let symbol = unsafe { dlsym(handle, name.as_ptr()) };
    (!symbol.is_null()).then_some(symbol)
}

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut c_void;
}
