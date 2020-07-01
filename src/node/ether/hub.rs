use crate::device::ether::HubBuilder;
use crate::network::NetworkHandle;
use crate::node::ether::EtherNode;
use crate::range::{Ipv4Range, Ipv6Range};
use crate::spawn_complete::SpawnComplete;
use crate::wire::EtherPlug;
use futures::channel::oneshot;
use futures::future::Future;
use std::any::Any;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A set of clients that can be attached to a hub node.
pub trait HubClients {
    /// The output of the nodes attached to the hub.
    type Output: Send + 'static;

    /// Build the set of nodes.
    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Option<Ipv4Range>,
        ipv6_range: Option<Ipv6Range>,
    ) -> (SpawnComplete<Self::Output>, EtherPlug);
}

struct JoinAll<X, T> {
    phantoms: PhantomData<X>,
    children: T,
}

macro_rules! tuple_impl {
    ($($ty:ident,)*) => {
        impl<$($ty),*> HubClients for ($($ty,)*)
        where
            $($ty: EtherNode + Unpin + Send + 'static,)*
        {
            type Output = ($($ty::Output,)*);

            fn build(
                self,
                handle: &NetworkHandle,
                ipv4_range: Option<Ipv4Range>,
                ipv6_range: Option<Ipv6Range>,
            ) -> (SpawnComplete<Self::Output>, EtherPlug)
            {
                #![allow(non_snake_case)]
                #![allow(unused_assignments)]
                #![allow(unused_mut)]
                #![allow(unused_variables)]

                let ($($ty,)*) = self;
                let hub = HubBuilder::new();
                let (hub, join_all) = {
                    let mut i = 0;
                    $(
                        let $ty = $ty;
                        i += 1;
                    )*
                    let ipv4_ranges = ipv4_range.map(|range| range.split(i));
                    let ipv6_ranges = ipv6_range.map(|range| range.split(i));
                    let mut i = 0;
                    $(
                        let ($ty, plug) = $ty.build(
                            handle,
                            ipv4_ranges.as_ref().map(|v| v[i]),
                            ipv6_ranges.as_ref().map(|v| v[i]),
                        );
                        let hub = hub.connect(plug);
                        i += 1;
                    )*
                    let join_all = JoinAll { phantoms: PhantomData::<($($ty,)*)>, children: ($(($ty, None),)*) };
                    (hub, join_all)
                };

                let (plug_0, plug_1) = EtherPlug::new_pair();
                let hub = hub.connect(plug_1);
                hub.spawn(handle);

                let (ret_tx, ret_rx) = oneshot::channel();
                handle.spawn(Box::pin(async move {
                    let result = join_all.await;
                    let _ = ret_tx.send(result);
                }));

                let spawn_complete = SpawnComplete::from_receiver(ret_rx);

                (spawn_complete, plug_0)
            }
        }

        impl<$($ty),*> Future for JoinAll<($($ty,)*), ($((SpawnComplete<$ty::Output>, Option<$ty::Output>),)*)>
        where
            $($ty: EtherNode + Unpin + 'static,)*
        {
            type Output = Result<($($ty::Output,)*), Box<dyn Any + Send + 'static>>;

            fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
                #![allow(non_snake_case)]

                let ($(ref mut $ty,)*) = self.children;
                $({
                    let (ref mut spawn_complete, ref mut result) = *$ty;
                    if result.is_none() {
                        match Pin::new(spawn_complete).poll(cx) {
                            Poll::Ready(Ok(val)) => {
                                *result = Some(val);
                            }
                            Poll::Pending => {
                                return Poll::Pending;
                            }
                            Poll::Ready(Err(err)) => {
                                return Poll::Ready(Err(err))
                            }
                        }
                    }
                })*

                $(
                    let (_, ref mut result) = *$ty;
                    let $ty = unwrap!(result.take());
                )*

                Poll::Ready(Ok(($($ty,)*)))
            }
        }
    }
}

tuple_impl!(T0,);
tuple_impl!(T0, T1,);
tuple_impl!(T0, T1, T2,);
tuple_impl!(T0, T1, T2, T3,);
tuple_impl!(T0, T1, T2, T3, T4,);
tuple_impl!(T0, T1, T2, T3, T4, T5,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14,);
tuple_impl!(T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15,);

/// A `Node` representing an ethernet hub.
pub struct HubNode<C> {
    clients: C,
}

/// Create a node for an ethernet hub.
pub fn hub<C: HubClients>(clients: C) -> HubNode<C> {
    HubNode { clients }
}

impl<C> EtherNode for HubNode<C>
where
    C: HubClients,
    C::Output: Unpin,
{
    type Output = C::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Option<Ipv4Range>,
        ipv6_range: Option<Ipv6Range>,
    ) -> (SpawnComplete<C::Output>, EtherPlug) {
        self.clients.build(handle, ipv4_range, ipv6_range)
    }
}
