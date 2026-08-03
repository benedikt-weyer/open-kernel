#![no_main]

use openkernel_rt as _;

#[unsafe(no_mangle)]
pub extern "C" fn openkernel_main() {
    let mut values = Vec::new();
    values.extend_from_slice(&[3_u64, 5, 8, 13]);

    println!("openkernel std smoke test");
    println!("vec_sum={}", values.iter().sum::<u64>());
}
