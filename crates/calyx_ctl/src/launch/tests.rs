use super::*;

#[cfg(windows)]
#[test]
fn launch_stops_its_standard_handles_from_being_inherited() {
    // Make them inheritable first, as a pipe from the caller may be.
    for handle in win32::std_handles() {
        // SAFETY: `handle` is one of this process's standard handles.
        unsafe {
            win32::SetHandleInformation(
                handle,
                win32::HANDLE_FLAG_INHERIT,
                win32::HANDLE_FLAG_INHERIT,
            )
        };
    }
    stop_std_handle_inheritance();
    let mut checked = 0;
    for handle in win32::std_handles() {
        let mut flags = 0;
        // SAFETY: `handle` is valid and `flags` is a live u32.
        if unsafe { win32::GetHandleInformation(handle, &mut flags) } != 0 {
            assert_eq!(flags & win32::HANDLE_FLAG_INHERIT, 0);
            checked += 1;
        }
    }
    assert!(checked > 0, "the test process has standard handles");
}
