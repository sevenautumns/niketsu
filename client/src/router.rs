use std::any::{Any, TypeId};
use std::sync::Arc;

use actix::{Message, Recipient};
use dashmap::DashMap;
use log::error;

#[derive(Debug, Clone, Default)]
pub struct Router {
    map: Arc<DashMap<TypeId, Vec<Box<dyn Any>>>>,
}

impl Router {
    pub fn subscribe<M: Message + Clone + Send + 'static>(&self, sub: Recipient<M>)
    where
        <M as Message>::Result: Send,
    {
        let typ = std::any::TypeId::of::<M>();
        let mut subscriber = self.map.entry(typ).or_insert_with(Default::default);
        subscriber.push(Box::new(sub))
    }

    pub fn do_send<M: Message + Send + Clone + 'static>(&self, msg: M)
    where
        <M as Message>::Result: Send,
    {
        let typ = std::any::TypeId::of::<M>();
        let Some(subscriber) = self.map.get(&typ) else {
            return;
        };
        for sub in subscriber.iter() {
            let Some(recipient) = sub.downcast_ref::<Box<Recipient<M>>>() else {
                error!("unexpected subscriber in router");
                return;
            };
            recipient.do_send(msg.clone());
        }
    }
}
