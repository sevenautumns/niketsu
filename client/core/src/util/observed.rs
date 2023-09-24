use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::Notify;

#[derive(Debug, Clone)]
pub struct Observed<T> {
    inner: Arc<ArcSwap<T>>,
    changed: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl<T> Observed<T> {
    pub fn new(value: T, notify: &Arc<Notify>) -> Self {
        Self {
            inner: Arc::new(ArcSwap::new(Arc::new(value))),
            changed: Default::default(),
            notify: notify.clone(),
        }
    }

    pub fn get_inner_arc(&self) -> Arc<T> {
        self.inner.load_full()
    }

    pub fn changed(&self) -> bool {
        if self.changed.load(Ordering::Relaxed) {
            self.changed.store(false, Ordering::Relaxed);
            return true;
        }
        false
    }

    pub fn set(&self, value: T) {
        self.inner.store(Arc::new(value));
        self.changed.store(true, Ordering::Relaxed);
        self.notify.notify_one();
    }

    pub fn rcu<R, F>(&self, mut f: F)
    where
        F: FnMut(&T) -> R,
        R: Into<T>,
        Arc<T>: From<R>,
    {
        self.inner.rcu(|inner| f(inner));
        self.changed.store(true, Ordering::Relaxed);
        self.notify.notify_one();
    }

    pub fn on_change_arc<F>(&mut self, f: F)
    where
        F: FnOnce(Arc<T>),
    {
        if self.changed() {
            f(self.get_inner_arc());
        }
    }
}

impl<T: Default> Observed<T> {
    pub fn default_with_notify(notify: &Arc<Notify>) -> Self {
        Self {
            inner: Default::default(),
            changed: Default::default(),
            notify: notify.clone(),
        }
    }
}

impl<T: Clone> Observed<T> {
    pub fn get_inner(&self) -> T {
        (*self.inner.load_full()).clone()
    }

    pub fn on_change<F>(&mut self, f: F)
    where
        F: FnOnce(T),
    {
        if self.changed() {
            f(self.get_inner());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use tokio::sync::Notify;
    use tokio::time::{timeout, Duration};

    use super::*;

    #[tokio::test]
    async fn test_observed_new() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::new(42, &notify);
        let inner = observed.get_inner();
        assert_eq!(inner, 42);
    }

    #[tokio::test]
    async fn test_observed_set() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::new(42, &notify);
        observed.set(123);
        let inner = observed.get_inner();
        assert_eq!(inner, 123);

        // Check that the Notify was triggered
        let notification = timeout(Duration::from_secs(1), notify.notified()).await;
        assert!(notification.is_ok());
    }

    #[tokio::test]
    async fn test_observed_rcu() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::new(42, &notify);

        observed.rcu(|inner| {
            assert_eq!(*inner, 42);
            41
        });

        let inner = observed.get_inner();
        assert_eq!(inner, 41);

        // Check that the Notify was triggered
        let notification = timeout(Duration::from_secs(1), notify.notified()).await;
        assert!(notification.is_ok());
    }

    #[tokio::test]
    async fn test_observed_changed() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::default_with_notify(&notify);

        assert!(!observed.changed());
        assert_eq!(observed.get_inner(), i32::default());
        observed.set(321);
        assert!(observed.changed());
        assert_eq!(observed.get_inner(), 321);

        // Check that the Notify was triggered
        let notification = timeout(Duration::from_secs(1), notify.notified()).await;
        assert!(notification.is_ok());
    }

    #[tokio::test]
    async fn test_observed_on_change() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::new(42, &notify);

        let observed_clone = observed.clone();
        tokio::spawn(async move {
            observed_clone.set(123);
        })
        .await
        .unwrap();

        let mut observed_clone2 = observed.clone();
        let result = tokio::spawn(async move {
            let mut changed = false;
            observed_clone2.on_change(|inner| {
                assert_eq!(inner, 123);
                changed = true;
            });
            changed
        })
        .await
        .unwrap();

        assert!(result);

        // Check that the Notify was triggered
        let notification = timeout(Duration::from_secs(1), notify.notified()).await;
        assert!(notification.is_ok());
    }

    #[tokio::test]
    async fn test_observed_on_change_arc() {
        let notify = Arc::new(Notify::new());
        let observed = Observed::new(42, &notify);

        let observed_clone = observed.clone();
        tokio::spawn(async move {
            observed_clone.set(123);
        })
        .await
        .unwrap();

        let mut observed_clone2 = observed.clone();
        let result = tokio::spawn(async move {
            let mut changed = false;
            observed_clone2.on_change_arc(|inner| {
                assert_eq!(*inner, 123);
                changed = true;
            });
            changed
        })
        .await
        .unwrap();

        assert!(result);

        // Check that the Notify was triggered
        let notification = timeout(Duration::from_secs(1), notify.notified()).await;
        assert!(notification.is_ok());
    }
}
