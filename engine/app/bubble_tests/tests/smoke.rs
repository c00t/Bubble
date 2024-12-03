use bubble_tests::TestSkeleton;
use rand_distr::Distribution;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[test]
fn smoke_test() {
    let test = TestSkeleton::new();

    let tick_thread = test.create_tick_thread(tick);

    tick_thread.join().unwrap();

    test.end_test();
}

static COUNTER: AtomicI32 = AtomicI32::new(0);

async fn long_running_async_task(idx: i32) -> String {
    let mut rng = rand::thread_rng();
    let normal = rand_distr::Normal::new(16.0, 10.0).unwrap();
    let x = normal.sample(&mut rng);
    let x = (x as u64).clamp(8, 100);
    println!("in tick {idx}, wait {x} ms");
    bubble_tasks::runtime::time::sleep(Duration::from_millis(x)).await;
    "Long Running Async Task".to_string()
}

pub async fn tick() -> bool {
    let counter = COUNTER.fetch_add(1, Ordering::SeqCst);
    long_running_async_task(counter).await;

    if counter == 200 {
        println!("in tick: {}, return false", counter);
        false
    } else {
        println!("in tick: {}, return true", counter);
        true
    }
}
