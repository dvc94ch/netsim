use super::Ipv4Node;
use crate::device::ipv4::Ipv4RouterBuilder;
use crate::network::NetworkHandle;
use crate::range::Ipv4Range;
use crate::route::Ipv4Route;
use crate::spawn_complete::SpawnComplete;
use crate::wire::Ipv4Plug;
use futures::channel::oneshot;
use futures::future::Future;
use futures::stream::{FuturesOrdered, TryStreamExt};
use std::any::Any;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// A set of clients that can be attached to a router node.
pub trait Ipv4RouterClients {
    /// The output of the nodes attached to the router.
    type Output: Send + 'static;

    /// Build the set of nodes.
    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Ipv4Range,
    ) -> (SpawnComplete<Self::Output>, Ipv4Plug);
}

struct JoinAll<X, T> {
    phantoms: PhantomData<X>,
    children: T,
}

macro_rules! tuple_impl {
    ($($ty:ident,)*) => {
        impl<$($ty),*> Ipv4RouterClients for ($($ty,)*)
        where
            $($ty: Ipv4Node + Unpin + Send + 'static,)*
        {
            type Output = ($($ty::Output,)*);

            fn build(self, handle: &NetworkHandle, ipv4_range: Ipv4Range) -> (SpawnComplete<Self::Output>, Ipv4Plug) {
                #![allow(non_snake_case)]
                #![allow(unused_assignments)]
                #![allow(unused_mut)]
                #![allow(unused_variables)]

                let ($($ty,)*) = self;

                let mut i = 0;
                $(
                    let $ty = $ty;
                    i += 1;
                )*
                let ranges = ipv4_range.split(i + 1);

                let router = Ipv4RouterBuilder::new(ranges[0].base_addr());
                let mut i = 1;
                $(
                    let ($ty, plug) = $ty.build(handle, ranges[i]);
                    let router = router.connect(plug, vec![Ipv4Route::new(ranges[i], None)]);
                    i += 1;
                )*

                let (plug_0, plug_1) = Ipv4Plug::new_pair();
                let router = router.connect(plug_1, vec![Ipv4Route::new(Ipv4Range::global(), None)]);
                router.spawn(handle);

                let (ret_tx, ret_rx) = oneshot::channel();
                handle.spawn(Box::pin(async move {
                    let res = JoinAll {
                        phantoms: PhantomData::<($($ty,)*)>,
                        children: ($(($ty, None),)*)
                    }.await;
                    let _ = ret_tx.send(res);
                }));

                let spawn_complete = SpawnComplete::from_receiver(ret_rx);

                (spawn_complete, plug_0)
            }
        }

        impl<$($ty),*> Future for JoinAll<($($ty,)*), ($((SpawnComplete<$ty::Output>, Option<$ty::Output>),)*)>
        where
            $($ty: Ipv4Node + Unpin + 'static,)*
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

impl<N> Ipv4RouterClients for Vec<N>
where
    N: Ipv4Node + Unpin + 'static,
{
    type Output = Vec<N::Output>;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Ipv4Range,
    ) -> (SpawnComplete<Vec<N::Output>>, Ipv4Plug) {
        let ranges = ipv4_range.split(self.len() as u32 + 1);
        let mut router = Ipv4RouterBuilder::new(ranges[0].base_addr());
        let mut spawn_completes = FuturesOrdered::new();

        for (i, node) in self.into_iter().enumerate() {
            let (spawn_complete, plug) = node.build(handle, ranges[i + 1]);
            router = router.connect(plug, vec![Ipv4Route::new(ranges[i + 1], None)]);
            spawn_completes.push(spawn_complete);
        }

        let (plug_0, plug_1) = Ipv4Plug::new_pair();
        let router = router.connect(plug_1, vec![Ipv4Route::new(Ipv4Range::global(), None)]);
        router.spawn(handle);

        let (tx, rx) = oneshot::channel();
        handle.spawn(Box::pin(async move {
            let res = spawn_completes.try_collect::<Vec<_>>().await;
            let _ = tx.send(res);
        }));

        let spawn_complete = SpawnComplete::from_receiver(rx);

        (spawn_complete, plug_0)
    }
}

/// A node representing an Ipv4 router
pub struct RouterNode<C> {
    clients: C,
}

/// Spawns a bunch of sub-nodes and routes packets between them.
pub fn router<C: Ipv4RouterClients>(clients: C) -> RouterNode<C> {
    RouterNode { clients }
}

impl<C> Ipv4Node for RouterNode<C>
where
    C: Ipv4RouterClients,
    C::Output: Unpin,
{
    type Output = C::Output;

    fn build(
        self,
        handle: &NetworkHandle,
        ipv4_range: Ipv4Range,
    ) -> (SpawnComplete<C::Output>, Ipv4Plug) {
        self.clients.build(handle, ipv4_range)
    }
}
