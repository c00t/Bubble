use bubble_tests::utils::mimic_game_tick;
use bubble_tests::TestSkeleton;
use rand_distr::Distribution;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn smoke_test() {
    let test = TestSkeleton::new();

    let tick_thread = test.create_tick_thread(mimic_game_tick, ());

    tick_thread.join().unwrap();

    test.end_test();
}
