//! The counter under the allocator installed for real, as in the binary. Each test measures one thread around one
//! allocation: libtest runs tests on threads of their own, so another test never moves this thread's counter.
use mimicway_alloc::{Counting, live_bytes};
use std::hint::black_box;

#[global_allocator]
static ALLOCATOR: Counting = Counting;

#[test]
fn an_allocation_counts_its_size_until_it_is_freed() {
    let before = live_bytes();
    let block = black_box(Vec::<u8>::with_capacity(1000));
    assert_eq!(live_bytes() - before, 1000);
    drop(block);
    assert_eq!(live_bytes(), before);
}

#[test]
fn a_zeroed_allocation_counts_its_size() {
    let before = live_bytes();
    let block = black_box(vec![0u8; 4096]);
    assert_eq!(live_bytes() - before, 4096);
    drop(block);
    assert_eq!(live_bytes(), before);
}

#[test]
fn a_reallocation_counts_the_difference() {
    let before = live_bytes();
    let mut block = black_box(Vec::<u8>::with_capacity(100));
    // An empty vector: reserving 1,000 grows the block of 100 to 1,000 in one reallocation.
    block.reserve_exact(1000);
    assert_eq!(block.capacity(), 1000);
    assert_eq!(live_bytes() - before, 1000);
    block.shrink_to(10);
    assert_eq!(live_bytes() - before, block.capacity() as i64);
    drop(block);
    assert_eq!(live_bytes(), before);
}

#[test]
fn another_thread_counts_on_its_own() {
    let held_there = std::thread::spawn(|| {
        let start = live_bytes();
        let block = black_box(Vec::<u8>::with_capacity(5000));
        (live_bytes() - start, block)
    })
    .join()
    .unwrap();
    assert_eq!(held_there.0, 5000);
    // The block came back to this thread: freeing it here lowers this thread's counter, never raises it.
    let after_join = live_bytes();
    drop(held_there.1);
    assert_eq!(live_bytes() - after_join, -5000);
}
