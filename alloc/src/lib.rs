//! The global allocator of Mimicway: the system allocator, counting the bytes each thread holds, so that a rule
//! script can be stopped when its run holds more memory than its budget (`src/engine/script_budget.rs`).
//!
//! Rhai bounds each value of a script, not the sum of everything a run holds: values passed to one call, keys of a
//! map, results gathered by `map`, values held by nested calls. Counting those from the language would mean finding
//! every way a script can hold a value; counting what the thread allocates finds them all. A run is synchronous, on
//! the thread that started it, so what that thread holds more than when the run started is what the run holds.
//!
//! This is the only crate of Mimicway with `unsafe` code: implementing [`GlobalAlloc`] is unsafe by definition. Each
//! method hands its arguments unchanged to [`System`] and returns what it returned; around the call, it adds to or
//! subtracts from a counter of the calling thread. The tests run under Miri (`alloc/README.md`).
//!
//! # What the counter means
//! - The bytes allocated on this thread minus the bytes freed on it, as the code asks for them (not what the system
//!   allocator rounds them to). A block freed on another thread than the one that allocated it lowers the counter of
//!   the thread that frees it: the counter is signed, and only differences on one thread mean anything.
//! - `realloc` counts the difference between the new size and the old.
//! - An allocation never fails because of the count: a budget is read between the operations of a script, where a
//!   run can be stopped with an error; a failed allocation would abort the whole process.
//!
//! # The counter never allocates
//! It is a `thread_local!` initialized by a `const`, of a type without a destructor: reading it allocates nothing
//! (otherwise the allocator would call itself), registers no destructor, and stays readable while a thread ends, when
//! the destructors of other thread-locals still free memory.
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// The system allocator, counting the bytes each thread holds. Installed in the binary:
/// `#[global_allocator] static ALLOCATOR: mimicway_alloc::Counting = mimicway_alloc::Counting;`
#[derive(Debug, Clone, Copy, Default)]
pub struct Counting;

thread_local! {
    static LIVE: Cell<i64> = const { Cell::new(0) };
}

// Checked at compile time: a thread-local with a destructor would allocate its registration on first use.
const _: () = assert!(!std::mem::needs_drop::<Cell<i64>>());

// A `Layout` size never exceeds `isize::MAX`, which `i64` holds on every target.
#[inline(always)]
fn add(bytes: i64) {
    // `try_with` rather than `with`: an allocator must never panic, and a thread-local without a destructor is
    // never in the destroyed state that would make it fail.
    let _ = LIVE.try_with(|live| live.set(live.get().wrapping_add(bytes)));
}

// SAFETY: every method hands its arguments unchanged to the same method of `System`, the standard library's allocator,
// and returns what it returned: this type makes, offsets, keeps or frees no pointer of its own, so it upholds the
// contract of `GlobalAlloc` exactly as `System` does. Around each call it only changes a thread-local counter, which
// neither allocates nor unwinds.
unsafe impl GlobalAlloc for Counting {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's guarantee for `layout` (a non-zero size) is the one `System::alloc` asks for, and
        // `layout` is passed on unchanged.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add(layout.size() as i64);
        }
        ptr
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`; `System::alloc_zeroed` has the same contract.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            add(layout.size() as i64);
        }
        ptr
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees `ptr` was allocated by this allocator with `layout`; every block this
        // allocator returned came from `System` with that same layout, so `System` may free it.
        unsafe { System.dealloc(ptr, layout) };
        add(-(layout.size() as i64));
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as for `dealloc`, `ptr` and `layout` are a block `System` returned; the caller's guarantee for
        // `new_size` (non-zero, not past `isize::MAX` once rounded to the alignment) is the one `System::realloc`
        // asks for.
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        // A null result leaves the old block in place, counted as it was.
        if !new_ptr.is_null() {
            add(new_size as i64 - layout.size() as i64);
        }
        new_ptr
    }
}

/// The bytes this thread has allocated and not freed since it started, as [`Counting`] counts them; always `0` when
/// another allocator is installed. Signed: a thread that frees what another allocated goes below zero.
#[inline]
pub fn live_bytes() -> i64 {
    LIVE.try_with(Cell::get).unwrap_or(0)
}
