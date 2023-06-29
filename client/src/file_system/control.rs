use std::path::PathBuf;
use std::sync::Arc;

use actix::{ActorFutureExt, AsyncContext, Handler, Message};
use futures::stream::FuturesUnordered;
use futures::StreamExt;

use super::actor::{FileDatabase, FileDatabaseData, FileDatabaseUpdater};

#[derive(Message)]
#[rtype(result = "()")]
pub struct FileDatabaseStartUpdate;

impl Handler<FileDatabaseStartUpdate> for FileDatabase {
    type Result = ();

    fn handle(&mut self, _: FileDatabaseStartUpdate, ctx: &mut Self::Context) -> Self::Result {
        if self.update.is_some() {
            return;
        }
        let update = actix::fut::wrap_future::<_, Self>(self.data.clone().start_update());
        let update = update.then(|_, _, ctx| {
            ctx.notify(FileDatabaseStopUpdate);
            actix::fut::ready(())
        });
        self.update = Some(ctx.spawn(update));
    }
}

impl FileDatabaseData {
    async fn start_update(self) {
        self.finished_dirs_reset();
        self.queued_dirs_reset();
        self.set_updating(true);
        let mut updates = FuturesUnordered::new();
        for p in self.paths.iter() {
            updates.push(FileDatabaseUpdater::new(p.to_path_buf(), self.clone()).complete());
        }
        while updates.next().await.is_some() {}
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct FileDatabaseStopUpdate;

impl Handler<FileDatabaseStopUpdate> for FileDatabase {
    type Result = ();

    fn handle(&mut self, _: FileDatabaseStopUpdate, ctx: &mut Self::Context) -> Self::Result {
        self.data.finished_dirs_reset();
        self.data.queued_dirs_reset();
        self.data.set_updating(false);
        // TODO move notify inside the data struct and notify on changes
        self.data.notify();
        if let Some(update) = self.update.take() {
            ctx.cancel_future(update);
        }
    }
}
