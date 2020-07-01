use crate::network::NetworkHandle;
use crate::plug::Plug;
use crate::util;
use async_timer::timer::PosixTimer as Timer;
use futures::future::Future;
use futures::stream::{FuturesUnordered, Stream};
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

struct InTransit<T: Unpin> {
    packet: Option<T>,
    timeout: Timer,
}

impl<T: Unpin> Future for InTransit<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        match Pin::new(&mut self.timeout).poll(cx) {
            Poll::Ready(()) => Poll::Ready(unwrap!(self.packet.take())),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Links two `Ipv4Plug`s and adds delay to packets travelling between them.
pub struct Latency<T: Unpin + fmt::Debug + 'static> {
    plug_a: Plug<T>,
    plug_b: Plug<T>,
    outgoing_a: FuturesUnordered<InTransit<T>>,
    outgoing_b: FuturesUnordered<InTransit<T>>,
    min_latency: Duration,
    mean_additional_latency: Duration,
}

impl<T: Unpin + fmt::Debug + Send + 'static> Latency<T> {
    pub fn spawn(
        handle: &NetworkHandle,
        min_latency: Duration,
        mean_additional_latency: Duration,
        plug_a: Plug<T>,
        plug_b: Plug<T>,
    ) {
        let latency = Self {
            plug_a,
            plug_b,
            outgoing_a: FuturesUnordered::new(),
            outgoing_b: FuturesUnordered::new(),
            min_latency,
            mean_additional_latency,
        };
        handle.spawn(latency);
    }
}

impl<T: Unpin + fmt::Debug + 'static> Future for Latency<T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let a_unplugged = loop {
            match Pin::new(&mut self.plug_a.rx).poll_next(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(packet)) => {
                    let delay = self.min_latency
                        + self
                            .mean_additional_latency
                            .mul_f64(util::expovariate_rand());
                    let in_transit = InTransit {
                        packet: Some(packet),
                        timeout: Timer::new(delay),
                    };
                    self.outgoing_b.push(in_transit);
                }
            }
        };

        let b_unplugged = loop {
            match Pin::new(&mut self.plug_b.rx).poll_next(cx) {
                Poll::Pending => break false,
                Poll::Ready(None) => break true,
                Poll::Ready(Some(packet)) => {
                    let delay = self.min_latency
                        + self
                            .mean_additional_latency
                            .mul_f64(util::expovariate_rand());
                    let in_transit = InTransit {
                        packet: Some(packet),
                        timeout: Timer::new(delay),
                    };
                    self.outgoing_a.push(in_transit);
                }
            }
        };

        loop {
            match Pin::new(&mut self.outgoing_a).poll_next(cx) {
                Poll::Pending => break,
                Poll::Ready(None) => break,
                Poll::Ready(Some(packet)) => {
                    let _ = self.plug_a.tx.unbounded_send(packet);
                }
            }
        }

        loop {
            match Pin::new(&mut self.outgoing_b).poll_next(cx) {
                Poll::Pending => break,
                Poll::Ready(None) => break,
                Poll::Ready(Some(packet)) => {
                    let _ = self.plug_b.tx.unbounded_send(packet);
                }
            }
        }

        if a_unplugged && b_unplugged {
            return Poll::Ready(());
        }

        Poll::Pending
    }
}

/*
   TODO: fix the math in this test

#[cfg(test)]
#[test]
fn test() {
    run_test(|| {
        use rand;

        const NUM_PACKETS: u64 = 1000;

        let mut core = unwrap!(Core::new());
        let handle = core.handle();

        let source_addr = SocketAddrV4::new(
            Ipv4Addr::random_global(),
            rand::random::<u16>() / 2 + 1000,
        );
        let dest_addr = SocketAddrV4::new(
            Ipv4Addr::random_global(),
            rand::random::<u16>() / 2 + 1000,
        );
        let packet = Ipv4Packet::new_from_fields_recursive(
            Ipv4Fields {
                source_ip: *source_addr.ip(),
                dest_ip: *dest_addr.ip(),
                ttl: 16,
            },
            Ipv4PayloadFields::Udp {
                fields: UdpFields::V4 {
                    source_addr: source_addr,
                    dest_addr: dest_addr,
                },
                payload: Bytes::from(&rand::random::<[u8; 8]>()[..]),
            },
        );

        let min_latency = Duration::from_millis(100).mul_f64(util::expovariate_rand());
        let mean_additional_latency = Duration::from_millis(100).mul_f64(util::expovariate_rand());

        let (plug_a, plug_a_pass) = Ipv4Plug::new_pair();
        let (plug_b, plug_b_pass) = Ipv4Plug::new_pair();
        Ipv4Latency::spawn(&handle, min_latency, mean_additional_latency, plug_a_pass, plug_b_pass);

        let res = core.run({
            let start_time_0 = Instant::now();
            for _ in 0..NUM_PACKETS {
                let _ = plug_a.tx.unbounded_send(packet.clone());
            }
            let start_time_1 = Instant::now();
            let start_time = start_time_0 + (start_time_1 - start_time_0) / 2;

            plug_b.rx
            .take(NUM_PACKETS)
            .map(move |_packet| {
                let delay = Instant::now() - start_time;
                assert!(delay >= min_latency);
                let additional_delay = delay - min_latency;
                additional_delay.div_to_f64(mean_additional_latency)
            })
            .collect()
            .map(move |samples| {
                // let this test fail one in a million times due to randomness
                const CHANCE_OF_FAILURE: f64 = 1e-6f64;

                // inverse of the normal distribution cumulative probability function
                fn quantile(mean: f64, variance: f64, p: f64) -> f64 {
                    use statrs::function::erf::erf_inv;

                    mean + f64::sqrt(2.0 * variance) * erf_inv(2.0 * p - 1.0)
                }

                // see: https://en.wikipedia.org/wiki/Exponential_distribution#Confidence_intervals
                // a chi-squared(k) distribution can be approximated by normal distribution with mean k
                // and variance 2 * k
                let lower_chi_squared = quantile(
                    NUM_PACKETS as f64,
                    (2 * NUM_PACKETS) as f64,
                    CHANCE_OF_FAILURE / 2.0,
                );
                let upper_chi_squared = quantile(
                    NUM_PACKETS as f64,
                    (2 * NUM_PACKETS) as f64,
                    1.0 - CHANCE_OF_FAILURE / 2.0,
                );

                let mean = samples.into_iter().sum::<f64>() / (NUM_PACKETS as f64);

                assert!(2.0 * NUM_PACKETS as f64 * mean / lower_chi_squared < 1.0);
                assert!(2.0 * NUM_PACKETS as f64 * mean / upper_chi_squared > 1.0);
            })
        });
        res.void_unwrap()
    })
}
*/
