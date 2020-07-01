use rand::distributions::{Distribution, Uniform};

pub mod bytes_mut;
pub mod duration;
pub mod ipv4_addr;
pub mod ipv6_addr;

pub fn expovariate_rand() -> f64 {
    let range = Uniform::new(0.0, 1.0);
    let mut rng = rand::thread_rng();
    let offset = range.sample(&mut rng);
    -f64::ln(1.0 - offset)
}
