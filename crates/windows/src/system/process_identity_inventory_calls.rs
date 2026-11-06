use std::cell::{Cell, RefCell};

thread_local! {
    static IMAGE_TABLE_WALKS: Cell<usize> = const { Cell::new(0) };
    static SINGLE_IMAGE_WALKS: Cell<usize> = const { Cell::new(0) };
    static SINGLE_IMAGE_PIDS: RefCell<Vec<super::ProcessId>> = const { RefCell::new(Vec::new()) };
    static IDENTITY_CAPTURES: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_image_table_walk() {
    crate::system::fixture::call_counter::record(&IMAGE_TABLE_WALKS);
}

pub(super) fn record_single_image_walk(pid: super::ProcessId) {
    crate::system::fixture::call_counter::record(&SINGLE_IMAGE_WALKS);
    SINGLE_IMAGE_PIDS.with(|pids| pids.borrow_mut().push(pid));
}

pub(super) fn record_identity_capture() {
    crate::system::fixture::call_counter::record(&IDENTITY_CAPTURES);
}

pub(crate) fn take() -> (usize, usize) {
    (
        crate::system::fixture::call_counter::take(&IMAGE_TABLE_WALKS),
        crate::system::fixture::call_counter::take(&SINGLE_IMAGE_WALKS),
    )
}

pub(crate) fn take_single_image_pids() -> Vec<super::ProcessId> {
    SINGLE_IMAGE_PIDS.with(|pids| std::mem::take(&mut *pids.borrow_mut()))
}

pub(crate) fn take_identity_captures() -> usize {
    crate::system::fixture::call_counter::take(&IDENTITY_CAPTURES)
}
