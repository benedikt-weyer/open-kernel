use openkernel_rt as _;

fn main() {
    let mut values = Vec::new();
    values.extend_from_slice(&[3_u64, 5, 8, 13]);

    println!("openkernel std smoke test");
    println!("vec_sum={}", values.iter().sum::<u64>());
}
