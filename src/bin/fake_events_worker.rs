use std::io::{self, Write};

fn main() {
    println!("fake_events_worker ready");
    let _ = io::stdout().flush();
    std::thread::sleep(std::time::Duration::from_secs(5));
}
